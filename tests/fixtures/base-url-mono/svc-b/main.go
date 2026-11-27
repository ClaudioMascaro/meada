package main

import "github.com/gin-gonic/gin"

func main() {
	r := gin.Default()
	routes := &Routes{}
	routes.Register(r.Group("/"))
	r.Run(":8080")
}
