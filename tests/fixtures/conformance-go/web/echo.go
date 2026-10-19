package web

import (
	"github.com/labstack/echo/v4"
)

func Item(c echo.Context) error { return nil }

func Create(c echo.Context) error { return nil }

func Wide(c echo.Context) error { return nil }

func Billing(c echo.Context) error { return nil }

func Auth(next echo.HandlerFunc) echo.HandlerFunc { return next }

// Groups nest, with and without middleware, and a group goes through a registration function.
func EchoRoutes() *echo.Echo {
	e := echo.New()
	api := e.Group("/api", Auth)
	items := api.Group("/items")
	items.GET("/:id", Item)
	items.POST("", Create, Auth)
	items.PUT("/:id", Create)
	items.PATCH("/:id", Create)
	items.DELETE("/:id", func(c echo.Context) error { return nil })
	items.HEAD("/:id", Item)
	items.OPTIONS("/:id", Item)
	items.Any("/any", Wide)
	e.GET("/static/*", Wide)
	RegisterBilling(api)
	RegisterBilling(e.Group("/internal"))
	return e
}

func RegisterBilling(g *echo.Group) {
	g.GET("/billing", Billing)
}

// A prefix Meada cannot read serves nothing.
func EchoDynamic(prefix string) {
	e := echo.New()
	e.Group(prefix).GET("/dynamic", Item)
	d := e.Group(prefix)
	d.GET("/held", Item)
}
