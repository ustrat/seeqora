package com.ustrat.seeqora

import android.Manifest
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.ustrat.seeqora.adapters.CalendarAdapter
import com.ustrat.seeqora.adapters.ContactsAdapter
import com.ustrat.seeqora.adapters.FilesAdapter
import com.ustrat.seeqora.adapters.MessagesAdapter
import com.ustrat.seeqora.adapters.PhotosAdapter
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.seeqora_core.Answer
import uniffi.seeqora_core.AnswerListener
import uniffi.seeqora_core.Engine
import uniffi.seeqora_core.SearchHit
import uniffi.seeqora_core.SearchResults
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord
import uniffi.seeqora_core.SourceStatus
import java.io.File
import java.util.TimeZone

fun gb(bytes: Long): String = "%.1f GB".format(bytes / 1_000_000_000.0)

/** The shipping embedding model, bundled under assets/models/ (docs/DECISION_LOG.md). */
const val MODEL_NAME = "bge-small-en-v1.5"
private const val EMBED_BATCH = 32u

/** Answers: the Qwen2.5 tokenizer ships under assets/models/; the model file is imported by the user (T1). */
private const val ANSWER_TOKENIZER = "qwen2.5"
const val ANSWER_MODEL_NAME = "Qwen2.5 1.5B Instruct (Q4_K_M, 1.1 GB)"
const val ANSWER_MODEL_URL =
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf"
/** Below this, answers are off and search shows ranked passages (DECISION_LOG: RAM floor is still open). */
private const val ANSWER_RAM_FLOOR = 5_500_000_000L
/** Free the model's ~1.5 GB after 5 idle minutes (T5). */
private const val IDLE_UNLOAD_MS = 5 * 60_000L

/** One index per process; the core serializes access internally. */
object CoreHolder {
    @Volatile private var engine: Engine? = null

    fun get(context: Context): Engine =
        engine ?: synchronized(this) {
            engine ?: Engine.open(File(context.filesDir, "seeqora.db").path).also { engine = it }
        }
}

/** Android runtime permissions each phone source needs. Files uses the folder picker instead. */
fun permissionsFor(kind: SourceKind): List<String> = when (kind) {
    SourceKind.MESSAGES -> listOf(Manifest.permission.READ_SMS)
    SourceKind.CONTACTS -> listOf(Manifest.permission.READ_CONTACTS)
    SourceKind.CALENDAR -> listOf(Manifest.permission.READ_CALENDAR)
    SourceKind.PHOTOS -> when {
        Build.VERSION.SDK_INT >= 34 -> listOf(
            Manifest.permission.READ_MEDIA_IMAGES,
            Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
        )
        Build.VERSION.SDK_INT >= 33 -> listOf(Manifest.permission.READ_MEDIA_IMAGES)
        else -> listOf(Manifest.permission.READ_EXTERNAL_STORAGE)
    }
    else -> emptyList()
}

/** UI state over the core. All core and adapter calls run off the main thread. */
class AppState(private val context: Context, private val engine: Engine) {
    private val prefs = context.getSharedPreferences("seeqora", Context.MODE_PRIVATE)
    private val syncLock = Mutex()

    var sources by mutableStateOf(listOf<SourceStatus>())
        private set
    var loaded by mutableStateOf(false)
        private set
    var busy by mutableStateOf(setOf<SourceKind>())
        private set
    /** Per-source progress line, e.g. "Reading text in photos 12 / 340". */
    val progress = mutableStateMapOf<SourceKind, String>()
    /** Bumped whenever the index changes, so open searches re-run. */
    var version by mutableIntStateOf(0)
        private set
    /** Sample-data mode: the bundled synthetic phone instead of this phone's data. */
    var sampleMode by mutableStateOf(prefs.getBoolean("sample_mode", false))
        private set
    var filesFolder by mutableStateOf(prefs.getString("files_tree", null)?.let(Uri::parse))
        private set

    /** Loaded embedding model id, or null while search is keyword-only. */
    var modelId by mutableStateOf<String?>(null)
        private set
    /** "Understanding meaning 120 / 900" while the backlog is embedded; null when idle. */
    var embedStatus by mutableStateOf<String?>(null)
        private set
    private val background = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var embedJob: Job? = null

    private val llmDir get() = File(context.filesDir, "models/llm")
    private val llmFile get() = File(llmDir, "model.gguf")
    private val answerLock = Mutex()
    private var unloadJob: Job? = null

    /** File name of the imported answer model, or null if none is installed. */
    var answerModel by mutableStateOf(prefs.getString("answer_model", null)?.takeIf { llmFile.exists() })
        private set
    /** "Copying 40%" while a model file is imported; null when idle. */
    var importStatus by mutableStateOf<String?>(null)
        private set
    /** True while a question is being answered; embedding pauses so the two don't share the CPU. */
    @Volatile var answering = false
        private set

