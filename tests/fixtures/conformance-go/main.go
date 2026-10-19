package main

import (
	_ "embed"
	"fmt"
	str "strings"

	"github.com/gin-gonic/gin"

	"example.com/acme/shop/internal/shapes"
	"example.com/acme/shop/store"
	. "example.com/acme/shop/internal/units"
)

func main() {
	c := shapes.NewCircle(2)
	fmt.Println(c.Area())
	Meters(3)
	str.ToUpper("x")
	gin.Default()
	run()
}

func run() {
	shapes.Each(nil, nil)
	// Any file of the package directory exports a name; one it keeps is no export.
	store.New()
	store.Exported()
	store.helper()
}
