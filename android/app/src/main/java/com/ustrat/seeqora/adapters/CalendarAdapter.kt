package com.ustrat.seeqora.adapters

import android.content.ContentUris
import android.content.Context
import android.provider.CalendarContract
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/**
 * Read-only calendar adapter. Reads event instances from a year back to six
 * months ahead, so recurring events appear once per occurrence.
 */
object CalendarAdapter {
    private const val DAY = 86_400_000L
    const val MAX_EVENTS = 5_000
    private val WHEN = DateTimeFormatter.ofPattern("EEEE MMMM d yyyy, h:mm a")
    private val ALL_DAY = DateTimeFormatter.ofPattern("EEEE MMMM d yyyy")

    fun read(context: Context, now: Long = System.currentTimeMillis()): List<SourceRecord> {
        val builder = CalendarContract.Instances.CONTENT_URI.buildUpon()
        ContentUris.appendId(builder, now - 365 * DAY)
        ContentUris.appendId(builder, now + 180 * DAY)
        val c = context.contentResolver.query(
            builder.build(),
            arrayOf(
                CalendarContract.Instances.EVENT_ID,
                CalendarContract.Instances.BEGIN,
                CalendarContract.Instances.TITLE,
                CalendarContract.Instances.EVENT_LOCATION,
                CalendarContract.Instances.DESCRIPTION,
                CalendarContract.Instances.ALL_DAY,
                CalendarContract.Instances.CALENDAR_DISPLAY_NAME,
            ),
            null,
            null,
            "${CalendarContract.Instances.BEGIN} DESC",
        ) ?: return emptyList()
        val out = ArrayList<SourceRecord>()
        val zone = ZoneId.systemDefault()
        try {
            while (c.moveToNext() && out.size < MAX_EVENTS) {
                val eventId = c.getLong(0)
                val begin = c.getLong(1)
                val title = c.getString(2)?.trim().orEmpty().ifEmpty { "Untitled event" }
                val location = c.getString(3)?.trim().orEmpty()
                val description = c.getString(4)?.trim().orEmpty()
                val allDay = c.getInt(5) == 1
                val whenText = if (allDay) {
                    ALL_DAY.format(Instant.ofEpochMilli(begin).atZone(ZoneOffset.UTC))
                } else {
                    WHEN.format(Instant.ofEpochMilli(begin).atZone(zone))
                }
                val text = buildList {
                    add(title)
                    if (location.isNotEmpty()) add("at $location")
                    add(whenText)
                    if (description.isNotEmpty()) add(description)
                }.joinToString("\n")
                val meta = HashMap<String, String>()
                meta["title"] = title
                if (location.isNotEmpty()) meta["location"] = location
                c.getString(6)?.let { meta["calendar"] = it }
                out += SourceRecord(
                    source = SourceKind.CALENDAR,
                    itemId = "$eventId@$begin",
                    timestampMs = begin,
                    text = text,
                    metadata = meta,
                    uri = ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, eventId).toString(),
                )
            }
        } finally {
            c.close()
        }
        return out
    }
}
