@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package com.ustrat.seeqora.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ustrat.seeqora.AppState
import kotlinx.coroutines.delay
import uniffi.seeqora_core.SearchHit
import uniffi.seeqora_core.SearchResults
import uniffi.seeqora_core.SourceKind
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.coroutines.cancellation.CancellationException

private val SUGGESTIONS = listOf(
    "cabin wifi password",
    "oil change",
    "flight confirmation",
    "rent due",
    "parking receipt",
    "dentist",
)

@Composable
fun SearchScreen(state: AppState, onChooseSources: () -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var results by remember { mutableStateOf<SearchResults?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var open by remember { mutableStateOf<SearchHit?>(null) }
    val focus = LocalFocusManager.current

    LaunchedEffect(query, state.version) {
        if (query.isBlank()) {
            results = null
            error = null
            return@LaunchedEffect
        }
        delay(200) // search as you type, without a call per keystroke
        try {
            results = state.search(query)
            error = null
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            error = e.message ?: e.toString()
        }
    }

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(start = 20.dp, end = 20.dp, top = 24.dp)) {
            Text("Seeqora", style = MaterialTheme.typography.headlineMedium)
            Text(
                "Search what's on this phone. Nothing leaves it.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Ask about your messages, notes, files…") },
            leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
            trailingIcon = {
                if (query.isNotEmpty()) {
                    IconButton(onClick = { query = "" }) {
                        Icon(Icons.Filled.Clear, contentDescription = "Clear search")
                    }
                }
            },
            singleLine = true,
            shape = RoundedCornerShape(28.dp),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { focus.clearFocus() }),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 16.dp),
        )

        when {
            !state.loaded -> Unit
            !state.anyGranted -> NoSources(onChooseSources)
            query.isBlank() -> Suggestions(onPick = { query = it; focus.clearFocus() })
            error != null -> Notice("Search failed: $error")
            else -> results?.let { r -> Results(r, query, onOpen = { open = it }) }
        }
    }

    open?.let { hit ->
        ModalBottomSheet(onDismissRequest = { open = null }) { HitDetail(hit) }
    }
}

@Composable
private fun Results(r: SearchResults, query: String, onOpen: (SearchHit) -> Unit) {
    val stems = remember(query) { queryStems(query) }
    val mark = MaterialTheme.colorScheme.tertiaryContainer
    LazyColumn(
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        // Reduced mode is always visible, never silent (T9).
        r.degraded?.let { msg -> item { Notice(msg, Modifier) } }
        item {
            Text(
                when (r.hits.size) {
                    0 -> "Nothing found in the sources you've switched on."
                    1 -> "1 result"
                    else -> "${r.hits.size} results"
                },
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp, vertical = 4.dp),
            )
        }
        items(r.hits, key = { it.chunkId }) { hit ->
            ResultCard(hit, stems, mark, onClick = { onOpen(hit) })
        }
    }
}

