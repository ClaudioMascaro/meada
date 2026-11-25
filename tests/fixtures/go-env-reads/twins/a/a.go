package a

// A struct of the same name and field as the other twin's, with another tag.
type Api struct {
	BaseURL string `env:"TWIN_A_URL"`
}