    val totalRamBytes: Long = run {
        val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        ActivityManager.MemoryInfo().also { am.getMemoryInfo(it) }.totalMem
    }
    val answersSupported: Boolean get() = totalRamBytes >= ANSWER_RAM_FLOOR
    val answersReady: Boolean get() = answersSupported && answerModel != null

    val sourceList: List<SourceInfo> get() = if (sampleMode) DEMO_SOURCES else PHONE_SOURCES
    val anyGranted: Boolean get() = sources.any { it.granted }

    fun isGranted(kind: SourceKind) = sources.any { it.source == kind && it.granted }

    fun hasAndroidPermission(kind: SourceKind): Boolean {
        val perms = permissionsFor(kind)
        return perms.isEmpty() || perms.any {
            context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED
        }
    }

    /**
     * Builds before real data shipped only held sample data, and never wrote
     * "sample_mode". Clear that index once so sample items never mix with the
     * user's own.
     */
    suspend fun migrate() {
        // Usage events ML Kit stored before its upload backend was removed (manifest).
        withContext(Dispatchers.IO) { context.deleteDatabase("com.google.android.datatransport.events") }
        if (prefs.contains("sample_mode")) return
        withContext(Dispatchers.IO) { engine.deleteEverything() }
        prefs.edit().putBoolean("sample_mode", false).apply()
    }

    /** Copy assets/models/<name>/ to app storage once per install or update. */
    private fun copyAssetDir(name: String): File {
        val dir = File(context.filesDir, "models/$name")
        val stamp = context.packageManager.getPackageInfo(context.packageName, 0).lastUpdateTime.toString()
        val marker = File(dir, ".copied")
        if (!marker.exists() || marker.readText() != stamp) {
            dir.mkdirs()
            val files = context.assets.list("models/$name").orEmpty()
            if (files.isEmpty()) error("this build is missing models/$name")
            for (f in files) {
                context.assets.open("models/$name/$f").use { input ->
                    File(dir, f).outputStream().use { input.copyTo(it, 1 shl 20) }
                }
            }
            marker.writeText(stamp)
        }
        return dir
    }

    /**
     * Copy the bundled embedding model out of the APK (once per app install or
     * update) and load it. On failure search stays keyword-only and says so (T9).
     */
    suspend fun loadModel(): String? = withContext(Dispatchers.IO) {
        try {
            val dir = copyAssetDir(MODEL_NAME)
            modelId = engine.loadEmbedder(dir.path)
            embedInBackground()
            null
        } catch (e: Exception) {
            "Meaning search is unavailable: ${e.message}"
        }
    }

    private fun tokenizerPath(): String = File(copyAssetDir(ANSWER_TOKENIZER), "tokenizer.json").path

