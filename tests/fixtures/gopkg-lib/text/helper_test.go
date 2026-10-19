package text

// Helper is exported but lives in a test file: no other package sees it.
func Helper() string {
	return Greet("test")
}
