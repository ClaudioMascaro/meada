package main

import (
	"example.com/acme/billing/invoice"
	"example.com/acme/legacy/old"
	"example.com/acme/worker/jobs"
)

func main() {
	jobs.Run()
	// Two modules of the repository declare the path, and no replace says which.
	invoice.Total()
	// No replace here: the module of the repository.
	old.Old()
}
