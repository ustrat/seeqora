package com.ustrat.seeqora.adapters

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord

/**
 * Read-only files and notes adapter over one folder the user picked through
 * the system picker (Storage Access Framework). No full-disk scanning.
 * Reads plain text and Markdown; PDF text extraction comes later.
 */
object FilesAdapter {
    val TEXT_EXTENSIONS = setOf("txt", "text", "md", "markdown")
    private const val MAX_BYTES = 1_000_000L
    const val MAX_FILES = 5_000

    fun read(context: Context, tree: Uri): List<SourceRecord> {
        val out = ArrayList<SourceRecord>()
        val pending = ArrayDeque<String>()
        pending.add(DocumentsContract.getTreeDocumentId(tree))
        while (pending.isNotEmpty() && out.size < MAX_FILES) {
            val dir = pending.removeLast()
            val c = context.contentResolver.query(
                DocumentsContract.buildChildDocumentsUriUsingTree(tree, dir),
                arrayOf(
                    Document.COLUMN_DOCUMENT_ID,
                    Document.COLUMN_DISPLAY_NAME,
                    Document.COLUMN_MIME_TYPE,
                    Document.COLUMN_LAST_MODIFIED,
                    Document.COLUMN_SIZE,
                ),
                null,
                null,
                null,
            ) ?: continue
            try {
                while (c.moveToNext() && out.size < MAX_FILES) {
                    val id = c.getString(0) ?: continue
                    val name = c.getString(1) ?: continue
                    if (c.getString(2) == Document.MIME_TYPE_DIR) {
                        pending.add(id)
                        continue
                    }
                    if (name.substringAfterLast('.', "").lowercase() !in TEXT_EXTENSIONS) continue
                    if (!c.isNull(4) && c.getLong(4) > MAX_BYTES) continue
                    val doc = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    val text = runCatching {
                        context.contentResolver.openInputStream(doc)?.bufferedReader()?.use { it.readText() }
                    }.getOrNull()
                    if (text.isNullOrBlank()) continue
                    out += SourceRecord(
                        source = SourceKind.FILES,
                        itemId = id,
                        timestampMs = if (c.isNull(3)) 0L else c.getLong(3),
                        text = text,
                        metadata = mapOf("filename" to name),
                        uri = doc.toString(),
                    )
                }
            } finally {
                c.close()
            }
        }
        return out
    }
}