    /**
     * Copy a user-chosen GGUF file into app storage, check that the core can
     * load it, and keep it. Returns a message to show.
     */
    suspend fun importAnswerModel(uri: Uri): String = withContext(Dispatchers.IO) {
        val tmp = File(llmDir, "import.tmp")
        try {
            llmDir.mkdirs()
            var name = "model.gguf"
            var size = -1L
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    name = c.getString(0) ?: name
                    if (!c.isNull(1)) size = c.getLong(1)
                }
            }
            if (!name.endsWith(".gguf", ignoreCase = true)) error("$name is not a .gguf model file")
            if (size > 0 && llmDir.usableSpace < size + 300_000_000L) error("not enough free space (needs ${gb(size)})")
            answerLock.withLock {
                engine.unloadGenerator()
                importStatus = "Copying $name…"
                val input = context.contentResolver.openInputStream(uri) ?: error("can't open $name")
                input.use { inp ->
                    tmp.outputStream().use { out ->
                        val buf = ByteArray(1 shl 20)
                        var copied = 0L
                        while (true) {
                            val n = inp.read(buf)
                            if (n < 0) break
                            out.write(buf, 0, n)
                            copied += n
                            if (size > 0) importStatus = "Copying $name: ${copied * 100 / size}%"
                        }
                    }
                }
                val magic = ByteArray(4)
                tmp.inputStream().use { it.read(magic) }
                if (String(magic, Charsets.US_ASCII) != "GGUF") error("$name is not a GGUF file")
                importStatus = "Checking the model…"
                engine.loadGenerator(tmp.path, tokenizerPath())
                llmFile.delete()
                if (!tmp.renameTo(llmFile)) error("couldn't save the model")
                prefs.edit().putString("answer_model", name).apply()
                answerModel = name
            }
            scheduleUnload()
            "Answers are on: $name"
        } catch (e: Exception) {
            tmp.delete()
            "Couldn't import the model: ${e.message}"
        } finally {
            importStatus = null
        }
    }

    /** Import outside any screen's scope, so leaving the screen doesn't cancel a 1 GB copy. */
    fun startImport(uri: Uri, done: (String) -> Unit) {
        background.launch {
            val msg = importAnswerModel(uri)
            withContext(Dispatchers.Main) { done(msg) }
        }
    }

    suspend fun removeAnswerModel(): String = answerLock.withLock {
        withContext(Dispatchers.IO) {
            engine.unloadGenerator()
            llmFile.delete()
        }
        prefs.edit().remove("answer_model").apply()
        answerModel = null
        "Answer model removed."
    }

    /**
     * Answer a question with citations. Loads the model on first use and frees
     * it after 5 idle minutes (T5). [onDelta] gets the answer as it is written;
     * cancelling the calling coroutine stops generation at the next token.
     */
    suspend fun ask(question: String, onStatus: (String) -> Unit, onDelta: (String) -> Unit): Answer =
        answerLock.withLock {
            withContext(Dispatchers.IO) {
                unloadJob?.cancel()
                answering = true
                try {
                    if (!engine.generatorLoaded()) {
                        onStatus("Loading the answer model…")
                        engine.loadGenerator(llmFile.path, tokenizerPath())
                    }
                    onStatus("Reading your data…")
                    val job = currentCoroutineContext()[Job]
                    val offsetMin = TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000
                    engine.answer(question, offsetMin, object : AnswerListener {
                        override fun onText(text: String): Boolean {
                            if (job?.isActive != true) return false
                            onDelta(text)
                            return true
                        }
                    })
                } finally {
                    answering = false
                    scheduleUnload()
                }
            }
        }

    /**
     * Load and warm the answer model while the user is still typing, so the
     * first question doesn't pay the cold start (~17 s on the S24 Ultra).
     * Embedding pauses meanwhile; the idle unload still applies (T5).
     */
    fun prepareAnswers() {
        if (!answersReady || engine.generatorLoaded() || answerLock.isLocked) return
        background.launch {
            answerLock.withLock {
                if (engine.generatorLoaded()) return@withLock
                unloadJob?.cancel()
                answering = true
                try {
                    runCatching { engine.loadGenerator(llmFile.path, tokenizerPath()) }
                        .onFailure { Log.w("Seeqora", "answer model preload failed: ${it.message}") }
                } finally {
                    answering = false
                    scheduleUnload()
                }
            }
        }
    }

    private fun scheduleUnload() {
        unloadJob?.cancel()
        unloadJob = background.launch {
            delay(IDLE_UNLOAD_MS)
            answerLock.withLock { engine.unloadGenerator() }
        }
    }

    /** Peak resident memory of this process, for the answer timing line. */
    fun peakMemoryMb(): Long? = runCatching {
        File("/proc/self/status").readLines()
            .first { it.startsWith("VmHWM:") }
            .split(Regex("\\s+"))[1].toLong() / 1024
    }.getOrNull()

    /** Embed every chunk that has no vector yet, a batch at a time, off the main thread. */
    fun embedInBackground() {
        if (modelId == null || embedJob?.isActive == true) return
        embedJob = background.launch {
            try {
                var total = engine.embeddingBacklog()
                var done = 0L
                while (total > 0) {
                    if (answering) {
                        Log.i("Seeqora", "embedding paused while answering")
                        while (answering) delay(250) // the answer gets the CPU
                        Log.i("Seeqora", "embedding resumed")
                    }
                    embedStatus = "Understanding meaning: $done / $total"
                    val n = engine.embedNext(EMBED_BATCH)
                    if (n == 0L) break
                    done += n
                    if (done > total) total = done
                }
            } catch (e: Exception) {
                embedStatus = null
                return@launch
            }
            embedStatus = null
            withContext(Dispatchers.Main) { version++ }
        }
    }

    suspend fun refresh() {
        sources = withContext(Dispatchers.IO) { engine.sources() }
        loaded = true
        version++
    }

    fun setFolder(uri: Uri) {
        context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        prefs.edit().putString("files_tree", uri.toString()).apply()
        filesFolder = uri
    }

    /** Switch a source on (grant, then index) or off (revoke). Returns a message to show. */
    suspend fun setGranted(kind: SourceKind, on: Boolean): String {
        val name = sourceName(kind)
        return try {
            if (on) {
                withContext(Dispatchers.IO) { engine.grant(kind) }
                val n = sync(kind)
                "$name on. $n items indexed on this phone."
            } else {
                val r = withContext(Dispatchers.IO) { engine.revoke(kind) }
                "$name off. Removed ${r.items} items and ${r.chunks} passages from the index."
            }
        } catch (e: Exception) {
            "Couldn't update $name: ${e.message}"
        } finally {
            refresh()
        }
    }

    /** Re-read every switched-on source; unchanged items are skipped by the core. */
    suspend fun syncAll(includePhotos: Boolean): String? {
        var failed: String? = null
        for (s in sources.filter { it.granted }) {
            if (s.source == SourceKind.PHOTOS && !includePhotos) continue
            try {
                sync(s.source)
            } catch (e: Exception) {
                failed = "Couldn't refresh ${sourceName(s.source)}: ${e.message}"
            }
        }
        refresh()
        return failed
    }

    /** Read one source through its adapter and index it. Returns the item count. */
    private suspend fun sync(kind: SourceKind): Int = syncLock.withLock {
        busy = busy + kind
        try {
            withContext(Dispatchers.IO) {
                if (sampleMode) {
                    val records = DemoData.load(context, kind)
                    indexInBatches(kind, records)
                    return@withContext records.size
                }
                if (!hasAndroidPermission(kind)) {
                    error("Android permission for ${sourceName(kind)} is off. Allow it in Settings > Apps > Seeqora.")
                }
                when (kind) {
                    SourceKind.PHOTOS -> {
                        val known = engine.indexedIds(kind).toSet()
                        val r = PhotosAdapter.read(context, known) { done, total ->
                            if (total > 0) progress[kind] = "Reading text in photos $done / $total"
                        }
                        indexInBatches(kind, r.records)
                        engine.retainOnly(kind, r.allIds)
                        engine.indexedIds(kind).size
                    }
                    else -> {
                        progress[kind] = "Reading ${sourceName(kind).lowercase()}…"
                        val records = read(kind)
                        indexInBatches(kind, records)
                        engine.retainOnly(kind, records.map { it.itemId })
                        records.size
                    }
                }
            }
        } finally {
            progress.remove(kind)
            busy = busy - kind
            embedInBackground()
        }
    }

    private fun read(kind: SourceKind): List<SourceRecord> = when (kind) {
        SourceKind.MESSAGES -> MessagesAdapter.read(context, useContactNames = isGranted(SourceKind.CONTACTS))
        SourceKind.CONTACTS -> ContactsAdapter.read(context)
        SourceKind.CALENDAR -> CalendarAdapter.read(context)
        SourceKind.FILES -> filesFolder?.let { FilesAdapter.read(context, it) }
            ?: error("Choose a folder for Files & notes first.")
        else -> emptyList()
    }

    private fun indexInBatches(kind: SourceKind, records: List<SourceRecord>) {
        val batches = records.chunked(250)
        batches.forEachIndexed { i, batch ->
            if (batches.size > 1) progress[kind] = "Indexing ${i * 250} / ${records.size}"
            engine.index(batch)
        }
    }

    suspend fun deleteEverything(): String =
        try {
            withContext(Dispatchers.IO) { engine.deleteEverything() }
            "Everything deleted. All sources are off."
        } catch (e: Exception) {
            "Couldn't delete: ${e.message}"
        } finally {
            refresh()
        }

    /** Switching between sample data and this phone's data starts from an empty index. */
    suspend fun setSampleMode(on: Boolean): String {
        val msg = deleteEverything()
        sampleMode = on
        prefs.edit().putBoolean("sample_mode", on).apply()
        return if (msg.startsWith("Couldn't")) msg
        else if (on) "Sample data mode. Switch sources on to load the sample phone."
        else "Using this phone's data. Switch sources on to index them."
    }

    suspend fun search(query: String): SearchResults =
        withContext(Dispatchers.IO) { engine.search(query, 20u) }

    /** Tap-through to the original item in its own app (T2). False if nothing can open it. */
    fun openOriginal(hit: SearchHit): Boolean {
        val uri = Uri.parse(hit.uri)
        if (uri.scheme != "sms" && uri.scheme != "content") return false
        val intent = Intent(Intent.ACTION_VIEW).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (hit.source == SourceKind.FILES) {
            val name = hit.metadata["filename"].orEmpty().lowercase()
            intent.setDataAndType(uri, if (name.endsWith(".md") || name.endsWith(".markdown")) "text/markdown" else "text/plain")
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } else {
            intent.data = uri
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        return runCatching { context.startActivity(intent) }.isSuccess
    }

    fun canOpen(hit: SearchHit): Boolean = Uri.parse(hit.uri).scheme.let { it == "sms" || it == "content" }
}
