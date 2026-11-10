//! The facade the Swift and Kotlin shells call (T6). Shells hand in records
//! from their read-only adapters and render what comes back; they never touch
//! the store or retrieval directly.
//!
//! Without an embedding model, search runs keyword-only and returns the
//! degraded notice for the shell to show (T9). Without an answer model, the
//! shell shows ranked passages only.

use crate::bert::BertEmbedder;
use crate::embed::Embedder;
use crate::error::CoreError;
use crate::generate::{
    attribute, build_prompt, check_citations, says_not_found, Generator, PromptPassage,
    ANSWER_PASSAGES, MAX_NEW_TOKENS, NOT_FOUND,
};
#[cfg(not(target_os = "android"))]
use crate::qwen::QwenGenerator;
use crate::record::{Record, SourceKind};
use crate::retrieve::hybrid_search;
use crate::store::{Filters, Store, UpsertOutcome};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Errors the shell can show as a plain message.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SeeqoraError {
    /// The user has not switched this source on (T3).
    #[error("{0}")]
    NoConsent(String),
    #[error("{0}")]
    Storage(String),
    /// The answer model is missing, busy, or failed to run.
    #[error("{0}")]
    Model(String),
}

impl From<CoreError> for SeeqoraError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NoConsent(_) => SeeqoraError::NoConsent(e.to_string()),
            CoreError::Generation(m) => SeeqoraError::Model(m),
            other => SeeqoraError::Storage(other.to_string()),
        }
    }
}

/// Receives the answer as it is written. Return `false` to stop.
#[uniffi::export(callback_interface)]
pub trait AnswerListener: Send + Sync {
    fn on_text(&self, text: String) -> bool;
}

/// Timings for the debug line, so speed can be read off the phone.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct AnswerStats {
    pub model_id: String,
    pub retrieve_ms: u64,
    pub prompt_tokens: u32,
    pub generated_tokens: u32,
    pub prefill_ms: u64,
    pub decode_ms: u64,
    pub stopped: bool,
}

/// A cited answer (T2). Passage `n` in the text is `passages[n - 1]`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Answer {
    /// The answer with invalid citations removed.
    pub text: String,
    /// False when the model found no answer or cited nothing valid: the shell
    /// then says so and shows the passages as ranked results (T9).
    pub found: bool,
    /// Passage numbers cited, in order of first use.
    pub cited: Vec<u32>,
    /// The model gave no citation and Seeqora matched the source itself;
    /// the shell must label it that way.
    pub attributed: bool,
    pub passages: Vec<SearchHit>,
    pub degraded: Option<String>,
    pub stats: AnswerStats,
}

type FfiResult<T> = std::result::Result<T, SeeqoraError>;

/// One item from a platform adapter.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SourceRecord {
    pub source: SourceKind,
    pub item_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub metadata: HashMap<String, String>,
    pub uri: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SourceStatus {
    pub source: SourceKind,
    pub granted: bool,
    pub items: i64,
}

/// What switching a source off removed (T3).
#[derive(Debug, Clone, uniffi::Record)]
pub struct RevokeSummary {
    pub items: i64,
    pub chunks: i64,
    pub embeddings: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct IndexSummary {
    pub indexed: i64,
    pub unchanged: i64,
}

/// One cited result (T2). `uri` is the tap-through to the original.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub chunk_id: String,
    pub source: SourceKind,
    pub item_id: String,
    pub text: String,
    pub uri: String,
    pub timestamp_ms: i64,
    pub metadata: HashMap<String, String>,
    pub score: f64,
    pub dense_rank: Option<i32>,
    pub keyword_rank: Option<i32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// Set when search ran in a reduced mode; the shell must show it (T9).
    pub degraded: Option<String>,
}

#[derive(uniffi::Object)]
pub struct Engine {
    store: Mutex<Store>,
    embedder: Mutex<Option<Arc<BertEmbedder>>>,
    /// Held for a whole answer; never held together with `store` (search
    /// finishes before generation starts), so search stays responsive.
    generator: Mutex<Option<Box<dyn Generator>>>,
}

