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

// A mux mounted on another: through `StripPrefix` it serves under the prefix, as the handler
// itself it serves where it is written, and in a wrapper Meada does not read it serves nowhere.
func Mounted() {
	root := http.NewServeMux()
	admin := http.NewServeMux()
	admin.HandleFunc("GET /stats", Stats)
	root.Handle("/admin/", http.StripPrefix("/admin", admin))
	plain := http.NewServeMux()
	plain.HandleFunc("GET /plain/ledger", Ledger)
	root.Handle("/plain/", plain)
	wrapped := http.NewServeMux()
	wrapped.HandleFunc("GET /wrapped/ledger", Ledger)
	root.Handle("/w/", http.StripPrefix("/w", Guard(wrapped)))
}
