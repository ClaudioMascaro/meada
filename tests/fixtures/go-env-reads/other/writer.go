package other

// A field of the same name on another type, written through a selector: no write of the
// tagged field.
type Other struct{ BaseURL string }

func Reset(o *Other) { o.BaseURL = "" }
