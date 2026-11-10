// Command regexp-oracle compiles each pattern of a JSON array of strings with Go's regexp, as
// Alertmanager does, and writes a JSON array with one [a, b] pair of Go's error texts per
// pattern, "" for a compiled one: a for "^(?:" + pattern + ")$", which wraps a match_re, and b
// for "^(?s:" + pattern + ")$", which wraps a matcher value.
//
// Usage: go run scripts/regexp-oracle.go <patterns.json> [<results.json>]
// The results go to standard output without the second argument. Only Go's standard library is
// needed. The test `refusal_agrees_with_go` in src/alert/regexp.rs runs it (#718).
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
)

func main() {
	if len(os.Args) < 2 || len(os.Args) > 3 {
		fmt.Fprintln(os.Stderr, "usage: go run scripts/regexp-oracle.go <patterns.json> [<results.json>]")
		os.Exit(2)
	}
	data, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	var patterns []string
	if err := json.Unmarshal(data, &patterns); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	errors := func(pattern, wrapper string) string {
		if _, err := regexp.Compile("^(?" + wrapper + ":" + pattern + ")$"); err != nil {
			return err.Error()
		}
		return ""
	}
	results := make([][2]string, len(patterns))
	for i, pattern := range patterns {
		results[i] = [2]string{errors(pattern, ""), errors(pattern, "s")}
	}
	out, err := json.Marshal(results)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if len(os.Args) == 3 {
		err = os.WriteFile(os.Args[2], out, 0o644)
	} else {
		_, err = fmt.Println(string(out))
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
