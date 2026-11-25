package b

import "net/http"

var api Api

type TwinB struct{ base string }

func NewTwinB(base string) *TwinB { return &TwinB{base: base} }

func fromTwinB() *TwinB { return NewTwinB(api.BaseURL) }

func (t *TwinB) Get() { http.Get(t.base + "/b") }
