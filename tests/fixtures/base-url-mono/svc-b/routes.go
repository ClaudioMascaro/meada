package main

import (
	"github.com/gin-gonic/gin"

	"example.com/mono/svc-b/svc"
)

type Routes struct{}

func (r *Routes) Register(rg *gin.RouterGroup) {
	g := rg.Group("/")
	g.POST("/run", r.run)
	g.POST("/w/:id/cancel", r.cancel)
}

func (r *Routes) run(c *gin.Context) {
	var req svc.RunRequest
	_ = c.ShouldBindJSON(&req)
}

func (r *Routes) cancel(c *gin.Context) {}
