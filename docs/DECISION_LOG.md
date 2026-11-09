# Decision log

To change a tenet, add a row here with the new decision and the tenet it replaces, then edit the tenet. The doctrine is reviewed at every phase gate. The five open questions must be answered before Phase 0 ends.

| Date | Decision or question | Status | Rationale / owner |
|---|---|---|---|
| 2026-11-07 | Shared Rust core with native shells, no cross-platform UI framework | Decided | One test suite for the hard parts; native UI keeps platform feel and OS integrations |
| 2026-11-07 | SQLite + sqlite-vec + FTS5 as the only datastore | Decided | Single file, encrypted by the OS, no sync layer to secure |
| 2026-11-07 | On-device generation by default, cloud per query only | Decided | Privacy is the product; cloud is a convenience, not a dependency |
| 2026-11-07 | Retrieval quality gates every phase | Decided | A chat UI over bad retrieval produces confident wrong answers |
| 2026-11-07 | Which embedding model ships first (quality vs. size vs. license) | Decided 2026-11-09 | **bge-small-en-v1.5 (MIT)**, run in the core via candle, shipped at f16. Golden set v0, hybrid: R@10 0.867 / R@3 0.778 (oracle filters 0.956 / 0.911), vs 0.844 / 0.778 (0.956 / 0.889) for all-MiniLM-L6-v2 and snowflake-arctic-embed-xs. About 2x their compute and +22 MB; the margin is 1–2 questions of 45, so re-run when the set reaches 200 |
| 2026-11-09 | Android sideload builds ahead of Gate 0 (real-data adapters, embeddings) | Decided | Owner asked for an installable full-function build; each stage still lands with a golden-set run |
| 2026-11-09 | Answer generator for the sideload build: candle (pure Rust, quantized GGUF) instead of llama.cpp; first model family Qwen2.5 Instruct | Decided (provisional) | BUILD_PLAN names llama.cpp as the fallback generator. candle is already in the core for embeddings, so it builds for iOS and Android with no C++ toolchain, and it sits behind the `Generator` trait (T8). Model: Qwen2.5-1.5B-Instruct Q4_K_M (Apache-2.0, 1.1 GB), imported by the user from a browser download; only its tokenizer ships in the app (T1). **Exit criterion:** if on the S24 Ultra the ~1,200-token prompt takes over 15 s to read, or writing runs under 5 tokens/s, switch to llama.cpp behind the same trait or a smaller model. The RAM-floor row below takes its numbers from the same measurement. **Measured 2026-11-09, S24 Ultra, first build:** 570-token prompt read in 11.1 s (51 tok/s, so ~23 s at 1,200), 2.8 tok/s writing, 2.5 GB peak. Misses on both counts. That build lacked `+dotprod` (candle falls back to emulated SDOT) and spread work across all 8 cores, little cores included. **One** retest with both fixed (sideload-only: `+dotprod` excludes ARMv8.0 phones, which the app now refuses at startup); if it still misses, switch to llama.cpp the same day. No further tuning rounds |
| 2026-11-07 | iOS messages: archive import only, or skip in v1? | Open | Depends on how many testers will export; decide after alpha survey |
| 2026-11-07 | Minimum RAM for on-device LLM: 6 GB or 8 GB? | Open | Measure tokens/s and thermal throttling on 6 GB devices in Phase 1 |
| 2026-11-07 | Business model: one-time purchase, subscription, or free + paid cloud? | Open | Affects whether a cloud path is built in Phase 3 at all |
| 2026-11-07 | Android default-SMS role for messages, or content-provider read with consent? | Open | Play policy review needed; default-SMS role is heavy for users |
| 2026-11-07 | Which phase owns licensing (license server, attestation, encrypted model)? | Open | Not placed in the roadmap; build plan proposes Phase 2 |
| 2026-11-07 | Does "unmetered" apply to all Android background work, or only model downloads? | Open | Platform table vs. "same constraints on both" |
| 2026-11-07 | Phase 0 chunk sizes use a whitespace-token estimate until the embedding tokenizer lands | Decided (temporary) | Unblocks chunker tests; replaced in plan W2 |
| 2026-11-07 | Phase 0 dense search is brute-force cosine over BLOB vectors; sqlite-vec swaps in at W4 | Decided (temporary) | Keeps the first CI build dependency-light; same results, slower |
