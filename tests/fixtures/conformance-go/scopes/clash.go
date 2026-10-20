// Two imports that give one name: neither is sure to bind it.
package scopes

import (
	"net/http"

	"example.com/acme/shop/internal/http-utils"
)

func Clash() {
	http.NewRequest()
}
