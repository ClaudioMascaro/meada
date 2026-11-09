package main

import (
	"net/http"
	"os"
	"strings"
)

// A struct field holds the read every composite literal that sets it gives it, directly or
// through a constructor's parameter that every caller passes it.

type fieldConfig struct {
	API struct {
		URL string `env:"FIELD_URL"`
	}
}

type Client struct{ base string }

func New(base string) *Client { return &Client{base: base} }

// The only two constructors: both fill base from FIELD_URL.
func FromTag() *Client {
	var cfg fieldConfig
	return New(cfg.API.URL)
}

func FromGetenv() *Client { return New(strings.TrimRight(os.Getenv("FIELD_URL"), "/")) }

func (c *Client) send(method, path string) (*http.Response, error) {
	req, err := http.NewRequest(method, c.base+path, nil)
	if err != nil {
		return nil, err
	}
	return http.DefaultClient.Do(req)
}

func (c *Client) OrdersDirect() (*http.Response, error) { return http.Get(c.base + "/orders") }
func (c *Client) Orders() (*http.Response, error)       { return c.send("GET", "/orders") }

// Not reads: the field is unknown.

// Two literals give different reads.
type twoLiterals struct{ twoLit string }

func twoLiteralsA() *twoLiterals { return &twoLiterals{twoLit: os.Getenv("TWO_A_URL")} }
func twoLiteralsB() *twoLiterals { return &twoLiterals{twoLit: os.Getenv("TWO_B_URL")} }
func (t *twoLiterals) get()      { http.Get(t.twoLit + "/a") }

// Two callers of the constructor give different reads.
type twoCallers struct{ twoCall string }

func newTwoCallers(u string) *twoCallers { return &twoCallers{twoCall: u} }
func twoCallersA() *twoCallers           { return newTwoCallers(os.Getenv("CALLER_A_URL")) }
func twoCallersB() *twoCallers           { return newTwoCallers(os.Getenv("CALLER_B_URL")) }
func (t *twoCallers) get()               { http.Get(t.twoCall + "/a") }

// A literal sets the field by position.
type byPosition struct{ pos string }

func newByPosition() byPosition { return byPosition{os.Getenv("POSITION_URL")} }
func (b byPosition) get()       { http.Get(b.pos + "/a") }

// A literal whose type the source does not tell sets the field: an alias's, which Meada does
// not read as the struct's.
type untold struct{ untoldBase string }

type untoldAlias = untold

var aliased = untoldAlias{untoldBase: os.Getenv("UNTOLD_URL")}

func (u untold) get() { http.Get(u.untoldBase + "/a") }

// No literal sets the field.
type unset struct{ unsetBase, other string }

func newUnset() *unset { return &unset{other: os.Getenv("UNSET_URL")} }
func (u *unset) get()  { http.Get(u.unsetBase + "/a") }

// A file writes the field through a selector.
type assigned struct{ assignedBase string }

func newAssigned() *assigned           { return &assigned{assignedBase: os.Getenv("ASSIGNED_URL")} }
func (a *assigned) set(b string)       { a.assignedBase = b }
func (a *assigned) get()               { http.Get(a.assignedBase + "/a") }

// The constructor has no caller in the repository.
type uncalled struct{ uncalledBase string }

func newUncalled(u string) *uncalled { return &uncalled{uncalledBase: u} }
func (u *uncalled) get()             { http.Get(u.uncalledBase + "/a") }

// A constructor method a call reaches by its name alone (on an interface value) may be given
// another read.
type matched struct{ matchedBase string }

type matcher struct{}

type makesMatched interface{ newMatched(u string) *matched }

func (matcher) newMatched(u string) *matched { return &matched{matchedBase: u} }
func matchedA() *matched                     { var m matcher; return m.newMatched(os.Getenv("MATCHED_URL")) }
func matchedB(i makesMatched) *matched        { return i.newMatched(os.Getenv("OTHER_MATCHED_URL")) }
func (m *matched) get()                       { http.Get(m.matchedBase + "/a") }

// A literal string, not a read.
type literal struct{ literalBase string }

func newLiteral() *literal { return &literal{literalBase: "http://localhost:3000"} }
func (l *literal) get()    { http.Get(l.literalBase + "/a") }
