package main

import (
	"net/http"
	"testing"
)

func TestRun(t *testing.T) {
	req, _ := http.NewRequest("POST", "/run", nil)
	_ = req
}
