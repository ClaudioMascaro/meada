package main

import (
	"github.com/gin-gonic/gin"

	httpcontroller "example.com/acme/svc/http"
)

func main() {
	handler := gin.New()
	// A comment inside the argument list is no argument.
	httpcontroller.NewRouter( // engine
		handler,
	)
}
