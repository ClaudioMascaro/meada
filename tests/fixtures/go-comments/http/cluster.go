package http

import "github.com/gin-gonic/gin"

func newClusterRoutes(handler *gin.RouterGroup) {
	h := handler.Group("/clusters")
	h.GET("", list)
	h.GET("/:id", show)
}

func list(c *gin.Context) {}

func show(c *gin.Context) {}
