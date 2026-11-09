package com.ustrat.seeqora

import android.content.Context
import org.json.JSONObject
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord
import java.time.Instant

/**
 * Stand-in adapter for the demo build: reads the golden-set fixtures bundled
 * as assets (`<source>.jsonl`) and emits core records, exactly as a real
 * read-only adapter will from W3 on.
 */
object DemoData {
    fun load(context: Context, source: SourceKind): List<SourceRecord> {
        val file = "${source.name.lowercase()}.jsonl"
        val text = context.assets.open(file).bufferedReader().use { it.readText() }
        return text.lineSequence()
            .filter { it.isNotBlank() }
            .map { line ->
                val o = JSONObject(line)
                val meta = o.optJSONObject("metadata")
                val metadata = buildMap {
                    meta?.keys()?.forEach { k -> put(k, meta.getString(k)) }
                }
                val id = o.getString("item_id")
                SourceRecord(
                    source = source,
                    itemId = id,
                    timestampMs = Instant.parse(o.getString("time")).toEpochMilli(),
                    text = o.getString("text"),
                    metadata = metadata,
                    uri = o.optString("uri").ifEmpty { "${o.getString("source")}://$id" },
                )
            }
            .toList()
    }
}

/** A source the demo build can switch on, with the one plain sentence the consent screen shows (T3). */
data class SourceInfo(val kind: SourceKind, val name: String, val reads: String)

val DEMO_SOURCES = listOf(
    SourceInfo(SourceKind.MESSAGES, "Messages", "Reads message text and who sent it, so you can find what people told you."),
    SourceInfo(SourceKind.NOTES, "Notes", "Reads your notes, so you can search what you wrote down."),
    SourceInfo(SourceKind.FILES, "Files", "Reads the text inside saved documents, like leases and PDFs."),
    SourceInfo(SourceKind.CALENDAR, "Calendar", "Reads event titles, times and places, so you can ask what's coming up."),
    SourceInfo(SourceKind.CONTACTS, "Contacts", "Reads names, numbers and contact notes, so results can say who someone is."),
    SourceInfo(SourceKind.PHOTOS, "Photos", "Reads text found in screenshots and photos, like receipts and tickets."),
)

/** This phone's sources, in the order the consent screen lists them. */
val PHONE_SOURCES = listOf(
    SourceInfo(SourceKind.MESSAGES, "Messages", "Reads your text messages (SMS) and who sent them. Names come from Contacts only if Contacts is on."),
    SourceInfo(SourceKind.CONTACTS, "Contacts", "Reads names, numbers, emails and notes, so results can say who someone is."),
    SourceInfo(SourceKind.CALENDAR, "Calendar", "Reads events from the past year and the next six months: titles, times and places."),
    SourceInfo(SourceKind.PHOTOS, "Photos", "Finds text in your ${com.ustrat.seeqora.adapters.PhotosAdapter.MAX_PHOTOS} newest photos and screenshots, like receipts and tickets. The text reading runs on this phone."),
    SourceInfo(SourceKind.FILES, "Files & notes", "Reads text and Markdown files (.txt, .md) in one folder you choose."),
)

fun sourceName(kind: SourceKind): String =
    PHONE_SOURCES.firstOrNull { it.kind == kind }?.name
        ?: DEMO_SOURCES.firstOrNull { it.kind == kind }?.name
        ?: kind.name.lowercase().replace('_', ' ').replaceFirstChar { it.uppercase() }
