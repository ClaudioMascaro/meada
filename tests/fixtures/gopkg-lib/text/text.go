package text

import "strings"

// Greet says hello.
func Greet(name string) string {
	return Shout("hello " + name)
}

// Shout raises the voice.
func Shout(s string) string {
	return strings.ToUpper(s)
}