@Composable
private fun ResultCard(hit: SearchHit, stems: List<String>, mark: Color, onClick: () -> Unit) {
    ElevatedCard(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                SourceBadge(hit.source)
                Spacer(Modifier.width(8.dp))
                Text(
                    citationLabel(hit),
                    style = MaterialTheme.typography.labelLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    formatDate(hit.timestampMs),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                highlight(hit.text.trim(), stems, mark),
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Tap-through to the cited passage and its original (T2). */
@Composable
private fun HitDetail(hit: SearchHit) {
    Column(
        Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(start = 24.dp, end = 24.dp, bottom = 40.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SourceBadge(hit.source)
            Spacer(Modifier.width(8.dp))
            Text(
                formatDate(hit.timestampMs, alwaysYear = true),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(
            citationLabel(hit),
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(top = 12.dp),
        )
        Text(
            hit.text.trim(),
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.padding(vertical = 16.dp),
        )
        HorizontalDivider()
        Text(
            "Original",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 16.dp),
        )
        Text(hit.uri, style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace)
        Text(
            "Demo data. On a real phone, this opens the original in its own app.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 4.dp),
        )
        Text(
            "Why this result",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 16.dp),
        )
        Text(matchExplanation(hit), style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun SourceBadge(kind: SourceKind) {
    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        shape = RoundedCornerShape(6.dp),
    ) {
        Text(
            kindLabel(kind),
            style = MaterialTheme.typography.labelSmall,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
        )
    }
}

@Composable
private fun Notice(text: String, modifier: Modifier = Modifier.padding(horizontal = 16.dp)) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(
                Icons.Filled.Info,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.width(10.dp))
            Text(text, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun NoSources(onChooseSources: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("No sources are on", style = MaterialTheme.typography.titleMedium)
        Text(
            "Seeqora only searches what you switch on.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 4.dp, bottom = 16.dp),
        )
        Button(onClick = onChooseSources) { Text("Choose sources") }
    }
}

@Composable
private fun Suggestions(onPick: (String) -> Unit) {
    Column(Modifier.padding(horizontal = 20.dp)) {
        Text(
            "Try",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            SUGGESTIONS.forEach { s ->
                SuggestionChip(onClick = { onPick(s) }, label = { Text(s) })
            }
        }
    }
}

// ---- formatting --------------------------------------------------------------

private fun kindLabel(kind: SourceKind): String = when (kind) {
    SourceKind.MESSAGES -> "Message"
    SourceKind.NOTES -> "Note"
    SourceKind.FILES -> "File"
    SourceKind.CONTACTS -> "Contact"
    SourceKind.CALENDAR -> "Event"
    SourceKind.PHOTOS -> "Photo"
    SourceKind.EMAIL -> "Email"
    SourceKind.CALL_LOG -> "Call"
    SourceKind.CLIPBOARD -> "Clipboard"
    SourceKind.MEDIA -> "Media"
}

/** Who or what the result came from: the visible half of the citation (T2). */
private fun citationLabel(hit: SearchHit): String {
    val m = hit.metadata
    return when (hit.source) {
        SourceKind.MESSAGES -> m["sender"]?.let { "From $it" } ?: "Message"
        SourceKind.NOTES -> m["title"] ?: "Note"
        SourceKind.FILES -> m["filename"] ?: "Document"
        SourceKind.CONTACTS -> m["name"] ?: "Contact"
        SourceKind.CALENDAR -> m["title"] ?: "Event"
        SourceKind.PHOTOS -> m["kind"]?.replaceFirstChar { it.uppercase() } ?: "Photo"
        else -> m["title"] ?: kindLabel(hit.source)
    }
}

private fun matchExplanation(hit: SearchHit): String {
    val parts = buildList {
        hit.keywordRank?.let { add("keyword match, rank $it") }
        hit.denseRank?.let { add("meaning match, rank $it") }
    }
    return if (parts.isEmpty()) "Ranked by fused score." else parts.joinToString("; ").replaceFirstChar { it.uppercase() } + "."
}

private val SHORT_DATE = DateTimeFormatter.ofPattern("MMM d")
private val LONG_DATE = DateTimeFormatter.ofPattern("MMM d, yyyy")

private fun formatDate(ms: Long, alwaysYear: Boolean = false): String {
    val zone = ZoneId.systemDefault()
    val d = Instant.ofEpochMilli(ms).atZone(zone)
    val sameYear = d.year == Instant.now().atZone(zone).year
    return (if (sameYear && !alwaysYear) SHORT_DATE else LONG_DATE).format(d)
}

private val STOP_WORDS = setOf(
    "the", "and", "for", "did", "does", "what", "whats", "when", "who", "whos", "where",
    "how", "was", "are", "you", "your", "from", "with", "that", "this", "have", "has", "get", "got",
)

/** Crude stems for highlighting only; ranking uses the core's Porter stemmer. */
private fun queryStems(q: String): List<String> =
    q.lowercase()
        .split(Regex("[^\\p{L}\\p{N}]+"))
        .filter { it.length >= 3 && it !in STOP_WORDS }
        .map { it.removeSuffix("ing").removeSuffix("ed").removeSuffix("es").removeSuffix("s") }
        .filter { it.length >= 3 }
        .distinct()

private fun highlight(text: String, stems: List<String>, mark: Color): AnnotatedString =
    buildAnnotatedString {
        append(text)
        if (stems.isEmpty()) return@buildAnnotatedString
        Regex("[\\p{L}\\p{N}]+").findAll(text).forEach { m ->
            val w = m.value.lowercase()
            if (stems.any { w.startsWith(it) }) {
                addStyle(
                    SpanStyle(fontWeight = FontWeight.SemiBold, background = mark),
                    m.range.first,
                    m.range.last + 1,
                )
            }
        }
    }
