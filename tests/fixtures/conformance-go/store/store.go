package store

import "fmt"

type Store struct {
	name string
}

func New() *Store {
	helper()
	open()
	return &Store{name: fmt.Sprintf("store-%d", 1)}
}

func (s *Store) Get(key string) string {
	return s.name + key
}

func open() {}
