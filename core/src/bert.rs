//! Sentence embeddings from a small BERT-family model (MiniLM, BGE, Arctic),
//! run on the CPU in pure Rust via candle, identically on iOS and Android (T6).
//!
//! A model is a directory holding `config.json`, `tokenizer.json`,
//! `model.safetensors` and a `seeqora.json` manifest:
//! `{"id": "bge-small-en-v1.5@1", "pooling": "cls", "query_prefix": "..."}`.
//! The id is stored with every vector, so swapping models re-embeds instead
//! of mixing vector spaces (T8).

use crate::embed::Embedder;
use crate::error::{CoreError, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use serde::Deserialize;
use std::path::Path;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

const MAX_TOKENS: usize = 512;
const BATCH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pooling {
    /// Masked mean over token vectors (sentence-transformers MiniLM).
    Mean,
    /// The [CLS] token vector (BGE, Arctic).
    Cls,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub pooling: Pooling,
    /// Prepended to queries only (BGE/Arctic retrieval instruction).
    #[serde(default)]
    pub query_prefix: String,
}

pub struct BertEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    manifest: Manifest,
    dims: usize,
}

fn err<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Embedding(e.to_string())
}

impl BertEmbedder {
    /// Load a model directory (see module docs).
    pub fn load(dir: &Path) -> Result<Self> {
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| err(format!("{name}: {e}")));
        let manifest: Manifest = serde_json::from_slice(&read("seeqora.json")?)?;
        let config: Config = serde_json::from_slice(&read("config.json")?)?;
        let mut tokenizer = Tokenizer::from_bytes(read("tokenizer.json")?).map_err(err)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(err)?;
        let device = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(read("model.safetensors")?, DType::F32, &device)
            .map_err(err)?;
        let model = BertModel::load(vb, &config).map_err(err)?;
        Ok(Self {
            model,
            tokenizer,
            device,
            manifest,
            dims: config.hidden_size,
        })
    }

    /// Unit-length embeddings for one batch.
    fn encode(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let enc = self.tokenizer.encode_batch(texts, true).map_err(err)?;
        let ids = enc
            .iter()
            .map(|e| Tensor::new(e.get_ids(), &self.device))
            .collect::<candle_core::Result<Vec<_>>>()
            .map_err(err)?;
        let mask = enc
            .iter()
            .map(|e| Tensor::new(e.get_attention_mask(), &self.device))
            .collect::<candle_core::Result<Vec<_>>>()
            .map_err(err)?;
        let run = || -> candle_core::Result<Vec<Vec<f32>>> {
            let ids = Tensor::stack(&ids, 0)?;
            let mask = Tensor::stack(&mask, 0)?;
            let type_ids = ids.zeros_like()?;
            let out = self.model.forward(&ids, &type_ids, Some(&mask))?; // [batch, tokens, hidden]
            let pooled = match self.manifest.pooling {
                Pooling::Cls => out.narrow(1, 0, 1)?.squeeze(1)?,
                Pooling::Mean => {
                    let m = mask.to_dtype(DType::F32)?.unsqueeze(2)?; // [batch, tokens, 1]
                    let sum = out.broadcast_mul(&m)?.sum(1)?; // [batch, hidden]
                    let count = m.sum(1)?; // [batch, 1]
                    sum.broadcast_div(&count)?
                }
            };
            let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
            pooled.broadcast_div(&norm)?.to_vec2::<f32>()
        };
        run().map_err(err)
    }
}

impl Embedder for BertEmbedder {
    fn model_id(&self) -> &str {
        &self.manifest.id
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            out.extend(self.encode(batch.iter().map(|s| s.to_string()).collect())?);
        }
        Ok(out)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.encode(vec![format!("{}{}", self.manifest.query_prefix, query)])?
            .into_iter()
            .next()
            .ok_or_else(|| err("empty query embedding"))
    }
}
