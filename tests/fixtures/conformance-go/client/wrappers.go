package client

import "net/http"

// A function whose URL depends on its parameters is a wrapper: each call site is a consumer
// with its arguments substituted, through any number of levels.
func get(path string) (*http.Response, error) {
	return http.Get(Host + path)
}

func getUser(id string) (*http.Response, error) {
	return get("/users/" + id)
}

func send(method, path string) (*http.Request, error) {
	return http.NewRequest(method, Host+path, nil)
}

func Wrapped() {
	get("/wrapped")
	getUser("42")
	send(http.MethodPut, "/wrapped/put")
	// A wrapper called with something unreadable is unresolved at its site.
	get(lookup())
}

// Wrappers that call each other end at the cycle.
func ping(path string) {
	pong(path)
}

func pong(path string) {
	if path == "" {
		ping("/")
	}
	http.Get(Host + path)
}

func Cycle() {
	ping("/cycle")
}