impl Engine {
    fn new(store: Store) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            embedder: Mutex::new(None),
            generator: Mutex::new(None),
        })
    }

    fn generator(&self) -> MutexGuard<'_, Option<Box<dyn Generator>>> {
        self.generator.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[cfg(test)]
    fn set_generator(&self, g: Box<dyn Generator>) {
        *self.generator() = Some(g);
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        // A panic elsewhere must not lock the user out of their index.
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn embedder(&self) -> Option<Arc<BertEmbedder>> {
        self.embedder.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[uniffi::export]
impl Engine {
    /// Open (or create) the index file at `path`.
    #[uniffi::constructor]
    pub fn open(path: String) -> FfiResult<Arc<Self>> {
        // Before any model can start candle's worker pool.
        configure_threads();
        Ok(Self::new(Store::open(path)?))
    }

    #[uniffi::constructor]
    pub fn in_memory() -> FfiResult<Arc<Self>> {
        Ok(Self::new(Store::open_in_memory()?))
    }

    /// Load the embedding model from a model directory (see `bert`). Until
    /// this succeeds, search is keyword-only and says so (T9). Returns the
    /// model id.
    pub fn load_embedder(&self, dir: String) -> FfiResult<String> {
        let e = BertEmbedder::load(std::path::Path::new(&dir))?;
        let id = e.model_id().to_string();
        *self.embedder.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(e));
        Ok(id)
    }

    /// Chunks still waiting for a vector from the loaded model (0 if none loaded).
    pub fn embedding_backlog(&self) -> FfiResult<i64> {
        let Some(e) = self.embedder() else { return Ok(0) };
        Ok(self.store().count_needing_embedding(e.model_id())? as i64)
    }

    /// Embed up to `max` waiting chunks. The model runs outside the store lock,
    /// so search stays responsive. Returns how many were stored (0 = done).
    pub fn embed_next(&self, max: u32) -> FfiResult<i64> {
        let Some(e) = self.embedder() else { return Ok(0) };
        let pending = self.store().chunks_needing_embedding(e.model_id(), max as usize)?;
        if pending.is_empty() {
            return Ok(0);
        }
        let texts: Vec<&str> = pending.iter().map(|(_, t)| t.as_str()).collect();
        let vectors = e.embed(&texts)?;
        let rows: Vec<(String, Vec<f32>)> = pending.into_iter().map(|(id, _)| id).zip(vectors).collect();
        // Chunks deleted meanwhile are skipped by the store; the batch still counts as progress.
        self.store().put_embeddings(e.model_id(), &rows)?;
        Ok(rows.len() as i64)
    }

    pub fn sources(&self) -> FfiResult<Vec<SourceStatus>> {
        let s = self.store();
        SourceKind::ALL
            .iter()
            .map(|&k| -> FfiResult<SourceStatus> {
                Ok(SourceStatus {
                    source: k,
                    granted: s.is_consented(k)?,
                    items: s.count_items(k)? as i64,
                })
            })
            .collect()
    }

    pub fn grant(&self, source: SourceKind) -> FfiResult<()> {
        Ok(self.store().grant(source)?)
    }

    pub fn revoke(&self, source: SourceKind) -> FfiResult<RevokeSummary> {
        let r = self.store().revoke(source)?;
        Ok(RevokeSummary {
            items: r.items as i64,
            chunks: r.chunks as i64,
            embeddings: r.embeddings as i64,
        })
    }

    pub fn delete_everything(&self) -> FfiResult<()> {
        Ok(self.store().delete_everything()?)
    }

    /// Index records. Fails with `NoConsent` if any record's source is off.
    pub fn index(&self, records: Vec<SourceRecord>) -> FfiResult<IndexSummary> {
        let mut s = self.store();
        let mut out = IndexSummary {
            indexed: 0,
            unchanged: 0,
        };
        for r in records {
            let rec = Record {
                source: r.source,
                item_id: r.item_id,
                timestamp_ms: r.timestamp_ms,
                text: r.text,
                metadata: r.metadata.into_iter().collect(),
                uri: r.uri,
            };
            match s.upsert_record(&rec)? {
                UpsertOutcome::Unchanged => out.unchanged += 1,
                UpsertOutcome::Indexed { .. } => out.indexed += 1,
            }
        }
        Ok(out)
    }

    /// Item ids already indexed for a source, so adapters can skip costly
    /// re-extraction (photo OCR) of items the core already has (T5).
    pub fn indexed_ids(&self, source: SourceKind) -> FfiResult<Vec<String>> {
        Ok(self.store().item_ids(source)?)
    }

    /// After a full read of a source, remove items that are gone from the
    /// device. Returns how many were removed.
    pub fn retain_only(&self, source: SourceKind, item_ids: Vec<String>) -> FfiResult<i64> {
        Ok(self.store().retain_items(source, &item_ids)? as i64)
    }

    /// Search switched-on sources. Revoked sources have no rows left, so they
    /// can never appear.
    pub fn search(&self, query: String, top_n: u32) -> FfiResult<SearchResults> {
        let embedder = self.embedder();
        let dense = embedder.as_deref().map(|e| e as &dyn Embedder);
        let s = self.store();
        let r = hybrid_search(&s, dense, &query, &Filters::default(), top_n as usize)?;
        let mut hits = Vec::with_capacity(r.hits.len());
        for h in r.hits {
            if let Some(c) = s.get_chunk(&h.chunk_id)? {
                hits.push(SearchHit {
                    chunk_id: c.chunk_id,
                    source: c.source,
                    item_id: c.item_id,
                    text: c.text,
                    uri: c.uri,
                    timestamp_ms: c.timestamp_ms,
                    metadata: c.metadata.into_iter().collect(),
                    score: h.score,
                    dense_rank: h.dense_rank.map(|x| x as i32),
                    keyword_rank: h.keyword_rank.map(|x| x as i32),
                });
            }
        }
        Ok(SearchResults {
            hits,
            degraded: r.degraded,
        })
    }

    /// Load the answer model: a Qwen2.5 Instruct GGUF file plus the Qwen2.5
    /// `tokenizer.json` the shell ships. Returns the model name. Replaces any
    /// model already loaded.
    ///
    /// Android runs llama.cpp, which reads the tokenizer from the GGUF and
    /// ignores `tokenizer_path`; other targets still use candle.
    pub fn load_generator(&self, model_path: String, tokenizer_path: String) -> FfiResult<String> {
        // Free the old model first so two are never in memory at once.
        *self.generator() = None;
        let g: Box<dyn Generator> = load_answer_model(
            std::path::Path::new(&model_path),
            std::path::Path::new(&tokenizer_path),
        )?;
        let id = g.model_id().to_string();
        *self.generator() = Some(g);
        Ok(id)
    }

    /// Free the answer model's memory (T5). Waits for an answer in progress.
    pub fn unload_generator(&self) {
        *self.generator() = None;
    }

    pub fn generator_loaded(&self) -> bool {
        self.generator().is_some()
    }

    /// Answer a question from the user's own data, citing passages (T2).
    /// Retrieval is the same hybrid search the results list uses (T6).
    /// `utc_offset_minutes` puts dates in the prompt in local time.
    pub fn answer(
        &self,
        question: String,
        utc_offset_minutes: i32,
        listener: Box<dyn AnswerListener>,
    ) -> FfiResult<Answer> {
        let t0 = Instant::now();
        let found = self.search(question.clone(), ANSWER_PASSAGES as u32)?;
        let mut stats = AnswerStats {
            retrieve_ms: t0.elapsed().as_millis() as u64,
            ..Default::default()
        };
        let not_found = |passages: Vec<SearchHit>, stats: AnswerStats| Answer {
            text: NOT_FOUND.to_string(),
            found: false,
            cited: vec![],
            attributed: false,
            passages,
            degraded: found.degraded.clone(),
            stats,
        };
        if found.hits.is_empty() {
            return Ok(not_found(vec![], stats));
        }

        let mut guard = match self.generator.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return Err(SeeqoraError::Model("Already answering a question.".into()))
            }
        };
        let Some(generator) = guard.as_mut() else {
            return Err(SeeqoraError::Model("No answer model is loaded.".into()));
        };
        stats.model_id = generator.model_id().to_string();

        let prompt_passages: Vec<PromptPassage> = found
            .hits
            .iter()
            .map(|h| PromptPassage {
                label: passage_label(h),
                timestamp_ms: h.timestamp_ms,
                text: h.text.clone(),
            })
            .collect();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let (system, user) = build_prompt(&question, &prompt_passages, now_ms, utc_offset_minutes);
        let mut on_text = |t: &str| listener.on_text(t.to_string());
        let (raw, g) = generator.generate(&system, &user, MAX_NEW_TOKENS, &mut on_text)?;
        drop(guard);

        stats.prompt_tokens = g.prompt_tokens as u32;
        stats.generated_tokens = g.generated_tokens as u32;
        stats.prefill_ms = g.prefill_ms;
        stats.decode_ms = g.decode_ms;
        stats.stopped = g.stopped;

        let (mut text, mut cited) = check_citations(&raw, found.hits.len());
        if says_not_found(&text) {
            return Ok(not_found(found.hits, stats));
        }
        // Small models often answer correctly but forget the [n]. Cite the
        // passage the answer clearly came from, and say that Seeqora matched it.
        let mut attributed = false;
        if cited.is_empty() {
            let texts: Vec<&str> = found.hits.iter().map(|h| h.text.as_str()).collect();
            match attribute(&text, &texts) {
                Some(n) => {
                    text = format!("{} [{n}]", text.trim_end());
                    cited = vec![n];
                    attributed = true;
                }
                None => return Ok(not_found(found.hits, stats)),
            }
        }
        Ok(Answer {
            text,
            found: true,
            cited,
            attributed,
            passages: found.hits,
            degraded: found.degraded,
            stats,
        })
    }
}

