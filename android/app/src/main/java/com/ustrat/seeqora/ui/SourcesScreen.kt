package com.ustrat.seeqora.ui

import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.material3.Surface
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
import com.ustrat.seeqora.SourceInfo
import com.ustrat.seeqora.permissionsFor
import kotlinx.coroutines.launch
import uniffi.seeqora_core.SourceKind

/** Per-source consent: one toggle per source, off by default, one plain sentence each (T3). */
@Composable
fun SourcesScreen(state: AppState, say: (String) -> Unit, version: String) {
    val scope = rememberCoroutineScope()
    var confirmDelete by remember { mutableStateOf(false) }
    var confirmMode by remember { mutableStateOf(false) }
    // The source waiting on an Android permission or folder pick.
    var pending by remember { mutableStateOf<SourceKind?>(null) }

    fun turnOn(kind: SourceKind) = scope.launch { say(state.setGranted(kind, true)) }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { result ->
        val kind = pending ?: return@rememberLauncherForActivityResult
        pending = null
        if (result.values.any { it }) {
            turnOn(kind)
        } else {
            say("Seeqora can't read ${state.sourceList.firstOrNull { it.kind == kind }?.name ?: "this"} without Android's permission. You can allow it in Settings > Apps > Seeqora.")
        }
    }
    val folderLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri: Uri? ->
        val kind = pending
        pending = null
        if (uri == null) return@rememberLauncherForActivityResult
        state.setFolder(uri)
        if (kind != null) turnOn(kind)
        else scope.launch { state.syncAll(includePhotos = false)?.let(say) }
    }

    fun request(kind: SourceKind) {
        when {
            state.sampleMode -> turnOn(kind)
            kind == SourceKind.FILES && state.filesFolder == null -> {
                pending = kind
                folderLauncher.launch(null)
            }
            !state.hasAndroidPermission(kind) -> {
                pending = kind
                permissionLauncher.launch(permissionsFor(kind).toTypedArray())
            }
            else -> turnOn(kind)
        }
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Row(Modifier.padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Sources", style = MaterialTheme.typography.headlineMedium)
                    Text(
                        "Seeqora reads only what you switch on. Everything stays on this phone.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
            }
        }
        if (state.sampleMode) item { SampleNotice() }
        items(state.sourceList, key = { it.kind.name }) { info ->
            val status = state.sources.firstOrNull { it.source == info.kind }
            SourceRow(
                info = info,
                granted = status?.granted == true,
                items = status?.items ?: 0L,
                busy = info.kind in state.busy,
                progress = state.progress[info.kind],
                folder = if (info.kind == SourceKind.FILES && !state.sampleMode) state.filesFolder else null,
                onToggle = { on ->
                    if (on) request(info.kind)
                    else scope.launch { say(state.setGranted(info.kind, false)) }
                },
                onChangeFolder = {
                    pending = null
                    folderLauncher.launch(null)
                },
            )
        }
        item {
            OutlinedButton(
                onClick = { scope.launch { say(state.syncAll(includePhotos = true) ?: "Index is up to date.") } },
                enabled = state.anyGranted && state.busy.isEmpty(),
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            ) { Text("Refresh now") }
        }
        item {
            Text(
                if (state.sampleMode) "Email, call log and audio come in later builds."
                else "Refresh reads new items. Photos are only scanned when you tap Refresh or switch Photos on. Email, call log and audio come in later builds.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        item {
            OutlinedCard(Modifier.fillMaxWidth().padding(top = 8.dp)) {
                Row(Modifier.padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Use sample data", style = MaterialTheme.typography.titleSmall)
                        Text(
                            "Try Seeqora on a made-up phone instead of yours. Switching clears the index.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Spacer(Modifier.width(12.dp))
                    Switch(checked = state.sampleMode, onCheckedChange = { confirmMode = true }, enabled = state.busy.isEmpty())
                }
            }
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
    if (confirmMode) {
        val toSample = !state.sampleMode
        AlertDialog(
            onDismissRequest = { confirmMode = false },
            title = { Text(if (toSample) "Switch to sample data?" else "Switch to this phone's data?") },
            text = { Text("This clears the index and switches every source off. Nothing on your phone is touched.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmMode = false
                    scope.launch { say(state.setSampleMode(toSample)) }
                }) { Text("Switch") }
            },
            dismissButton = { TextButton(onClick = { confirmMode = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun SourceRow(
    info: SourceInfo,
    granted: Boolean,
    items: Long,
    busy: Boolean,
    progress: String?,
    folder: Uri?,
    onToggle: (Boolean) -> Unit,
    onChangeFolder: () -> Unit,
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
                when {
                    busy -> Text(
                        progress ?: "Indexing…",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.secondary,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                    granted -> Text(
                        "$items items indexed",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
                if (folder != null) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "Folder: ${folderName(folder)}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        TextButton(onClick = onChangeFolder, enabled = !busy) { Text("Change") }
                    }
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

private fun folderName(uri: Uri): String =
    runCatching { DocumentsContract.getTreeDocumentId(uri).substringAfterLast(':').ifEmpty { "(root)" } }
        .getOrDefault(uri.lastPathSegment ?: "folder")

@Composable
private fun SampleNotice() {
    Surface(
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Text(
            "Sample data: these sources hold a made-up phone for Alex Rivera, not your data. Turn off \"Use sample data\" below to use this phone.",
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(12.dp),
        )
    }
}
