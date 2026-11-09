package com.ustrat.seeqora

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.seeqora_core.Engine
import uniffi.seeqora_core.SearchHit
import uniffi.seeqora_core.SearchResults
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord
import uniffi.seeqora_core.SourceStatus
import java.io.File

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

    val sourceList: List<SourceInfo> get() = if (sampleMode) DEMO_SOURCES else PHONE_SOURCES
    val anyGranted: Boolean get() = sources.any { it.granted }

    fun isGranted(kind: SourceKind) = sources.any { it.source == kind && it.granted }

    fun hasAndroidPermission(kind: SourceKind): Boolean {
        val perms = permissionsFor(kind)
        return perms.isEmpty() || perms.any {
            context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED
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
