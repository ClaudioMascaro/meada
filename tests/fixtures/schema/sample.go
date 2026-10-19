package store

import (
	"fmt"
	. "example.com/shop/units"
	db "example.com/shop/sql"
)

const Prefix = "store-"

type Store struct{}

type Getter interface {
	Get(key string) string
}

func New(name string) (s *Store) {
	label := fmt.Sprint(Prefix, name)
	db.Open(label)
	return &Store{}
}

func (s *Store) Get(key string) string {
	return Meters(key)
}
