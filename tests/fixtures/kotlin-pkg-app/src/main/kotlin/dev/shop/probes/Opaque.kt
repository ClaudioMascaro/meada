package dev.shop.probes

import dev.kt.core.*
import outside.absent.Money

// A class from a library outside the workspace, named like the package's `Money`, wins over the
// star import: its companion is unknown.
fun outsideMoney(): Int = Money.of(1)
