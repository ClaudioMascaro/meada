package dev.kt.core

import outside.absent.Thing

// Extensions beside no class of theirs: one of a class of this package, one of a type from
// outside the workspace, and one a member of its receiver hides.
fun Money.label(): String = "label"

fun Thing.render(): String = "render"

fun Money.total(): Int = 0
