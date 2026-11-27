package main

import "github.com/gin-gonic/gin"

func main() {
	r := gin.Default()
	r.POST("/run", run)
	r.Run(":8080")
}

func run(c *gin.Context) {}
