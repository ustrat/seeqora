# Seeqora iOS shell

SwiftUI app plus read-only platform adapters. **Owns platform data access and UI only.** Ingestion, chunking, embedding, storage, retrieval and prompt assembly all live in [`../core`](../core) (T6).

- **Minimum target:** iOS 17. On-device LLM only on devices with ≥ 6 GB RAM (the floor is still an open decision). Below that, answers are ranked snippets with citations.
- **Core binding:** Rust built as an XCFramework, with UniFFI-generated Swift bindings.
- **Adapters:** each is under 300 lines, has no business logic, and emits the core `Record` schema. Each adapter has a contract test that must match the Android adapter's output on shared fixtures.

| Source | API | Phase |
|---|---|---|
| Files / notes | Files app (security-scoped bookmarks), Notes via export | 0 |
| Photos / OCR | PhotoKit + Vision | 1 |
| Contacts / calendar | Contacts, EventKit | 2 |
| Messages | Not accessible to third-party apps; exported archive import only (open decision) | 2 |
| Email | IMAP/OAuth in core; tokens in Keychain only | 2 |
| Media | AVFoundation decode; whisper.cpp in core | 2 |

- **Background work:** BGProcessingTask (charging + idle); foreground mini-pass on app open capped at 2 s of CPU.
- **Storage:** `NSFileProtectionComplete`; index excluded from iCloud backup.
- **Privacy:** Face ID gate (default on after first index); answer screens blurred in the app switcher.
- **Generator:** Apple Foundation Models when available, llama.cpp (Metal) otherwise, behind the core `Generator` interface.

**Build:** `bash scripts/build-ios.sh` on a Mac, then open `ios/Seeqora.xcodeproj`. The project is generated from [`project.yml`](project.yml) with XcodeGen. See [docs/MACINCLOUD.md](../docs/MACINCLOUD.md).

*Phase 0 W1: an empty shell that links the core and shows `coreVersion()`.*
