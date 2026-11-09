package com.ustrat.seeqora.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Fixed brand palette (deep teal) so the app looks the same on every device.
private val Light = lightColorScheme(
    primary = Color(0xFF006A6B),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFF9EF1F1),
    onPrimaryContainer = Color(0xFF002020),
    secondaryContainer = Color(0xFFCDE8E7),
    onSecondaryContainer = Color(0xFF041F1F),
    tertiaryContainer = Color(0xFFFFE0B2),
    onTertiaryContainer = Color(0xFF2B1700),
    background = Color(0xFFF7FAF9),
    surface = Color(0xFFF7FAF9),
    surfaceContainerLow = Color(0xFFF1F5F4),
    surfaceContainer = Color(0xFFEBEFEE),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF82D5D4),
    onPrimary = Color(0xFF003737),
    primaryContainer = Color(0xFF004F50),
    onPrimaryContainer = Color(0xFF9EF1F1),
    secondaryContainer = Color(0xFF324B4B),
    onSecondaryContainer = Color(0xFFCDE8E7),
    tertiaryContainer = Color(0xFF5E4100),
    onTertiaryContainer = Color(0xFFFFDDA8),
    background = Color(0xFF0F1414),
    surface = Color(0xFF0F1414),
    surfaceContainerLow = Color(0xFF171D1D),
    surfaceContainer = Color(0xFF1B2121),
)

@Composable
fun SeeqoraTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) Dark else Light,
        content = content,
    )
}
