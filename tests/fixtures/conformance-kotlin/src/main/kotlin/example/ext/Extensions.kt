package example.ext

import example.Cache
import example.Store

// Extension functions of other packages: matched by the receiver type's name.
fun Cache.describe(): String = "cache"

fun Cache.describe(verbose: Boolean): String = "cache"

fun Store.tag(): String = "store"

fun Cache.tag(): String = "cache"
