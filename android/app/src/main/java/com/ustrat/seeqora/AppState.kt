package com.ustrat.seeqora

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.seeqora_core.Engine
import uniffi.seeqora_core.SearchResults
import uniffi.seeqora_core.SourceKind
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

/** UI state over the core. All core calls run off the main thread. */
class AppState(private val context: Context, private val engine: Engine) {
    var sources by mutableStateOf(listOf<SourceStatus>())
        private set
    var loaded by mutableStateOf(false)
        private set
    var busy by mutableStateOf(setOf<SourceKind>())
        private set
    /** Bumped whenever the index changes, so open searches re-run. */
    var version by mutableIntStateOf(0)
        private set

    val anyGranted: Boolean get() = sources.any { it.granted }

    suspend fun refresh() {
        sources = withContext(Dispatchers.IO) { engine.sources() }
        loaded = true
        version++
    }

    /** Switch a source on (grant, then index) or off (revoke). Returns a message to show. */
    suspend fun setGranted(kind: SourceKind, on: Boolean): String {
        busy = busy + kind
        val name = sourceName(kind)
        return try {
            withContext(Dispatchers.IO) {
                if (on) {
                    engine.grant(kind)
                    val n = engine.index(DemoData.load(context, kind))
                    "$name on. ${n.indexed + n.unchanged} items indexed on this phone."
                } else {
                    val r = engine.revoke(kind)
                    "$name off. Removed ${r.items} items and ${r.chunks} passages from the index."
                }
            }
        } catch (e: Exception) {
            "Couldn't update $name: ${e.message}"
        } finally {
            busy = busy - kind
            refresh()
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

    suspend fun search(query: String): SearchResults =
        withContext(Dispatchers.IO) { engine.search(query, 20u) }
}
