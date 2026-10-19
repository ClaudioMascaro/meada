package main

import (
	"fmt"

	"example.com/outside/lib/codec"
	"example.com/shared/kit/api"
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

func encode() {
	codec.Encode()
}

func shadowed() string {
	k := localKit{}
	return k.Greet("x")
}

type localKit struct{}

func (localKit) Greet(string) string { return "" }
