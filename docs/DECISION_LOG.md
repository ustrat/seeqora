# Decision log

To change a tenet, add a row here with the new decision and the tenet it replaces, then edit the tenet. The doctrine is reviewed at every phase gate. The five open questions must be answered before Phase 0 ends.

| Date | Decision or question | Status | Rationale / owner |
|---|---|---|---|
| 2026-11-07 | Shared Rust core with native shells, no cross-platform UI framework | Decided | One test suite for the hard parts; native UI keeps platform feel and OS integrations |
| 2026-11-07 | SQLite + sqlite-vec + FTS5 as the only datastore | Decided | Single file, encrypted by the OS, no sync layer to secure |
| 2026-11-07 | On-device generation by default, cloud per query only | Decided | Privacy is the product; cloud is a convenience, not a dependency |
| 2026-11-07 | Retrieval quality gates every phase | Decided | A chat UI over bad retrieval produces confident wrong answers |
| 2026-11-07 | Which embedding model ships first (quality vs. size vs. license) | Open | Run three candidates through the golden set in Phase 0 (plan: W4) |
| 2026-11-07 | iOS messages: archive import only, or skip in v1? | Open | Depends on how many testers will export; decide after alpha survey |
| 2026-11-07 | Minimum RAM for on-device LLM: 6 GB or 8 GB? | Open | Measure tokens/s and thermal throttling on 6 GB devices in Phase 1 |
| 2026-11-07 | Business model: one-time purchase, subscription, or free + paid cloud? | Open | Affects whether a cloud path is built in Phase 3 at all |
| 2026-11-07 | Android default-SMS role for messages, or content-provider read with consent? | Open | Play policy review needed; default-SMS role is heavy for users |
| 2026-11-07 | Which phase owns licensing (license server, attestation, encrypted model)? | Open | Not placed in the roadmap; build plan proposes Phase 2 |
| 2026-11-07 | Does "unmetered" apply to all Android background work, or only model downloads? | Open | Platform table vs. "same constraints on both" |
| 2026-11-07 | Phase 0 chunk sizes use a whitespace-token estimate until the embedding tokenizer lands | Decided (temporary) | Unblocks chunker tests; replaced in plan W2 |
| 2026-11-07 | Phase 0 dense search is brute-force cosine over BLOB vectors; sqlite-vec swaps in at W4 | Decided (temporary) | Keeps the first CI build dependency-light; same results, slower |
