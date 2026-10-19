package web

import (
	"net/http"

	"github.com/gorilla/mux"
)

func MuxIndex(w http.ResponseWriter, r *http.Request) {}

func MuxShow(w http.ResponseWriter, r *http.Request) {}

func MuxMake(w http.ResponseWriter, r *http.Request) {}

func MuxAny(w http.ResponseWriter, r *http.Request) {}

func MuxOrder(w http.ResponseWriter, r *http.Request) {}

func MuxHidden(w http.ResponseWriter, r *http.Request) {}

// `HandleFunc` and `Handle` serve every method, `.Methods(..)` limits them (several give one
// provider each), parameters have a regular expression, and a subrouter adds its prefix.
func GorillaRoutes() *mux.Router {
	r := mux.NewRouter()
	r.HandleFunc("/", MuxIndex)
	r.HandleFunc("/items/{id}", MuxShow).Methods("GET")
	r.HandleFunc("/items/{id:[0-9]+}/tags", MuxShow).Methods("GET", "HEAD")
	r.HandleFunc("/items", MuxMake).Methods(http.MethodPost)
	r.HandleFunc("/named", MuxShow).Name("named").Methods("PUT")
	r.HandleFunc("/inline", func(w http.ResponseWriter, r *http.Request) {}).Methods("DELETE")
	r.Handle("/handled", http.HandlerFunc(MuxAny))
	api := r.PathPrefix("/api").Subrouter()
	api.HandleFunc("/users", MuxShow).Methods("GET")
	v2 := api.PathPrefix("/v2/").Subrouter()
	v2.HandleFunc("/users/{id}", MuxShow).Methods("GET")
	RegisterOrders(api)
	RegisterOrders(r.PathPrefix("/shop").Subrouter())
	r.PathPrefix("/chain").Subrouter().HandleFunc("/c", MuxShow).Methods("GET")
	return r
}

// Reached under two prefixes, and from itself: a cycle adds nothing.
func RegisterOrders(s *mux.Router) {
	orders := s.PathPrefix("/orders").Subrouter()
	orders.HandleFunc("", MuxOrder).Methods("POST")
	RegisterOrders(orders)
}

// A method Meada cannot read, two `Methods` on one route (they exclude each other), and a
// prefix it cannot read serve nothing.
func GorillaUnread(verb, prefix string) {
	r := mux.NewRouter()
	r.HandleFunc("/verb", MuxHidden).Methods(verb)
	r.HandleFunc("/both", MuxHidden).Methods("GET").Methods("POST")
	r.HandleFunc("/none", MuxHidden).Methods()
	r.PathPrefix(prefix).Subrouter().HandleFunc("/dynamic", MuxHidden)
}

// A name a block or a loop binds again hides the router in the whole function, and an import a
// local rebinds is no import.
func GorillaHidden(routers []*mux.Router) {
	r := mux.NewRouter()
	r.HandleFunc("/visible", MuxShow)
	{
		r := muxOther()
		r.HandleFunc("/block", MuxHidden)
	}
	for _, r := range routers {
		r.HandleFunc("/ranged", MuxHidden)
	}
}

func GorillaShadow() {
	mux := muxOther()
	r := mux.NewRouter()
	r.HandleFunc("/shadow", MuxHidden)
}

func muxOther() *mux.Router { return nil }
