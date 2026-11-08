# Seeqora: Build Plan

*Started 2026-11-07. This plan schedules work and the build doctrine sets the rules it must follow. Every line item cites the tenet(s) it serves as `[T#]`, matching the ranked tenets below. If a tenet changes, it is logged first in [DECISION_LOG.md](DECISION_LOG.md).*

## Tenets (reference)

| # | Tenet |
|---|---|
| T1 | The phone is the server |
| T2 | Every answer cites its source |
| T3 | Permission before ingestion |
| T4 | Fresh over complete |
| T5 | Battery and thermals are features |
| T6 | One core, two shells |
| T7 | Deterministic retrieval, explainable ranking |
| T8 | Small models, swappable |
| T9 | Degrade, never fail silent |
| T10 | Measure retrieval before polishing chat |

When two tenets conflict, the higher-numbered one yields.

## Roadmap: 4 phases, 3 gates, about 24 weeks

| Phase | Weeks | Scope | Gate to exit (must hold on **both** platforms) |
|---|---|---|---|
| **0 Foundation** | 1–6 | Rust core, SQLite index, files + notes, search only | **Recall@10 ≥ 0.90** on the golden set |
| **1 Alpha** | 7–12 | Photos/OCR, local LLM, both shells, 10 testers | **Performance budgets green** |
| **2 Beta** | 13–20 | Messages, email, calendar, background indexing, media | **Privacy review passed** |
| **3 Launch** | 21–24 | Store release, cloud opt-in, export/import, support | Store approval on both stores |

A phase does not start until the previous gate holds on both platforms. If a gate slips, the plan moves later. The bar does not move.

**Working rules (all phases)**
- Every week ends with a build a tester can install on both platforms (TestFlight + Play internal track). [T6]
- Core changes land with their tests and a golden-set run. UI changes land with a screenshot. [T10]
- Each doctrine section has one owner (see Owners below).

---

## Phase 0: Foundation (weeks 1–6)

Goal: files and notes indexed on-device, with hybrid search and citations, measured against the golden set. There is no chat UI in this phase. [T10]

| Week | Deliverable | Tenets | Done when |
|---|---|---|---|
| **W1** | Repo, CI, core skeleton: normalized `Record` schema, natural-unit chunker with stable ids, SQLite schema (items / chunks / embeddings / FTS5 / sources), keyword + brute-force dense search, RRF fusion, consent + revoke-deletes | T3, T6, T7 | `cargo test` green in CI ✅ *(this commit)* |
| W1 | Empty SwiftUI + Compose shells that link the core through UniFFI (XCFramework / `.aar`) and call `core_version()` | T6 | Installable build on TestFlight and Play internal track |
| **W2** | Golden set v0: synthetic phone fixtures (notes, files, contacts, events, messages) + first 50 questions with known source items; Rust eval CLI reporting recall@3 / recall@10 / unanswerable rate | T10 | Harness runs in CI and posts metrics to the PR |
| W2 | Text-to-tokens: replace whitespace token estimate with the candidate embedding model's tokenizer | T7, T8 | Chunk sizes measured in real tokens |
| **W3** | Files + notes adapters. iOS: Files app with security-scoped bookmarks, Notes via export. Android: Storage Access Framework, Keep via export. PDF text extraction in core | T3, T6 | Adapter contract tests: both adapters emit identical records for the same fixtures; each adapter < 300 lines |
| W3 | Per-source consent screen: one toggle per source, off by default, with one plain sentence saying what will be read and why | T3 | Revoke shows a count of what was removed |
| **W4** | Embedding runtime in core (ONNX Runtime; Core ML/ANE and NNAPI/LiteRT exports of the same weights) | T1, T8 | Same query gives same top-10 on both platforms |
| W4 | **Embedding model bake-off:** 3 permissive-licensed candidates (int8, < 150 MB, 384–768 dims) through the golden set; pick by recall first, latency second | T8, T10 | Decision log row "Which embedding model ships first" closed |
| W4 | Swap brute-force dense search for sqlite-vec | T7 | Query-to-first-result < 300 ms on Pixel 6a / iPhone 13 |
| **W5** | Query rewrite for time expressions ("last Tuesday" becomes an absolute range); metadata filters (source, date, sender) applied **before** fusion | T7 | Temporal golden questions pass |
| W5 | Debug "why retrieved" view in both shells: filters, dense rank, keyword rank, fused score | T7 | Every result shows its ranking breakdown |
| W5 | Golden set grows to 200 questions (lookup, temporal, multi-source, negative) | T10 | — |
| **W6** | Chaos cases: corrupt DB, missing model, permission revoked mid-index, low storage. Each must show a visible message and never crash | T9 | Chaos suite green on both platforms |
| W6 | Foreground mini-pass on app open (only items new since the last run, ≤ 2 s CPU); idempotent re-index by content hash | T4, T5 | New item searchable < 60 s after app open |
| W6 | "Delete everything" | T3 | Wipes the DB, models and caches in one action |
| **Gate 0** | **Recall@10 ≥ 0.90** on the golden set, both platforms | T10 | Nightly device-farm run |

