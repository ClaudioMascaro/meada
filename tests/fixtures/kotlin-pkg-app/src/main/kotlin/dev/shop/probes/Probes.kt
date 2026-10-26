package dev.shop.probes

import dev.kt.core.*

fun probe(): String = "own"

// The file's own package beats a star import.
fun shadowed(): String = probe()
