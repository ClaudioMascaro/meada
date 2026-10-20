package main

import (
	"net/http"
	"os"
)

// A local hides the builtin `len` in this file: the test is no fallback.
func lengthShadowed() {
	len := func(string) int { return 1 }
	base := os.Getenv("SHADOW_LEN_URL")
	if len(base) == 0 {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}
