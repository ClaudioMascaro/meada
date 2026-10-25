package example.other

import example.Cache

// Extensions of a package no other file imports: out of scope there.
fun Cache.describe(): String = "other"

fun String.reversed(): String = this
