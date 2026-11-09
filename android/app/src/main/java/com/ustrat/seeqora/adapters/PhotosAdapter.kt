package com.ustrat.seeqora.adapters

import android.content.ContentUris
import android.content.Context
import android.graphics.ImageDecoder
import android.provider.MediaStore
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.latin.TextRecognizerOptions
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord
import java.io.File

/**
 * Read-only photos adapter: on-device OCR (ML Kit, bundled model, no network)
 * over the newest photos. Photos already indexed, or already found to have no
 * text, are never OCR'd again (T5). Call from a background thread.
 */
object PhotosAdapter {
    const val MAX_PHOTOS = 500
    private const val MIN_TEXT = 12
    private const val MAX_EDGE = 2048

    class Result(val records: List<SourceRecord>, val allIds: List<String>)

    fun read(
        context: Context,
        alreadyIndexed: Set<String>,
        onProgress: (done: Int, total: Int) -> Unit,
    ): Result {
        val noText = NoTextLog(File(context.filesDir, "photos_no_text.txt"))
        val all = ArrayList<String>()
        val todo = ArrayList<Triple<Long, Long, String>>() // id, time, name
        val c = context.contentResolver.query(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            arrayOf(
                MediaStore.Images.Media._ID,
                MediaStore.Images.Media.DATE_TAKEN,
                MediaStore.Images.Media.DATE_ADDED,
                MediaStore.Images.Media.DISPLAY_NAME,
            ),
            null,
            null,
            "${MediaStore.Images.Media.DATE_ADDED} DESC",
        ) ?: return Result(emptyList(), emptyList())
        try {
            while (c.moveToNext()) {
                val id = c.getLong(0)
                all += id.toString()
                if (all.size > MAX_PHOTOS) continue
                if (id.toString() in alreadyIndexed || noText.contains(id.toString())) continue
                val time = if (!c.isNull(1) && c.getLong(1) > 0) c.getLong(1) else c.getLong(2) * 1000
                todo += Triple(id, time, c.getString(3) ?: "photo")
            }
        } finally {
            c.close()
        }

        val out = ArrayList<SourceRecord>()
        val recognizer = TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS)
        try {
            todo.forEachIndexed { i, (id, time, name) ->
                onProgress(i, todo.size)
                val uri = ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, id)
                val text = runCatching {
                    val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(context.contentResolver, uri)) { decoder, info, _ ->
                        val edge = maxOf(info.size.width, info.size.height)
                        if (edge > MAX_EDGE) {
                            val s = MAX_EDGE.toDouble() / edge
                            decoder.setTargetSize((info.size.width * s).toInt(), (info.size.height * s).toInt())
                        }
                        decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                    }
                    try {
                        Tasks.await(recognizer.process(InputImage.fromBitmap(bitmap, 0))).text
                    } finally {
                        bitmap.recycle()
                    }
                }.getOrNull()?.trim().orEmpty()
                if (text.length < MIN_TEXT) {
                    noText.add(id.toString())
                } else {
                    out += SourceRecord(
                        source = SourceKind.PHOTOS,
                        itemId = id.toString(),
                        timestampMs = time,
                        text = text,
                        metadata = mapOf("kind" to "photo", "filename" to name),
                        uri = uri.toString(),
                    )
                }
            }
            onProgress(todo.size, todo.size)
        } finally {
            recognizer.close()
            noText.save()
        }
        return Result(out, all)
    }

    /** Photo ids already OCR'd and found to hold no text. */
    private class NoTextLog(private val file: File) {
        private val ids: MutableSet<String> =
            runCatching { file.readLines().toMutableSet() }.getOrDefault(mutableSetOf())
        private var dirty = false

        fun contains(id: String) = id in ids
        fun add(id: String) {
            dirty = dirty or ids.add(id)
        }
        fun save() {
            if (dirty) runCatching { file.writeText(ids.joinToString("\n")) }
        }
    }
}
