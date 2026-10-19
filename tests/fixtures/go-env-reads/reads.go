package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
)

// A package-level variable bound to a read.
var pkgBase = os.Getenv("PKG_URL")

func main() {}

// Reads: each function's request goes to the base URL variable named in the call.

func direct() {
	http.Get(os.Getenv("DIRECT_URL") + "/a")
}

func named() {
	base := os.Getenv("NAMED_URL")
	http.Get(base + "/a")
}

func fallback() {
	base := os.Getenv("FALLBACK_URL")
	if base == "" {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func emptyOnTheLeft() {
	base := os.Getenv("LEFT_URL")
	if "" == base {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func selfTrim() {
	base := os.Getenv("SELF_URL")
	base = strings.TrimSpace(base)
	http.Get(base + "/a")
}

func trimmed() {
	base := os.Getenv("TRIMMED_URL")
	http.Get(strings.TrimSuffix(base, "/") + "/a")
}

func trimmedRight() {
	http.Get(strings.TrimRight(os.Getenv("RIGHT_URL"), "/") + "/a")
}

func lookedUp() {
	base, ok := os.LookupEnv("LOOKUP_URL")
	_ = ok
	http.Get(base + "/a")
}

func formatted() {
	http.Get(fmt.Sprintf("%s/a", os.Getenv("FORMAT_URL")))
}

func packageLevel() {
	http.Get(pkgBase + "/a")
}

// Not reads: the base URL is unknown, however close the code looks.

func twoReads() {
	http.Get(os.Getenv("FIRST_URL") + os.Getenv("SECOND_URL") + "/a")
}

func trimmedByARead() {
	http.Get(strings.TrimSuffix(os.Getenv("CUT_URL"), os.Getenv("CUT_SUFFIX")) + "/a")
}

func fallbackToAnotherRead() {
	base := os.Getenv("MAIN_URL")
	if base == "" {
		base = os.Getenv("OTHER_URL")
	}
	http.Get(base + "/a")
}

func fallbackWithElse() {
	base := os.Getenv("ELSE_URL")
	if base == "" {
		base = "http://localhost:3000"
	} else {
		base = "http://elsewhere"
	}
	http.Get(base + "/a")
}

func fallbackOnAnotherTest() {
	base := os.Getenv("TEST_URL")
	if base != "" {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func assignedAgain() {
	base := os.Getenv("AGAIN_URL")
	base = other()
	http.Get(base + "/a")
}

func fallbackOfALiteral() {
	base := "http://example.com"
	if base == "" {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func hostInTheMiddle() {
	http.Get("http://" + os.Getenv("HOST_NAME") + "/a")
}

func nameFromAVariable(key string) {
	http.Get(os.Getenv(key) + "/a")
}

func trimmedLiteral() {
	const base = "http://example.com/"
	http.Get(strings.TrimSuffix(base, "/") + "/a")
}

func trimmedOtherName() {
	base := other()
	http.Get(strings.TrimSuffix(base, "/") + "/a")
}


func other() string { return "" }
