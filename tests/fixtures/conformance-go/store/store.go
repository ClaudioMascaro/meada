package store

import "fmt"

type Store struct {
	name string
}

func New() *Store {
	helper()
	open()
	// The variant the constraints build for the target.
	sysOpen()
	// The variant that needs no cgo.
	encode()
	return &Store{name: fmt.Sprintf("store-%d", 1)}
}

func (s *Store) Get(key string) string {
	return s.name + key
}

func open() {}

// A package may hold several `init` functions, and blank ones no code names.
func init() {
	open()
}

func init() {
	helper()
}

func _() {
	open()
}
