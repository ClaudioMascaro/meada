package main

import (
	"net/http"
	"os"
)

// A local hides the package `os` in this file: the call is not an environment read.
type fake struct{}

func (fake) Getenv(string) string { return "" }

var _ = os.Args

func shadowedPackage() {
	os := fake{}
	http.Get(os.Getenv("SHADOW_URL") + "/a")
}
