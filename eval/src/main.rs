//! Golden-set retrieval evaluation (T10: measure retrieval before polishing chat).
//!
//! Loads the synthetic phone in `<golden>/fixtures/*.jsonl` into an in-memory
//! core store, runs every question in `<golden>/questions.jsonl` through
//! `hybrid_search`, and reports:
//!
//! - **Recall@k**: share of answerable questions with at least one expected
//!   item among the items of the top-k retrieved chunks (the doctrine's
//!   "correct item in top k chunks").
//! - **Coverage@10**: share of *all* expected items found in the top 10
//!   (matters for multi-source questions).
//!
//! Modes: by default, questions run with no filters (query rewrite lands in W5).
//! `--oracle-filters` applies each question's `expected_filters` to measure
//! retrieval as if the rewrite were perfect.
//!
//! Exit code is non-zero if `--gate` is set and a bar fails, or if `--baseline`
//! is given and any metric dropped more than 2 points.

use chrono::DateTime;
use seeqora_core::embed::{Embedder, HashEmbedder};
use seeqora_core::record::{Record, SourceKind};
use seeqora_core::retrieve::{embed_pending, hybrid_search};
use seeqora_core::store::{Filters, Store};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const BAR_RECALL_10: f64 = 0.90;
const BAR_RECALL_3: f64 = 0.75;
const MAX_DROP: f64 = 0.02;

#[derive(Deserialize)]
struct FixtureItem {
    source: SourceKind,
    item_id: String,
    time: String,
    text: String,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
struct ExpectedItem {
    source: SourceKind,
    item_id: String,
}

#[derive(Deserialize, Default)]
struct ExpectedFilters {
    source: Option<SourceKind>,
    from: Option<String>,
    to: Option<String>,
    sender: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Question {
    id: String,
    kind: String,
    question: String,
    now: String,
    expected_items: Vec<ExpectedItem>,
    #[serde(default)]
    expected_filters: Option<ExpectedFilters>,
    #[serde(default)]
    notes: Option<String>,
}

struct Args {
    golden: PathBuf,
    oracle_filters: bool,
    keyword_only: bool,
    json_out: Option<PathBuf>,
    baseline: Option<PathBuf>,
    gate: bool,
}

struct Outcome {
    id: String,
    kind: String,
    question: String,
    answerable: bool,
    hit3: bool,
    hit10: bool,
    expected: usize,
    found10: usize,
    top3: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        golden: PathBuf::from("eval/golden"),
        oracle_filters: false,
        keyword_only: false,
        json_out: None,
        baseline: None,
        gate: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--golden" => a.golden = it.next().ok_or("--golden needs a path")?.into(),
            "--oracle-filters" => a.oracle_filters = true,
            "--keyword-only" => a.keyword_only = true,
            "--json" => a.json_out = Some(it.next().ok_or("--json needs a path")?.into()),
            "--baseline" => a.baseline = Some(it.next().ok_or("--baseline needs a path")?.into()),
            "--gate" => a.gate = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(a)
}

fn ms(s: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis())
        .map_err(|e| format!("bad time '{s}': {e}"))
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))
        })
        .collect()
}

fn build_store(golden: &Path, embedder: &dyn Embedder) -> Result<(Store, usize), String> {
    let mut store = Store::open_in_memory().map_err(|e| e.to_string())?;
    for s in SourceKind::ALL {
        store.grant(s).map_err(|e| e.to_string())?;
    }
    let dir = golden.join("fixtures");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "jsonl"))
        .collect();
    files.sort();

    let mut items = 0;
    for f in files {
        for it in read_jsonl::<FixtureItem>(&f)? {
            let uri = it
                .uri
                .clone()
                .unwrap_or_else(|| format!("{}://{}", it.source.as_str(), it.item_id));
            let rec = Record {
                source: it.source,
                item_id: it.item_id,
                timestamp_ms: ms(&it.time)?,
                text: it.text,
                metadata: it.metadata,
                uri,
            };
            store.upsert_record(&rec).map_err(|e| e.to_string())?;
            items += 1;
        }
    }
    while embed_pending(&mut store, embedder).map_err(|e| e.to_string())? > 0 {}
    Ok((store, items))
}

fn to_filters(f: Option<&ExpectedFilters>) -> Result<Filters, String> {
    let Some(f) = f else {
        return Ok(Filters::default());
    };
    Ok(Filters {
        source: f.source,
        from_ms: f.from.as_deref().map(ms).transpose()?,
        to_ms: f.to.as_deref().map(ms).transpose()?,
        sender: f.sender.clone(),
    })
}