/// Use only the fast cores for model math. candle's worker pool waits at a
/// barrier after every matrix multiply, so one slow little core holds back
/// all of them. Runs once, before any model loads; an explicit
/// CANDLE_NUM_THREADS / RAYON_NUM_THREADS in the environment wins.
#[cfg(target_os = "android")]
fn load_answer_model(model: &std::path::Path, _tokenizer: &std::path::Path) -> Result<Box<dyn Generator>, CoreError> {
    Ok(Box::new(crate::llama::LlamaGenerator::load(model, answer_threads())?))
}

#[cfg(not(target_os = "android"))]
fn load_answer_model(model: &std::path::Path, tokenizer: &std::path::Path) -> Result<Box<dyn Generator>, CoreError> {
    Ok(Box::new(QwenGenerator::load(model, tokenizer)?))
}

/// Threads for model work: the performance cores. Spreading a matmul over the
/// little cores makes every step wait for the slowest one.
fn answer_threads() -> usize {
    performance_cores().unwrap_or_else(|| {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
    })
}

fn configure_threads() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let n = answer_threads();
        for var in ["CANDLE_NUM_THREADS", "RAYON_NUM_THREADS"] {
            if std::env::var_os(var).is_none() {
                std::env::set_var(var, n.to_string());
            }
        }
    });
}

