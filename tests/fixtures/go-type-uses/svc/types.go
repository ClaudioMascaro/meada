package svc

// The request a handler of another package declares.
type Req struct {
	Name string
}

type Inner struct {
	A string
}

// A struct field's type is held by its struct.
type Outer struct {
	In Inner
}
