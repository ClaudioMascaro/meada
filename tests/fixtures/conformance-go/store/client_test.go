package store_test

import (
	"testing"

	"example.com/acme/shop/store"
)

// The external test package is a package of its own: its `helper` is no other file's.
func helper() {}

func TestClient(t *testing.T) {
	store.New()
	helper()
}
