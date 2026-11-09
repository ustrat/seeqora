package com.ustrat.seeqora.adapters

import android.content.Context
import android.provider.ContactsContract
import android.provider.ContactsContract.CommonDataKinds.Email
import android.provider.ContactsContract.CommonDataKinds.Note
import android.provider.ContactsContract.CommonDataKinds.Organization
import android.provider.ContactsContract.CommonDataKinds.Phone
import android.provider.ContactsContract.CommonDataKinds.StructuredName
import android.provider.ContactsContract.CommonDataKinds.StructuredPostal
import android.provider.ContactsContract.Data
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord

/** Read-only contacts adapter (ContactsContract). One record per contact. */
object ContactsAdapter {
    private class Acc {
        var name = ""
        var lookupKey = ""
        var updated = 0L
        val lines = ArrayList<String>()
    }

    fun read(context: Context): List<SourceRecord> {
        val byContact = LinkedHashMap<Long, Acc>()
        val mimes = arrayOf(
            StructuredName.CONTENT_ITEM_TYPE,
            Phone.CONTENT_ITEM_TYPE,
            Email.CONTENT_ITEM_TYPE,
            Note.CONTENT_ITEM_TYPE,
            Organization.CONTENT_ITEM_TYPE,
            StructuredPostal.CONTENT_ITEM_TYPE,
        )
        val c = context.contentResolver.query(
            Data.CONTENT_URI,
            arrayOf(
                Data.CONTACT_ID,
                Data.MIMETYPE,
                Data.DATA1,
                Data.DATA2,
                Data.DATA3,
                Data.DATA4,
                Data.LOOKUP_KEY,
                Data.CONTACT_LAST_UPDATED_TIMESTAMP,
            ),
            "${Data.MIMETYPE} IN (${mimes.joinToString(",") { "?" }})",
            mimes,
            null,
        ) ?: return emptyList()
        val res = context.resources
        try {
            while (c.moveToNext()) {
                val a = byContact.getOrPut(c.getLong(0)) { Acc() }
                a.lookupKey = c.getString(6) ?: a.lookupKey
                a.updated = maxOf(a.updated, if (c.isNull(7)) 0L else c.getLong(7))
                val v = c.getString(2)?.trim().orEmpty()
                if (v.isEmpty()) continue
                when (c.getString(1)) {
                    StructuredName.CONTENT_ITEM_TYPE -> a.name = v
                    Phone.CONTENT_ITEM_TYPE -> {
                        val label = Phone.getTypeLabel(res, c.getInt(3), c.getString(4)).toString().lowercase()
                        a.lines += "$label $v"
                    }
                    Email.CONTENT_ITEM_TYPE -> a.lines += "email $v"
                    Note.CONTENT_ITEM_TYPE -> a.lines += "note: $v"
                    Organization.CONTENT_ITEM_TYPE -> {
                        val title = c.getString(5)?.trim().orEmpty()
                        a.lines += if (title.isEmpty()) "works at $v" else "$title at $v"
                    }
                    StructuredPostal.CONTENT_ITEM_TYPE -> a.lines += "address ${v.replace('\n', ' ')}"
                }
            }
        } finally {
            c.close()
        }
        return byContact.mapNotNull { (id, a) ->
            if (a.name.isEmpty() && a.lines.isEmpty()) return@mapNotNull null
            val name = a.name.ifEmpty { "Unnamed contact" }
            SourceRecord(
                source = SourceKind.CONTACTS,
                itemId = id.toString(),
                timestampMs = a.updated,
                text = (listOf(name) + a.lines).joinToString("\n"),
                metadata = mapOf("name" to name),
                uri = ContactsContract.Contacts.getLookupUri(id, a.lookupKey).toString(),
            )
        }
    }
}
