# Seeqora Android shell

Jetpack Compose app plus read-only platform adapters. **Owns platform data access and UI only.** Ingestion, chunking, embedding, storage, retrieval and prompt assembly all live in [`../core`](../core) (T6).

- **Minimum target:** Android 10 (API 29). On-device LLM only on devices with ≥ 6 GB RAM (the floor is still an open decision). Below that, answers are ranked snippets with citations.
- **Core binding:** Rust built as an `.aar` (JNI), with UniFFI-generated Kotlin bindings.
- **Adapters:** each is under 300 lines, has no business logic, and emits the core `Record` schema. Each adapter has a contract test that must match the iOS adapter's output on shared fixtures.

| Source | API | Phase |
|---|---|---|
| Files / notes | Storage Access Framework, Keep via export (no full-disk scanning) | 0 |
| Photos / OCR | MediaStore + ML Kit Text Recognition | 1 |
| Contacts / calendar | ContactsContract, CalendarContract | 2 |
| Messages | SMS/MMS via default-SMS role or Telephony provider with consent (open decision; Play policy review) | 2 |
| Email | IMAP/OAuth in core; tokens in Android Keystore only | 2 |
| Media | MediaCodec / MediaExtractor decode; whisper.cpp in core | 2 |

- **Background work:** WorkManager (charging + idle + battery > 30 %); change detection through ContentObserver, never polling.
- **Storage:** SQLCipher or Jetpack Security EncryptedFile; index excluded from Google backup.
- **Privacy:** biometric gate (default on after first index); `FLAG_SECURE` on answer screens.
- **Generator:** Gemini Nano through AICore when available, llama.cpp (Vulkan/OpenCL) otherwise, behind the core `Generator` interface.

**Build:** `bash scripts/build-android.sh` (needs the NDK and `ANDROID_NDK_HOME`), then `gradle -p android assembleDebug`. CI uploads a debug APK on every run. A Gradle wrapper will be added once a JDK is available locally.

**Demo build (Phase 0):** two screens on top of the core `Engine`.

- **Sources:** one switch per source, all off by default, each with one sentence saying what is read. Switching on grants consent and indexes that source. Switching off revokes it and shows how many items were removed. There is also a "Delete everything" button.
- **Search:** search as you type over switched-on sources. Each result shows its source, who or what it came from, and the date. Tapping a result shows the full passage, its original URI and why it matched. Keyword-only mode is announced on screen until the embedding model lands (W4).

Data comes from the golden-set fixtures in [`../eval/golden/fixtures`](../eval/golden/fixtures), bundled as assets. That is the same synthetic phone the eval measures. Real adapters replace it from W3 on.

**Try it in an emulator:** download the `app-debug` artifact from the latest `ci` run, then run `adb install app-debug.apk`.
