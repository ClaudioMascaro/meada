package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
)

// A package-level variable bound to a read.
var pkgBase = os.Getenv("PKG_URL")

// A package-level constant, and a variable, as defaults.
const DefaultURL = "http://localhost:3000"

var defaultVar = "http://localhost:4000"

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

func constantDefault() {
	base := os.Getenv("CONST_URL")
	if base == "" {
		base = DefaultURL
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

func lookedUpFallback() {
	base, ok := os.LookupEnv("OK_URL")
	if !ok {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func lookedUpOrEmpty() {
	base, ok := os.LookupEnv("OK_EMPTY_URL")
	if !ok || base == "" {
		base = DefaultURL
	}
	http.Get(base + "/a")
}

func lengthFallback() {
	base := os.Getenv("LEN_URL")
	if len(base) == 0 {
		base = "http://localhost:3000"
	}
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

func fallbackToANamedRead() {
	base := os.Getenv("NAMED_MAIN_URL")
	other := os.Getenv("NAMED_OTHER_URL")
	if base == "" {
		base = other
	}
	http.Get(base + "/a")
}

func fallbackToACall() {
	base := os.Getenv("CALL_MAIN_URL")
	if base == "" {
		base = fallbackURL()
	}
	http.Get(base + "/a")
}

func trimmedAfterANamedRead() {
	raw := os.Getenv("RAW_URL")
	alt := os.Getenv("ALT_URL")
	if raw == "" {
		raw = alt
	}
	http.Get(strings.TrimSuffix(raw, "/") + "/a")
}

func fallbackToALoopVariable(alts []string) {
	base := os.Getenv("LOOP_URL")
	for _, alt := range alts {
		if base == "" {
			base = alt
		}
	}
	http.Get(base + "/a")
}

func fallbackToAVariable() {
	base := os.Getenv("VAR_URL")
	if base == "" {
		base = defaultVar
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


func okOfAnotherLookup() {
	base, _ := os.LookupEnv("A_URL")
	_, ok := os.LookupEnv("B_URL")
	if !ok {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func okOfNoLookup() {
	ok := other() != ""
	base := os.Getenv("NO_LOOKUP_URL")
	if !ok {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func okAssignedAgain() {
	base, ok := os.LookupEnv("OK_AGAIN_URL")
	ok = other() != ""
	if !ok {
		base = "http://localhost:3000"
	}
	http.Get(base + "/a")
}

func lookedUpToAVariable() {
	base, ok := os.LookupEnv("OK_VAR_URL")
	if !ok {
		base = defaultVar
	}
	http.Get(base + "/a")
}

func other() string { return "" }

func fallbackURL() string { return os.Getenv("G_URL") }
