//! Qwen2.5 Instruct in GGUF form, run on the CPU by candle (pure Rust, the same
//! stack as the embedder, so iOS and Android build it identically: T6).
//!
//! The model file is imported by the user. Its tokenizer is not inside the
//! GGUF in a form `tokenizers` reads, so the shell ships Qwen2.5's
//! `tokenizer.json` (shared by every Qwen2.5 size) and passes both paths.
//! Any architecture other than `qwen2` is refused at load with a plain message.

use crate::error::{CoreError, Result};
use crate::generate::{GenStats, Generator};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen2::ModelWeights;
use std::path::Path;
use std::time::Instant;
use tokenizers::Tokenizer;

/// Prompt tokens per forward pass. Bounds the attention scratch memory.
const PREFILL_CHUNK: usize = 256;
const REPEAT_PENALTY: f32 = 1.1;
const REPEAT_LAST_N: usize = 64;

fn err<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Generation(e.to_string())
}

pub struct QwenGenerator {
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    id: String,
    stop: Vec<u32>,
    context: usize,
}

impl QwenGenerator {
    pub fn load(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(model_path)
            .map_err(|e| err(format!("can't open {}: {e}", model_path.display())))?;
        let content = gguf_file::Content::read(&mut file)
            .map_err(|e| err(format!("not a GGUF model file ({e})")))?;
        let meta_str = |k: &str| {
            content
                .metadata
                .get(k)
                .and_then(|v| v.to_string().ok())
                .cloned()
        };
        let arch = meta_str("general.architecture").unwrap_or_default();
        if arch != "qwen2" {
            return Err(err(format!(
                "this model is '{arch}'; Seeqora answers need a Qwen2.5 Instruct GGUF file"
            )));
        }
        let id = meta_str("general.name").unwrap_or_else(|| "qwen2".to_string());
        let context = content
            .metadata
            .get("qwen2.context_length")
            .and_then(|v| v.to_u32().ok())
            .unwrap_or(4096) as usize;
        let device = Device::Cpu;
        let model = ModelWeights::from_gguf(content, &mut file, &device).map_err(err)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(err)?;
        let stop: Vec<u32> = ["<|im_end|>", "<|endoftext|>"]
            .iter()
            .filter_map(|t| tokenizer.token_to_id(t))
            .collect();
        if stop.is_empty() {
            return Err(err("tokenizer has no Qwen end-of-turn token"));
        }
        Ok(Self {
            model,
            tokenizer,
            device,
            id,
            stop,
            context,
        })
    }

    fn step(&mut self, ids: &[u32], pos: usize) -> Result<Tensor> {
        let input = Tensor::new(ids, &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(err)?;
        self.model.forward(&input, pos).map_err(err)
    }
}

impl Generator for QwenGenerator {
    fn model_id(&self) -> &str {
        &self.id
    }

    fn generate(
        &mut self,
        system: &str,
        user: &str,
        max_new_tokens: usize,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(String, GenStats)> {
        // Qwen2.5 chat template (ChatML).
        let prompt = format!(
            "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
        );
        let prompt_ids = self
            .tokenizer
            .encode(prompt, false)
            .map_err(err)?
            .get_ids()
            .to_vec();
        if prompt_ids.is_empty() {
            return Err(err("empty prompt"));
        }
        if prompt_ids.len() + max_new_tokens > self.context {
            return Err(err(format!(
                "prompt of {} tokens is too long for this model",
                prompt_ids.len()
            )));
        }
        let mut stats = GenStats {
            prompt_tokens: prompt_ids.len(),
            ..Default::default()
        };

        self.model.clear_kv_cache();
        let t0 = Instant::now();
        let mut pos = 0;
        let mut logits = None;
        for chunk in prompt_ids.chunks(PREFILL_CHUNK) {
            logits = Some(self.step(chunk, pos)?);
            pos += chunk.len();
        }
        let mut logits = logits.ok_or_else(|| err("empty prompt"))?;
        stats.prefill_ms = t0.elapsed().as_millis() as u64;

        // Greedy decoding: answers should be the most likely reading of the
        // sources, and the same question should give the same answer.
        let mut sampler = LogitsProcessor::from_sampling(0, Sampling::ArgMax);
        let mut out: Vec<u32> = Vec::new();
        let mut text = String::new();
        let t1 = Instant::now();
        for _ in 0..max_new_tokens {
            let l = logits
                .squeeze(0)
                .and_then(|l| l.to_dtype(DType::F32))
                .map_err(err)?;
            let l = if out.is_empty() {
                l
            } else {
                let start = out.len().saturating_sub(REPEAT_LAST_N);
                candle_transformers::utils::apply_repeat_penalty(&l, REPEAT_PENALTY, &out[start..])
                    .map_err(err)?
            };
            let next = sampler.sample(&l).map_err(err)?;
            if self.stop.contains(&next) {
                break;
            }
            out.push(next);

            // Emit only complete characters; a multi-byte character can span tokens.
            let full = self.tokenizer.decode(&out, true).map_err(err)?;
            if full.ends_with('\u{FFFD}') {
                // Wait for the rest of the character.
            } else if full.starts_with(&text) {
                let keep_going = full.len() == text.len() || on_text(&full[text.len()..]);
                text = full;
                if !keep_going {
                    stats.stopped = true;
                    break;
                }
            } else {
                // Decoding rewrote earlier text; the final answer replaces the stream.
                text = full;
            }

            logits = self.step(&[next], pos)?;
            pos += 1;
        }
        stats.generated_tokens = out.len();
        stats.decode_ms = t1.elapsed().as_millis() as u64;
        let answer = self.tokenizer.decode(&out, true).map_err(err)?;
        Ok((answer, stats))
    }
}