/// Cores whose top clock is within 70% of the fastest (Linux/Android sysfs).
fn performance_cores() -> Option<usize> {
    let mut freqs = Vec::new();
    for i in 0..256 {
        let path = format!("/sys/devices/system/cpu/cpu{i}/cpufreq/cpuinfo_max_freq");
        match std::fs::read_to_string(path) {
            Ok(s) => freqs.push(s.trim().parse::<u64>().ok()?),
            Err(_) => break,
        }
    }
    let max = *freqs.iter().max()?;
    let n = freqs.iter().filter(|&&f| f * 10 >= max * 7).count();
    (n > 0).then_some(n)
}

/// How the core was built and how it runs, for the answer timing line.
#[uniffi::export]
pub fn engine_info() -> String {
    let runtime = if cfg!(target_os = "android") { "llama.cpp" } else { "candle" };
    let dotprod = if cfg!(target_feature = "dotprod") { "on" } else { "off" };
    format!("{runtime}, {} threads, dotprod {dotprod}", answer_threads())
}

/// Who or what a passage came from, as the model sees it.
fn passage_label(h: &SearchHit) -> String {
    let get = |k: &str| h.metadata.get(k).map(String::as_str).filter(|s| !s.is_empty());
    match h.source {
        SourceKind::Messages => match get("sender") {
            Some("Me") => match get("to") {
                Some(to) => format!("Text message you sent to {to}"),
                None => "Text message you sent".to_string(),
            },
            Some(s) => format!("Text message from {s}"),
            None => "Text message".to_string(),
        },
        SourceKind::Contacts => format!("Contact card for {}", get("name").unwrap_or("a contact")),
        SourceKind::Calendar => format!("Calendar event \"{}\"", get("title").unwrap_or("untitled")),
        SourceKind::Photos => "Text read from a photo".to_string(),
        SourceKind::Files => format!("File {}", get("filename").unwrap_or("document")),
        SourceKind::Notes => format!("Note \"{}\"", get("title").unwrap_or("untitled")),
        other => other.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(source: SourceKind, id: &str, text: &str) -> SourceRecord {
        SourceRecord {
            source,
            item_id: id.into(),
            timestamp_ms: 1_760_000_000_000,
            text: text.into(),
            metadata: HashMap::from([("sender".to_string(), "Dana".to_string())]),
            uri: format!("{}://{id}", source.as_str()),
        }
    }

    #[test]
    fn consent_index_search_revoke() {
        let e = Engine::in_memory().unwrap();
        let wifi = rec(SourceKind::Messages, "m1", "cabin wifi password is Tamarack");
        assert!(matches!(
            e.index(vec![wifi.clone()]),
            Err(SeeqoraError::NoConsent(_))
        ));

        e.grant(SourceKind::Messages).unwrap();
        assert_eq!(e.index(vec![wifi.clone()]).unwrap().indexed, 1);
        assert_eq!(e.index(vec![wifi]).unwrap().unchanged, 1);

        // Prefix match on the last word while typing.
        assert_eq!(e.search("cabin wif".into(), 10).unwrap().hits[0].item_id, "m1");

        let r = e.search("wifi password".into(), 10).unwrap();
        assert!(r.degraded.is_some(), "keyword-only must be announced");
        assert_eq!(r.hits[0].item_id, "m1");
        assert_eq!(r.hits[0].uri, "messages://m1");
        assert_eq!(r.hits[0].metadata.get("sender").map(String::as_str), Some("Dana"));

        let status = e.sources().unwrap();
        let m = status.iter().find(|s| s.source == SourceKind::Messages).unwrap();
        assert!(m.granted);
        assert_eq!(m.items, 1);

        e.index(vec![rec(SourceKind::Messages, "m2", "dinner at eight")]).unwrap();
        assert_eq!(e.indexed_ids(SourceKind::Messages).unwrap(), vec!["m1", "m2"]);
        assert_eq!(e.retain_only(SourceKind::Messages, vec!["m1".into()]).unwrap(), 1);
        assert!(e.search("dinner".into(), 10).unwrap().hits.is_empty());

        assert_eq!(e.revoke(SourceKind::Messages).unwrap().items, 1);
        assert!(e.search("wifi".into(), 10).unwrap().hits.is_empty());
    }

    /// Replies with a fixed text, streamed word by word, and records the prompt.
    struct FakeGen {
        reply: String,
        seen: Arc<Mutex<String>>,
    }

    impl Generator for FakeGen {
        fn model_id(&self) -> &str {
            "fake@1"
        }
        fn generate(
            &mut self,
            system: &str,
            user: &str,
            _max: usize,
            on_text: &mut dyn FnMut(&str) -> bool,
        ) -> crate::Result<(String, crate::generate::GenStats)> {
            *self.seen.lock().unwrap() = format!("{system}\n{user}");
            for w in self.reply.split_inclusive(' ') {
                if !on_text(w) {
                    break;
                }
            }
            Ok((self.reply.clone(), Default::default()))
        }
    }

    struct Collect(Arc<Mutex<String>>);
    impl AnswerListener for Collect {
        fn on_text(&self, text: String) -> bool {
            self.0.lock().unwrap().push_str(&text);
            true
        }
    }

    fn engine_with(reply: &str) -> (Arc<Engine>, Arc<Mutex<String>>) {
        let e = Engine::in_memory().unwrap();
        e.grant(SourceKind::Messages).unwrap();
        e.index(vec![rec(SourceKind::Messages, "m1", "cabin wifi password is Tamarack")])
            .unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        e.set_generator(Box::new(FakeGen {
            reply: reply.into(),
            seen: seen.clone(),
        }));
        (e, seen)
    }

    #[test]
    fn answer_cites_passages_and_drops_invalid_numbers() {
        let (e, seen) = engine_with("The cabin wifi password is Tamarack [1] [4].");
        let streamed = Arc::new(Mutex::new(String::new()));
        let a = e
            .answer("cabin wifi password".into(), 0, Box::new(Collect(streamed.clone())))
            .unwrap();
        assert!(a.found);
        assert_eq!(a.text, "The cabin wifi password is Tamarack [1].");
        assert_eq!(a.cited, vec![1]);
        assert_eq!(a.passages[0].item_id, "m1");
        assert_eq!(a.stats.model_id, "fake@1");
        assert_eq!(*streamed.lock().unwrap(), "The cabin wifi password is Tamarack [1] [4].");
        let prompt = seen.lock().unwrap();
        assert!(prompt.contains("[1] Text message from Dana ("), "{prompt}");
        assert!(prompt.contains("Question: cabin wifi password"));
    }

    #[test]
    fn uncited_answer_is_matched_to_its_source_and_labelled() {
        let (e, _) = engine_with("The cabin password is Tamarack.");
        let a = e
            .answer("cabin wifi password".into(), 0, Box::new(Collect(Default::default())))
            .unwrap();
        assert!(a.found);
        assert!(a.attributed, "a Seeqora-matched citation must be labelled as such");
        assert_eq!(a.cited, vec![1]);
        assert_eq!(a.text, "The cabin password is Tamarack. [1]");
        assert!(!engine_info().is_empty());
    }

    #[test]
    fn uncited_or_refused_answers_are_not_found() {
        let (e, _) = engine_with("It is purple.");
        let a = e
            .answer("cabin wifi password".into(), 0, Box::new(Collect(Default::default())))
            .unwrap();
        assert!(!a.found, "an answer with no citation and no clear source must not be shown");
        assert_eq!(a.text, NOT_FOUND);
        assert_eq!(a.passages.len(), 1, "passages still shown as ranked results");

        let (e, _) = engine_with(NOT_FOUND);
        let a = e
            .answer("cabin wifi password".into(), 0, Box::new(Collect(Default::default())))
            .unwrap();
        assert!(!a.found);
    }

    #[test]
    fn no_passages_skips_the_model_and_no_model_is_an_error() {
        let (e, seen) = engine_with("should not run [1]");
        let a = e
            .answer("dentist appointment".into(), 0, Box::new(Collect(Default::default())))
            .unwrap();
        assert!(!a.found);
        assert!(a.passages.is_empty());
        assert!(seen.lock().unwrap().is_empty(), "model must not run without passages");

        e.unload_generator();
        assert!(!e.generator_loaded());
        assert!(matches!(
            e.answer("wifi".into(), 0, Box::new(Collect(Default::default()))),
            Err(SeeqoraError::Model(_))
        ));
    }
}
