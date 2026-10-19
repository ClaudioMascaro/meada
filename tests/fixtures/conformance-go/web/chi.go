package web

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func ChiPing(w http.ResponseWriter, r *http.Request) {}

func ChiIndex(w http.ResponseWriter, r *http.Request) {}

func ChiShow(w http.ResponseWriter, r *http.Request) {}

func ChiMake(w http.ResponseWriter, r *http.Request) {}

func ChiEdit(w http.ResponseWriter, r *http.Request) {}

func ChiAny(w http.ResponseWriter, r *http.Request) {}

func ChiFiles(w http.ResponseWriter, r *http.Request) {}

func ChiTeam(w http.ResponseWriter, r *http.Request) {}

func ChiAdmin(w http.ResponseWriter, r *http.Request) {}

func ChiHidden(w http.ResponseWriter, r *http.Request) {}

// A middleware wraps the handler it is given.
func ChiGuard(h http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { h.ServeHTTP(w, r) })
}

// Every method, `Method`, `HandleFunc` and `Handle` (every method), a wildcard, and prefixes by
// `Route` (a function literal, a named function, nested), `Group` (no prefix) and `Mount`.
func ChiRoutes() http.Handler {
	r := chi.NewRouter()
	r.Get("/ping", ChiPing)
	r.Post("/things", ChiMake)
	r.Put("/things/{id}", ChiEdit)
	r.Patch("/things/{id:[0-9]+}/patch", ChiEdit)
	r.Delete("/things/{id}", func(w http.ResponseWriter, r *http.Request) {})
	r.Head("/probe", ChiPing)
	r.Options("/probe", ChiPing)
	r.Connect("/probe", ChiPing)
	r.Trace("/probe", ChiPing)
	r.Method("GET", "/method", ChiShow)
	r.Method(http.MethodPost, "/method", ChiMake)
	r.HandleFunc("/any", ChiAny)
	r.Handle("/handled", http.HandlerFunc(ChiAny))
	r.Get("/files/*", ChiFiles)
	r.Route("/rooms", func(r chi.Router) {
		r.Get("/", ChiIndex)
		r.Route("/members/{id}", func(r chi.Router) {
			r.Get("/", ChiShow)
			r.Put("/", ChiEdit)
		})
		r.Route("/teams", teamRoutes)
	})
	r.Group(func(r chi.Router) {
		r.Get("/grouped", ChiPing)
	})
	r.Mount("/admin", adminRoutes())
	return r
}

func teamRoutes(r chi.Router) {
	r.Get("/", ChiTeam)
	r.Get("/{team}/members", ChiTeam)
}

// A router a function returns, mounted, with a router mounted on it.
func adminRoutes() chi.Router {
	r := chi.NewRouter()
	r.Get("/stats", ChiAdmin)
	r.Mount("/deep", deepRoutes())
	return r
}

func deepRoutes() *chi.Mux {
	r := chi.NewMux()
	r.Get("/x", ChiAdmin)
	return r
}

// One router mounted under two prefixes.
func ChiTwice() {
	root := chi.NewRouter()
	shared := chi.NewRouter()
	shared.Get("/shared", ChiAdmin)
	root.Mount("/a", shared)
	root.Mount("/b", shared)
}

// A router given to a function that nests it under itself: the cycle adds nothing.
func ChiNest() {
	root := chi.NewRouter()
	ChiAudit(root)
}

func ChiAudit(r chi.Router) {
	r.Route("/audit", func(r chi.Router) {
		r.Get("/", ChiTeam)
	})
	r.Route("/nested", ChiAudit)
}

// A name a block or a loop binds again hides the subrouter in the whole function, and a prefix
// Meada cannot read serves nothing.
func ChiHiddenNames(prefix string) {
	r := chi.NewRouter()
	r.Route("/shadowed", func(r chi.Router) {
		{
			r := chiOther()
			r.Post("/block", ChiHidden)
		}
		for _, r := range chiMany() {
			r.Post("/ranged", ChiHidden)
		}
		r.Post("/hidden-by-name", ChiHidden)
	})
	r.Route(prefix, func(r chi.Router) {
		r.Get("/dynamic", ChiHidden)
	})
	r.Mount(prefix, unreadable())
}

func unreadable() chi.Router {
	r := chi.NewRouter()
	r.Get("/never", ChiHidden)
	return r
}

// An import name a local rebinds is no import.
func ChiShadow() {
	chi := chiOther()
	r := chi.NewRouter()
	r.Post("/shadow", ChiHidden)
}

// A router mounted inside a call, or a handler that is no router, is read nowhere.
func ChiUnread() {
	r := chi.NewRouter()
	guarded := chi.NewRouter()
	guarded.Get("/guarded", ChiHidden)
	r.Mount("/guarded", ChiGuard(guarded))
	legacy := http.NewServeMux()
	legacy.HandleFunc("GET /legacy", ChiHidden)
	r.Mount("/legacy", legacy)
	r.Mount("/plain", http.HandlerFunc(ChiHidden))
}

func chiOther() chi.Router { return nil }

func chiMany() []chi.Router { return nil }
