package com.ustrat.seeqora.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// Brand, taken from seeqora_logo.png: a black field, dots running from sky to
// electric blue, white type. The app is dark-only to match the mark.
object Brand {
    val Black = Color(0xFF000000)
    val Blue = Color(0xFF1E6BFF)
    val Sky = Color(0xFF3DB4FF)
    val Gradient = Brush.linearGradient(listOf(Sky, Blue))
}

private val Scheme = darkColorScheme(
    primary = Color(0xFF4C9BFF),
    onPrimary = Color(0xFF00214F),
    primaryContainer = Color(0xFF0B3A8F),
    onPrimaryContainer = Color(0xFFD8E5FF),
    secondary = Brand.Sky,
    onSecondary = Color(0xFF00243A),
    secondaryContainer = Color(0xFF12304D),
    onSecondaryContainer = Color(0xFFCDE6FF),
    tertiary = Color(0xFF8FD3FF),
    tertiaryContainer = Color(0xFF0E2A44),
    onTertiaryContainer = Color(0xFFCDE6FF),
    background = Brand.Black,
    onBackground = Color(0xFFF2F5FA),
    surface = Brand.Black,
    onSurface = Color(0xFFF2F5FA),
    surfaceVariant = Color(0xFF1A2130),
    onSurfaceVariant = Color(0xFFA7B1C2),
    surfaceContainerLowest = Color(0xFF000000),
    surfaceContainerLow = Color(0xFF0A0E15),
    surfaceContainer = Color(0xFF0F141D),
    surfaceContainerHigh = Color(0xFF151B26),
    surfaceContainerHighest = Color(0xFF1B2230),
    outline = Color(0xFF3A4458),
    outlineVariant = Color(0xFF232B3A),
    error = Color(0xFFFF8A80),
)

// Light, wide lowercase headings echo the wordmark.
private val BaseType = Typography()
private val Type = BaseType.copy(
    headlineMedium = BaseType.headlineMedium.copy(fontWeight = FontWeight.Light, letterSpacing = 0.5.sp),
    titleLarge = BaseType.titleLarge.copy(fontWeight = FontWeight.Normal),
)

/** Wordmark style: light, widely tracked, lowercase. */
val WordmarkStyle = TextStyle(fontSize = 34.sp, fontWeight = FontWeight.Light, letterSpacing = 3.sp)

@Composable
fun SeeqoraTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = Scheme, typography = Type, content = content)
}
