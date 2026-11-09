package com.ustrat.seeqora.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ustrat.seeqora.AppState
import com.ustrat.seeqora.DEMO_SOURCES
import com.ustrat.seeqora.SourceInfo
import kotlinx.coroutines.launch

/** Per-source consent: one toggle per source, off by default, one plain sentence each (T3). */
@Composable
fun SourcesScreen(state: AppState, say: (String) -> Unit, version: String) {
    val scope = rememberCoroutineScope()
    var confirmDelete by remember { mutableStateOf(false) }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Column(Modifier.padding(horizontal = 4.dp)) {
                Text("Sources", style = MaterialTheme.typography.headlineMedium)
                Text(
                    "Seeqora reads only what you switch on. Everything stays on this phone.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }
        }
        item { DemoNotice() }
        items(DEMO_SOURCES, key = { it.kind.name }) { info ->
            val status = state.sources.firstOrNull { it.source == info.kind }
            SourceRow(
                info = info,
                granted = status?.granted == true,
                items = status?.items ?: 0L,
                busy = info.kind in state.busy,
                onToggle = { on -> scope.launch { say(state.setGranted(info.kind, on)) } },
            )
        }
        item {
            Text(
                "Email, call log, clipboard, and audio and video come in later builds.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        item {
            OutlinedButton(
                onClick = { confirmDelete = true },
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            ) { Text("Delete everything") }
        }
        item {
            Text(
                "Core $version",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
        }
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete everything?") },
            text = {
                Text("This removes everything Seeqora has indexed and switches every source off. The originals on your phone are not touched.")
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    scope.launch { say(state.deleteEverything()) }
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun SourceRow(
    info: SourceInfo,
    granted: Boolean,
    items: Long,
    busy: Boolean,
    onToggle: (Boolean) -> Unit,
) {
    OutlinedCard(Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(start = 16.dp, end = 12.dp, top = 14.dp, bottom = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(info.name, style = MaterialTheme.typography.titleMedium)
                Text(
                    info.reads,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 2.dp),
                )
                if (granted) {
                    Text(
                        "$items items indexed",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
            }
            Spacer(Modifier.width(12.dp))
            if (busy) {
                CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
            } else {
                Switch(checked = granted, onCheckedChange = onToggle)
            }
        }
    }
}

@Composable
fun DemoNotice() {
    androidx.compose.material3.Surface(
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Text(
            "Demo build: these sources hold sample data for a made-up person, Alex Rivera. Real phone data comes with the adapters in later builds.",
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(12.dp),
        )
    }
}
