package web

import (
	"net/http"

	g "github.com/gin-gonic/gin"
)

func Show(c *g.Context) {}

func Save(c *g.Context) {}

func Audit(c *g.Context) {}

func Status(c *g.Context) {}

func Serve(c *g.Context) {}

func Mid(c *g.Context) {}

func Hidden(c *g.Context) {}

// Groups nest, a group is passed to a registration function and returned from another, and a
// router is registered under two prefixes.
func GinRoutes() *g.Engine {
	r := g.Default()
	v1 := r.Group("/v1")
	users := v1.Group("/users")
	users.GET("/:id", Show)
	users.POST("", Save)
	users.PUT("/:id", Save)
	users.PATCH("/:id", Save)
	users.HEAD("/:id", Show)
	users.OPTIONS("/:id", Show)
	users.DELETE("/:id", func(c *g.Context) {})
	users.Handle(http.MethodGet, "/:id/tags", Show)
	users.Handle("POST", "/:id/tags", Save)
	users.Any("/any", Save)
	r.GET("/files/*filepath", Mid, Serve)
	r.Group("/mw", Mid).GET("/chained", Show)
	RegisterAudit(v1)
	RegisterAudit(r.Group("/admin"))
	GinHidden(r, nil)
	ops := Operations(r)
	ops.GET("/status", Status)
	return r
}

func Operations(r *g.Engine) *g.RouterGroup {
	return r.Group("/ops")
}

// Reached under two prefixes, and from itself: a cycle adds nothing.
func RegisterAudit(rg *g.RouterGroup) {
	audit := rg.Group("/audit")
	audit.GET("", Audit)
	RegisterAudit(audit)
}

// Names a block, a loop or a literal binds again hide the router.
func GinHidden(r *g.Engine, routers []*g.Engine) {
	r.GET("/visible", Show)
	{
		r := other()
		r.GET("/block", Hidden)
	}
	for _, r := range routers {
		r.GET("/ranged", Hidden)
	}
	func(r *g.Engine) { r.GET("/literal", Hidden) }(nil)
}

// A name assigned again holds no one router.
func GinReassigned() {
	r := g.Default()
	r = g.New()
	r.GET("/twice", Hidden)
	h := g.Default()
	h.GET("/once", Show)
	func() { h = g.New() }()
}

// An import name a local rebinds is no import.
func GinShadow() {
	g := other()
	r := g.Default()
	r.GET("/shadow", Hidden)
}

// A closure sees the router it captures.
func GinCaptured() {
	r := g.New()
	func() { r.GET("/captured", Show) }()
}

func other() *g.Engine { return nil }

// A mux inside `gin.WrapH` is mounted where Meada does not read.
func GinWrapped() {
	r := g.Default()
	legacy := http.NewServeMux()
	legacy.HandleFunc("GET /c2", Hidden2)
	r.Any("/legacy/*rest", g.WrapH(http.StripPrefix("/legacy", legacy)))
}

func Hidden2(w http.ResponseWriter, r *http.Request) {}

// A name whose address is taken may be written through the pointer.
func GinEscaped() {
	p := "/escaped"
	setPath(&p)
	r := g.Default()
	r.GET(p, Show)
}

func setPath(p *string) { *p = "/other" }
