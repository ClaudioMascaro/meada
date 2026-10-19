package config

// Variables another package writes through a selector hold no one value, whatever they were
// declared with; `Stable` is written by no one.
var (
	Base   = "https://default.example.com"
	Path   = "/default"
	Mode   = "/mode"
	Stable = "/stable"
)
