# Seeqora

*Pronounced "SEEK-or-uh."*

Seeqora is a private, on-device assistant for iOS and Android. You ask questions about what's on your phone (messages, photos, notes, files, contacts, calendar, email) and it answers with citations that open the original item. Indexing, retrieval and generation all run on the phone by default.

## Repository layout

| Path | What lives there | Doctrine rule |
|---|---|---|
| [`core/`](core/) | Shared Rust core: ingest, chunk, embed, index, retrieve, prompt assembly | Tenet 6: one core, two shells |
| [`ios/`](ios/) | SwiftUI shell + read-only platform adapters | Data access and UI only |
| [`android/`](android/) | Jetpack Compose shell + read-only platform adapters | Data access and UI only |
| [`eval/`](eval/) | Golden query set and retrieval evaluation harness | Tenet 10: measure retrieval before polishing chat |
| [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md) | Phased build plan; every item cites the tenet it serves | — |
| [`docs/DECISION_LOG.md`](docs/DECISION_LOG.md) | Decided and open questions; tenet changes go here first | — |

One repo, one test suite, one release tag.

## Status

**Phase 0, week 1 (Foundation).** The core skeleton is in place: the normalized record schema, natural-unit chunking with stable ids, the SQLite schema (items, chunks, embeddings, FTS5, sources with consent), keyword and dense search, and reciprocal rank fusion. Unit tests run in CI. The platform shells are not started yet.

## Build the core

```sh
cargo test -p seeqora-core
```

Requires a stable Rust toolchain. SQLite is bundled, with FTS5 included.
