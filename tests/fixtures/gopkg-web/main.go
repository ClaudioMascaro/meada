package main

import (
	"fmt"

	"example.com/outside/lib/codec"
	"example.com/shared/kit/api"
	nest "example.com/shared/kit/extras"
	"example.com/shared/kit/missing"
	k "example.com/shared/kit/text"
)

func main() {
	fmt.Println(welcome())
}

func welcome() string {
	return k.Greet("web")
}

func connect() string {
	return api.NewClient().Create("a")
}

func peek() {
	missing.Probe()
}

// A name in a test file, in a file built for another target, in a directory of a nested module
// and in two files of one directory: none of them is certain.
func testOnly() string {
	return k.Helper()
}

func windows() {
	k.WinOnly()
}

func nested() int {
	return nest.Boost(1)
}

func dup() {
	api.Dup()
}

func encode() {
	codec.Encode()
}

func shadowed() string {
	k := localKit{}
	return k.Greet("x")
}

type localKit struct{}

func (localKit) Greet(string) string { return "" }