## Phase 1: Alpha (weeks 7–12)

Goal: the **MVP**. That means one codebase; files, notes and photos indexed on-device; hybrid search with citations; on-device answers on supported devices and ranked snippets on the rest; per-source consent; delete-everything. Nothing else.

| Deliverable | Tenets |
|---|---|
| Photos/OCR adapters: PhotoKit + Vision / MediaStore + ML Kit, with an identical record schema; one chunk per caption + OCR block; never re-OCR an item | T3, T5, T6 |
| Photo change detection through OS observers (PhotoKit change observer, ContentObserver), never periodic scans | T4, T5 |
| `Generator` interface in core: llama.cpp (Metal / Vulkan) as the fallback; Apple Foundation Models and Gemini Nano (AICore) preferred when present | T1, T8 |
| Cited prompt assembly: the prompt contains only the top chunks with their ids; the model emits `[n]`; the shell maps each `[n]` to a tap-through of the source item | T2 |
| Retrieval-only answers (ranked snippets + citations) on devices below the RAM floor, and for plain lookups | T5, T8, T9 |
| Lazy generator load on the first question; unload after 5 min idle | T5 |
| Optional cross-encoder rerank to top-8 (devices with 6 GB+ RAM) | T7, T8 |
| Access control: biometric unlock (default on after first index); `FLAG_SECURE` on Android; blur in the iOS app switcher | T1 |
| Model manager: versioned assets (model_id, semver, SHA-256, license, model card); resumable, hash-verified, Wi-Fi-only downloads | T8 |
| Per-query local trace (filters, top chunks + scores, prompt tokens, model id, latency), viewable and shareable by the user only | T7, T9 |
| **Measure tokens/s + thermal throttling on 6 GB devices** to close the RAM-floor question | T5, T8 |
| Alpha survey of 10 testers on exporting iOS messages, to close the iOS-messages question | — |
| **Gate 1: performance budgets green** (cold start < 1.5 s; query to first result < 300 ms; first token < 2 s; 10k items indexed in < 30 min while charging; background indexing < 3 %/day; install < 60 MB) | T5, T10 |

## Phase 2: Beta (weeks 13–20)

