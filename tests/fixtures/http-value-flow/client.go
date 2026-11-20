package client

import (
	"net/http"
	"os"
)

type Client struct{}

func (Client) methodBase() string { return os.Getenv("API_URL") }
func functionBase() string        { return os.Getenv("API_URL") + "/v2" }
func (c Client) ViaMethod()       { _, _ = http.Get(c.methodBase() + "/api/orders") }
func ViaFunction()               { _, _ = http.Get(functionBase() + "/orders") }

func envGet(key, path string) { _, _ = http.Get(os.Getenv(key) + path) }
func ParamA()                { envGet("API_URL", "/api/orders") }
func ParamB()                { envGet("SECOND_URL", "/api/orders") }
func ParamUnknown(key string) { envGet(key, "/api/orders") }

func QueryAppend(flag bool) {
	path := "/api/orders"
	if flag { path += "?x=1" }
	_, _ = http.Get(os.Getenv("API_URL") + path)
}

func QueryRewrite(flag bool) {
	path := "/api/orders"
	if flag { path = path + "?x=1" }
	_, _ = http.Get(os.Getenv("API_URL") + path)
}

func QuerySuffix(flag bool) {
	q := ""
	if flag { q = "?x=1" }
	_, _ = http.Get(os.Getenv("API_URL") + "/api/orders" + q)
}

func PathChanged(flag bool) {
	path := "/api/orders"
	if flag { path += "/other" }
	_, _ = http.Get(os.Getenv("API_URL") + path)
}

func PathReplaced(flag bool) {
	path := "/api/orders"
	if flag { path = "/v2/orders" }
	_, _ = http.Get(os.Getenv("API_URL") + path)
}

func PathUnknown(suffix string) {
	_, _ = http.Get(os.Getenv("API_URL") + "/api/orders" + suffix)
}

func MiddleQuery(flag bool) {
	p := "/api/orders"
	if flag { p += "?filter=1" }
	_, _ = http.Get(os.Getenv("API_URL") + p + "/other")
}

func unsafeDispatch(url interface{ methodBase() string }) {
	_, _ = http.Get(url.methodBase() + "/api/orders")
}
