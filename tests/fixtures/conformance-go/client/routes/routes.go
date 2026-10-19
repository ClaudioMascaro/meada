package routes

// Paths of the peer API, read from another package.
const Users = "/users"

const (
	Base   = "https://api.example.com"
	Prefix = "/v2"
)

var Teams = "/teams"

// Moved is assigned by a function, so no call reads it as a path.
var Moved = "/moved"

func Move() {
	Moved = "/elsewhere"
}
