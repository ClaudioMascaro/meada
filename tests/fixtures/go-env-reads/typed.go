package main

import (
	"net/http"

	"example.com/goenv/probes/config"
)

// The base URL comes from a tagged field reached through a variable's declared type.
var typedCfg config.Config

type Typed struct{ base string }

func NewTyped(base string) *Typed { return &Typed{base: base} }

func typedFromConfig() *Typed { return NewTyped(typedCfg.Api.BaseURL) }

func (t *Typed) Get() { http.Get(t.base + "/typed") }
