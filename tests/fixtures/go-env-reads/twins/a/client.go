package a

import "net/http"

var api Api

type TwinA struct{ base string }

func NewTwinA(base string) *TwinA { return &TwinA{base: base} }

func fromTwinA() *TwinA { return NewTwinA(api.BaseURL) }

func (t *TwinA) Get() { http.Get(t.base + "/a") }
