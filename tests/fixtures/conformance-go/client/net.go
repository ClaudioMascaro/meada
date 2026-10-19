package client

import (
	"context"
	"fmt"
	"net/http"
	"strings"

	"example.com/acme/shop/client/config"
	"example.com/acme/shop/client/routes"
)

// The host of the peer API, a constant of the package.
const Host = "https://api.example.com"

// A package-level var nothing assigns folds like a constant; one assigned in a function, here
// or in another file of the package, does not.
var (
	Orders  = "/orders"
	Reports = "/reports"
	Tenants = "/tenants"
)

func init() {
	Reports = "/reports/v2"
}

// The functions of the package, on its default client and on a `Client`.
func Package(body *strings.Reader, form map[string][]string) {
	http.Get(Host + "/health")
	http.Head("https://api.example.com/ping")
	http.Post(Host+"/items", "application/json", body)
	http.PostForm(Host+"/forms", form)
}

func Clients(c *http.Client) {
	c.Get(Host + "/on-client")
	c.Head(Host + "/on-client")
	c.Post(Host+"/on-client", "text/plain", nil)
	c.PostForm(Host+"/on-client", nil)
	http.DefaultClient.Get(Host + "/on-default")
	local := &http.Client{}
	local.Get(Host + "/on-local")
}

// The request construction is the consumer, sent or not.
func Requests(ctx context.Context) {
	http.NewRequest(http.MethodGet, Host+"/req/get", nil)
	http.NewRequest("POST", Host+"/req/post", nil)
	http.NewRequestWithContext(ctx, http.MethodDelete, Host+"/req/delete", nil)
	http.NewRequestWithContext(ctx, strings.ToUpper("put"), Host+"/req/unread", nil)
}

// How a URL is built: raw strings, `+`, `fmt.Sprintf` verbs, constants and package-level names.
func Urls(id int, name string) {
	http.Get(`https://api.example.com/raw`)
	http.Get("https://api.example.com" + "/users/" + name)
	http.Get(fmt.Sprintf("%s/users/%d/orders", Host, id))
	http.Get(fmt.Sprintf("https://api.example.com/search/%v?q=100%%", name))
	http.Get(Host + Orders)
	http.Get(Host + routes.Users + "/" + name)
	http.Get(routes.Base + routes.Prefix + routes.Teams)
	http.Get(Host + Reports)
	http.Get(Host + Tenants)
	http.Get(Host + routes.Moved)
}

// A call whose path cannot be evaluated is an unresolved consumer.
func Unresolved(id int) {
	http.Get(lookup())
	http.Get(fmt.Sprintf("%x", id))
	http.Get(Host + fmt.Sprintf("/odd/%[1]d", id))
}

func lookup() string { return "" }

// Not consumers: `Get` on a header, on a request URL and on a type of the repository.
func Lookup(r *http.Request, s *Cache) string {
	r.URL.Query().Get("q")
	s.Get("key")
	return r.Header.Get("X-Id")
}

// A package variable another package writes through a selector holds no one value; one nobody
// writes folds.
func Written() {
	http.Get(config.Base + "/a")
	http.Get("https://x.example.com" + config.Path)
	http.Get(Host + config.Mode)
	http.Get(Host + config.Stable)
}

// A format flag, width or precision changes what is printed; `%d` prints an integer.
func Formats(name string) {
	http.Get(fmt.Sprintf("https://api.example.com/pad/%5s", "ab"))
	http.Get(fmt.Sprintf("https://api.example.com/prec/%.1s", "abc"))
	http.Get(fmt.Sprintf("https://api.example.com/num/%d", "ab"))
	http.Get(fmt.Sprintf("https://api.example.com/v%d/items/%v", 2, 7))
	http.Get(fmt.Sprintf("https://api.example.com/plain/%s/%v", "ab", name))
}

type Cache struct{}

func (c *Cache) Get(key string) string { return key }
