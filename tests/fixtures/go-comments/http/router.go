package http

import "github.com/gin-gonic/gin"

// A comment inside the parameter list is no parameter.
func NewRouter( //nolint:funlen
	handler *gin.Engine,
) {
	handler.GET("/healthcheck", health)
	g := handler.Group("/")
	newClusterRoutes(g)
}

func health(c *gin.Context) {}