fn evaluate(args: &Args) -> Result<(Vec<Outcome>, usize, usize, String), String> {
    let embedder = HashEmbedder::new(256);
    let (store, items) = build_store(&args.golden, &embedder)?;
    let chunks = store.count_chunks().map_err(|e| e.to_string())?;
    let questions: Vec<Question> = read_jsonl(&args.golden.join("questions.jsonl"))?;

    let mut out = Vec::new();
    for q in &questions {
        ms(&q.now)?;
        let filters = if args.oracle_filters {
            to_filters(q.expected_filters.as_ref())?
        } else {
            Filters::default()
        };
        let dense: Option<&dyn Embedder> = if args.keyword_only { None } else { Some(&embedder) };
        let r = hybrid_search(&store, dense, &q.question, &filters, 10)
            .map_err(|e| e.to_string())?;
        let mut ranked: Vec<(SourceKind, String)> = Vec::new();
        for h in &r.hits {
            if let Some(c) = store.get_chunk(&h.chunk_id).map_err(|e| e.to_string())? {
                ranked.push((c.source, c.item_id));
            }
        }
        let expected: HashSet<(SourceKind, String)> = q
            .expected_items
            .iter()
            .map(|x| (x.source, x.item_id.clone()))
            .collect();
        let in_top = |k: usize| -> HashSet<&(SourceKind, String)> {
            ranked.iter().take(k).filter(|i| expected.contains(*i)).collect()
        };
        out.push(Outcome {
            id: q.id.clone(),
            kind: q.kind.clone(),
            question: q.question.clone(),
            answerable: !expected.is_empty(),
            hit3: !in_top(3).is_empty(),
            hit10: !in_top(10).is_empty(),
            expected: expected.len(),
            found10: in_top(10).len(),
            top3: ranked
                .iter()
                .take(3)
                .map(|(s, id)| format!("{}/{}", s.as_str(), id))
                .collect(),
        });
    }
    let model = if args.keyword_only {
        "none (keyword only)".to_string()
    } else {
        embedder.model_id().to_string()
    };
    Ok((out, items, chunks, model))
}

fn ratio(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

fn mark(v: f64, bar: f64) -> &'static str {
    if v >= bar {
        "✅"
    } else {
        "❌"
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (outcomes, items, chunks, model_id) = match evaluate(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("evaluation failed: {e}");
            return ExitCode::from(2);
        }
    };

    let ans: Vec<&Outcome> = outcomes.iter().filter(|o| o.answerable).collect();
    let negatives = outcomes.len() - ans.len();
    let r3 = ratio(ans.iter().filter(|o| o.hit3).count(), ans.len());
    let r10 = ratio(ans.iter().filter(|o| o.hit10).count(), ans.len());
    let cov10 = ratio(
        ans.iter().map(|o| o.found10).sum(),
        ans.iter().map(|o| o.expected).sum(),
    );
    let mode = if args.oracle_filters { "oracle filters" } else { "no filters" };

    println!("## Golden set retrieval ({mode})\n");
    println!(
        "Embedder `{model_id}` · {items} items · {chunks} chunks · {} questions ({} answerable, {negatives} negative)\n",
        outcomes.len(),
        ans.len()
    );
    println!("| Metric | Value | Bar |\n|---|---|---|");
    println!("| Recall@10 | {r10:.3} | ≥ {BAR_RECALL_10:.2} {} |", mark(r10, BAR_RECALL_10));
    println!("| Recall@3 | {r3:.3} | ≥ {BAR_RECALL_3:.2} {} |", mark(r3, BAR_RECALL_3));
    println!("| Coverage@10 | {cov10:.3} | — |");
    println!("| Unanswerable handled | n/a (needs the generator, Phase 1) | ≥ 0.95 |\n");

    let mut kinds: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for o in &ans {
        let e = kinds.entry(o.kind.as_str()).or_default();
        e.0 += 1;
        e.1 += o.hit3 as usize;
        e.2 += o.hit10 as usize;
    }
    println!("| Kind | n | Recall@3 | Recall@10 |\n|---|---|---|---|");
    for (k, (n, h3, h10)) in &kinds {
        println!("| {k} | {n} | {:.3} | {:.3} |", ratio(*h3, *n), ratio(*h10, *n));
    }

    let misses: Vec<&&Outcome> = ans.iter().filter(|o| !o.hit10).collect();
    if !misses.is_empty() {
        println!("\n### Misses at 10\n");
        for o in misses {
            println!(
                "- `{}` ({}) {} → top: {}",
                o.id,
                o.kind,
                o.question,
                if o.top3.is_empty() { "nothing".to_string() } else { o.top3.join(", ") }
            );
        }
    }

    let mut failed = false;
    if let Some(p) = &args.json_out {
        let json = serde_json::json!({
            "mode": mode,
            "embedder": model_id,
            "items": items,
            "chunks": chunks,
            "answerable": ans.len(),
            "negative": negatives,
            "recall_at_3": r3,
            "recall_at_10": r10,
            "coverage_at_10": cov10,
        });
        if let Err(e) = fs::write(p, serde_json::to_string_pretty(&json).unwrap_or_default()) {
            eprintln!("could not write {}: {e}", p.display());
            failed = true;
        }
    }

    if let Some(p) = &args.baseline {
        match fs::read_to_string(p)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(base) => {
                for (name, now) in [("recall_at_3", r3), ("recall_at_10", r10), ("coverage_at_10", cov10)] {
                    if let Some(b) = base.get(name).and_then(|v| v.as_f64()) {
                        if now + 1e-9 < b - MAX_DROP {
                            println!("\n❌ Regression: {name} {now:.3} vs baseline {b:.3} (max drop {MAX_DROP})");
                            failed = true;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("could not read baseline {}: {e}", p.display());
                failed = true;
            }
        }
    }

    if args.gate && (r10 < BAR_RECALL_10 || r3 < BAR_RECALL_3) {
        println!("\n❌ Gate failed: Recall@10 must be ≥ {BAR_RECALL_10} and Recall@3 ≥ {BAR_RECALL_3}");
        failed = true;
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
