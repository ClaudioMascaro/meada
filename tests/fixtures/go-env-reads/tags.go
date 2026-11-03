package main

import "net/http"

// Struct fields tagged for an environment library are reads wherever the field is read.
type Config struct {
	API struct {
		URL string `env:"TAG_API_URL" env-required:"true"`
	}
	Plain  string `envconfig:"TAG_ENVCONFIG_URL"`
	Options string `env:"TAG_CAARLO_URL,required"`
	Bare   string `json:"bare"`
}

var cfg Config

func tagNested() {
	http.Get(cfg.API.URL + "/a")
}

func tagEnvconfig() {
	http.Get(cfg.Plain + "/a")
}

func tagOptions() {
	http.Get(cfg.Options + "/a")
}

// A field without such a tag is no read.
func tagUntagged() {
	http.Get(cfg.Bare + "/a")
}
