package main

import (
	"net/http"

	"github.com/acme/ext"
)

// A tagged field behind a step of unknown type: `ext.Get()` is outside the repository.
type Unknown struct{ base string }

func NewUnknown(base string) *Unknown { return &Unknown{base: base} }

func unknownFromExt() *Unknown { return NewUnknown(ext.Get().Api.BaseURL) }

func (u *Unknown) Get() { http.Get(u.base + "/unknown") }