| Deliverable | Tenets |
|---|---|
| Background indexer: BGProcessingTask / WorkManager; charging + idle + battery > 30 %; batches of 256; stop within 5 s when the window is revoked; pause above "fair" thermal state; Low Power Mode / Battery Saver pauses all work | T4, T5 |
| Battery instrumentation (per-run CPU/GPU/NPU seconds, thermal state, cut-short flag); CI reads MetricKit / Macrobenchmark + Battery Historian and fails the build on a budget breach; 7-day soak on iPhone 13 / Pixel 6a | T5 |
| User controls: "Index only while charging" (on by default), "Pause indexing", per-source photo toggle | T3, T5 |
| Messages. Android: SMS/MMS per the open default-SMS-role decision. iOS: archive import or skip, per the alpha survey. The gap is documented in onboarding | T3, T6 |
| Contacts + calendar adapters (read-only, scoped to granted calendars) | T3 |
| Email: IMAP/OAuth connectors in core; tokens only in Keychain / Keystore | T1, T3 |
| Media (video/audio): metadata always; whisper.cpp transcripts with `start_ms`/`end_ms`; keyframe OCR every 10 s or on scene change; separate toggles, off by default, with an estimate shown before enabling; 2 h per-file cap; citations seek to the cited moment | T2, T5 |
| Embedding model swap path: background full re-embed; atomic swap once coverage reaches 100 % | T8 |
| Storage hardening: `NSFileProtectionComplete`; SQLCipher or EncryptedFile on Android; index excluded from iCloud/Google backup | T1 |
| Licensing (see Gaps below): license server, StoreKit 2 / Play Billing, App Attest / Play Integrity, encrypted embedding model unlocked by an Ed25519 entitlement token verified in core | T1 |
| **Gate 2: privacy review passed**: zero egress in the default config; "data not collected" privacy labels; telemetry is counts and timings only, and opt-in | T1, T3 |

## Phase 3: Launch (weeks 21–24)

| Deliverable | Tenets |
|---|---|
| Cloud path (if the business-model decision keeps it): per-query switch, visible indicator, provider/endpoint/retention shown before the first call, PII redaction on by default, only retrieved chunks sent | T1 |
| Export / import: a JSON archive of items + chunks (no embeddings) that the app can re-import | T3 |
| Offline grace and lockout UX: "renew to search" with one-tap restore; indexing, viewing and export stay available | T9 |
| Store listings, privacy labels, support flow (shareable trace) | T1, T9 |
| Golden set grows with every production bug | T10 |

## Deferred past v1

Voice input and read-aloud answers · cross-device sync of the index · third-party app integrations (Slack, WhatsApp exports beyond plain archives) · on-device fine-tuning or memory of past conversations · widgets, Shortcuts/App Actions, share-sheet "ask about this".

## Cross-cutting release-blocking bars

| Quality metric | Bar |
|---|---|
| Recall@10 | ≥ 0.90 |
| Recall@3 | ≥ 0.75 |
| Citation correctness | ≥ 0.95 |
| Unanswerable handled | ≥ 0.95 |
| Index freshness | < 60 s after app open |

A nightly golden-set run that drops more than 2 points on any bar fails the build.

## Gaps and tensions found while planning

These need a decision-log entry. They are not resolved here.

1. **Licensing is not scheduled.** The roadmap does not place the license server, attestation or encrypted-model work in any phase. This plan puts it in Phase 2 so it can be ready for store release, and its shape depends on the open business-model question.
2. **Android background constraints.** The platform table adds "unmetered" to WorkManager constraints, but it also says the two platforms have "same constraints on both". Decide whether unmetered applies only to model downloads, which seems to be the intent.
3. **Shells in Phase 0.** The roadmap lists "Both shells" under Phase 1, but the weekly-installable-build rule applies from week 1. This plan builds minimal search-only shells in Phase 0.
4. **Licensing vs. T9.** "No token, no search" is a deliberate hard stop. This plan treats it as T9-compliant because the app says so plainly and keeps indexing, viewing and export working.

## Owners (one per doctrine section)

| Section | Owner |
|---|---|
| Architecture / core | TBD |
| Platform: iOS | TBD |
| Platform: Android | TBD |
| Battery | TBD |
| Licensing | TBD |
| Media | TBD |
| Data & privacy | TBD |
| Models | TBD |
| Quality | TBD |
| Delivery | @Camir |
