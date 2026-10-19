package main

import (
	"example.com/acme/billing/invoice"
	"example.com/acme/legacy/old"
	sh "example.com/acme/shop/internal/shapes"
	"example.com/acme/shop/worker/jobs"
)

func report() {
	sh.NewCircle(1)
	// A function of another file of the package.
	run()
	// Through the replace.
	invoice.Total()
	// The nested module owns its subtree: this path names no directory of this module.
	jobs.Run()
	// The replace makes the module an outside one.
	old.Old()
}
