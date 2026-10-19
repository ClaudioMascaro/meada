package web

import "net/http"

func Health(w http.ResponseWriter, r *http.Request) {}

func Echo(w http.ResponseWriter, r *http.Request) {}

func GetUser(w http.ResponseWriter, r *http.Request) {}

func CreateUser(w http.ResponseWriter, r *http.Request) {}

func Files(w http.ResponseWriter, r *http.Request) {}

func Hosted(w http.ResponseWriter, r *http.Request) {}

func Home(w http.ResponseWriter, r *http.Request) {}

func Ping(w http.ResponseWriter, r *http.Request) {}

func Stats(w http.ResponseWriter, r *http.Request) {}

func Ledger(w http.ResponseWriter, r *http.Request) {}

func Guard(h http.Handler) http.Handler { return h }

// The default mux: a bare pattern serves every method, `HandlerFunc` is a conversion.
func Default() {
	http.HandleFunc("/health", Health)
	http.Handle("/echo", http.HandlerFunc(Echo))
}

// A mux of its own, with Go 1.22 patterns: a method, a parameter, a rest wildcard, a host, a
// subtree, the `{$}` anchor, a function literal and a registration function that receives it.
func Mux() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /users/{id}", GetUser)
	mux.HandleFunc("POST /users", CreateUser)
	mux.HandleFunc("/files/{path...}", Files)
	mux.HandleFunc("example.com/hosted/", Hosted)
	mux.HandleFunc("GET /{$}", Home)
	mux.HandleFunc("DELETE /users/{id}", func(w http.ResponseWriter, r *http.Request) {})
	Register(mux)
	return mux
}

func Register(mux *http.ServeMux) {
	mux.HandleFunc("GET /ping", Ping)
}

// A mux given as the handler of another sees the whole path: its routes are served where they
// fall under the outer pattern, and for the outer method alone.
func Mounted() {
	root := http.NewServeMux()
	plain := http.NewServeMux()
	plain.HandleFunc("GET /plain/ledger", Ledger)
	root.Handle("/plain/", plain)
	sub := http.NewServeMux()
	sub.HandleFunc("GET /sub/a1", Ledger)
	sub.HandleFunc("POST /a2", Ledger)
	sub.HandleFunc("/sub/any", Ledger)
	root.Handle("/sub/", sub)
	only := http.NewServeMux()
	only.HandleFunc("/static/a", Ledger)
	only.HandleFunc("POST /static/b", Ledger)
	root.Handle("GET /static/", only)
	exact := http.NewServeMux()
	exact.HandleFunc("/exact", Ledger)
	exact.HandleFunc("/exact/deeper", Ledger)
	root.Handle("/exact", exact)
}

// A mux a function returns, mounted with and without a wrapper: `StripPrefix` and every other
// middleware rewrite or wrap the handler, so the mux inside is mounted where Meada does not
// read and serves nowhere.
func Wrapped() {
	http.Handle("/returned/", newAPI())
	http.Handle("/stripped/", http.StripPrefix("/stripped", newStripped()))
	http.Handle("/guarded/", Guard(newGuarded()))
}

func newAPI() *http.ServeMux {
	api := http.NewServeMux()
	api.HandleFunc("GET /returned/k1", Ledger)
	api.HandleFunc("GET /k2", Ledger)
	return api
}

func newStripped() *http.ServeMux {
	api := http.NewServeMux()
	api.HandleFunc("GET /k3", Ledger)
	return api
}

func newGuarded() *http.ServeMux {
	api := http.NewServeMux()
	api.HandleFunc("GET /k4", Ledger)
	return api
}

type Server struct{ mux *http.ServeMux }

// A mux mounted on a router held in a field: the mount is not read, the mux serves nowhere.
func (s *Server) Routes() {
	api := http.NewServeMux()
	api.HandleFunc("GET /b1", Ledger)
	s.mux.Handle("/api/", http.StripPrefix("/api", api))
	direct := http.NewServeMux()
	direct.HandleFunc("GET /b2", Ledger)
	s.mux.Handle("/direct/", direct)
}

// In `net/http` a `:name` segment is a literal that no contract can spell, and `*` is a literal.
func Literals() {
	http.HandleFunc("/e/:id", Stats)
	http.HandleFunc("/star/*", Stats)
}
