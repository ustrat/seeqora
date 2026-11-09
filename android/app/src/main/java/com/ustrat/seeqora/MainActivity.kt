package com.ustrat.seeqora

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ustrat.seeqora.ui.SearchScreen
import com.ustrat.seeqora.ui.SeeqoraTheme
import com.ustrat.seeqora.ui.SourcesScreen
import kotlinx.coroutines.launch
import uniffi.seeqora_core.coreVersion

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Dark-only brand: light status/nav bar icons regardless of the system setting.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        val opened = runCatching { CoreHolder.get(applicationContext) }
        setContent {
            SeeqoraTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    opened.fold(
                        onSuccess = { engine -> App(remember { AppState(applicationContext, engine) }) },
                        onFailure = { e -> StartupError(e.message ?: e.toString()) },
                    )
                }
            }
        }
    }
}

private const val TAB_UNSET = -1
private const val TAB_SEARCH = 0
private const val TAB_SOURCES = 1

@Composable
fun App(state: AppState) {
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    var tab by rememberSaveable { mutableIntStateOf(TAB_UNSET) }
    val say: (String) -> Unit = { msg ->
        scope.launch {
            snackbar.currentSnackbarData?.dismiss()
            snackbar.showSnackbar(msg)
        }
    }
    LaunchedEffect(Unit) {
        state.refresh()
        // First run: nothing is switched on, so start on the consent screen.
        if (tab == TAB_UNSET) tab = if (state.anyGranted) TAB_SEARCH else TAB_SOURCES
        // Pick up new messages, contacts, events and files. Unchanged items are
        // skipped by the core; photo OCR only runs on an explicit refresh (T5).
        state.syncAll(includePhotos = false)?.let(say)
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = tab == TAB_SEARCH,
                    onClick = { tab = TAB_SEARCH },
                    icon = { Icon(Icons.Filled.Search, contentDescription = null) },
                    label = { Text("Search") },
                )
                NavigationBarItem(
                    selected = tab == TAB_SOURCES,
                    onClick = { tab = TAB_SOURCES },
                    icon = { Icon(Icons.Filled.Lock, contentDescription = null) },
                    label = { Text("Sources") },
                )
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding)) {
            when (tab) {
                TAB_SEARCH -> SearchScreen(state, onChooseSources = { tab = TAB_SOURCES })
                TAB_SOURCES -> SourcesScreen(state, say, version = coreVersion())
            }
        }
    }
}

/** The index could not be opened. Say so plainly instead of crashing (T9). */
@Composable
fun StartupError(message: String) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Seeqora couldn't open its index", style = MaterialTheme.typography.titleLarge)
        Text(
            message,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 12.dp),
        )
    }
}
