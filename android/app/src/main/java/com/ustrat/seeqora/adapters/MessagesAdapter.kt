package com.ustrat.seeqora.adapters

import android.content.Context
import android.net.Uri
import android.provider.ContactsContract
import android.provider.Telephony
import uniffi.seeqora_core.SourceKind
import uniffi.seeqora_core.SourceRecord

/**
 * Read-only SMS adapter (Telephony provider). One record per message.
 * Contact names are looked up only when the user has switched Contacts on in
 * Seeqora, not merely because Android granted the permission (T3).
 */
object MessagesAdapter {
    /** Newest first. Indexing is cheap; embedding runs ~12 passages/s on a flagship (W4 measurement). */
    const val MAX_MESSAGES = 50_000

    fun read(context: Context, useContactNames: Boolean): List<SourceRecord> {
        val names = ContactNames(context, useContactNames)
        val out = ArrayList<SourceRecord>()
        val c = context.contentResolver.query(
            Telephony.Sms.CONTENT_URI,
            arrayOf(
                Telephony.Sms._ID,
                Telephony.Sms.THREAD_ID,
                Telephony.Sms.ADDRESS,
                Telephony.Sms.BODY,
                Telephony.Sms.DATE,
                Telephony.Sms.TYPE,
            ),
            null,
            null,
            "${Telephony.Sms.DATE} DESC",
        ) ?: return out
        try {
            while (c.moveToNext() && out.size < MAX_MESSAGES) {
                val body = c.getString(3)
                if (body.isNullOrBlank()) continue
                val address = c.getString(2) ?: ""
                val sent = c.getInt(5) == Telephony.Sms.MESSAGE_TYPE_SENT
                val who = names.lookup(address) ?: address
                val meta = HashMap<String, String>()
                meta["address"] = address
                meta["thread_id"] = c.getLong(1).toString()
                if (sent) {
                    meta["sender"] = "Me"
                    meta["to"] = who
                } else {
                    meta["sender"] = who
                }
                out += SourceRecord(
                    source = SourceKind.MESSAGES,
                    itemId = c.getLong(0).toString(),
                    timestampMs = c.getLong(4),
                    text = body,
                    metadata = meta,
                    uri = "sms:$address",
                )
            }
        } finally {
            c.close()
        }
        return out
    }
}

/** Phone number to contact name, cached. Disabled unless Contacts is switched on. */
class ContactNames(private val context: Context, private val enabled: Boolean) {
    private val cache = HashMap<String, String>()
    private val misses = HashSet<String>()

    fun lookup(address: String): String? {
        if (!enabled || address.isBlank() || address in misses) return null
        cache[address]?.let { return it }
        val uri = Uri.withAppendedPath(ContactsContract.PhoneLookup.CONTENT_FILTER_URI, Uri.encode(address))
        val name = runCatching {
            context.contentResolver.query(uri, arrayOf(ContactsContract.PhoneLookup.DISPLAY_NAME), null, null, null)
                ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
        }.getOrNull()
        if (name == null) misses += address else cache[address] = name
        return name
    }
}
