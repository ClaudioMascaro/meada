package client

import "github.com/go-resty/resty/v2"

// A client kept in a struct field carries the base of the literals that set the field.
type Billing struct {
	rc *resty.Client
}

func NewBilling() *Billing {
	rc := resty.New()
	rc.SetBaseURL("https://api.example.com/v1")
	return &Billing{rc: rc}
}

func (b *Billing) Items() {
	b.rc.R().Get("/items")
}

const ledgerHost = "https://ledger.example.com"

type Ledger struct {
	api *resty.Client
}

func NewLedger() Ledger {
	return Ledger{api: resty.New().SetBaseURL(ledgerHost)}
}

func (l Ledger) Entries() {
	l.api.R().Get("/entries")
}

// Two literals that give the field different bases leave it unknown.
type Mirror struct {
	conn *resty.Client
}

func primary() *Mirror {
	return &Mirror{conn: resty.New().SetBaseURL("https://a.example.com")}
}

func secondary() *Mirror {
	return &Mirror{conn: resty.New().SetBaseURL("https://b.example.com")}
}

func (m *Mirror) Sync() {
	m.conn.R().Get("/sync")
}

// A literal whose client is not traced leaves the field unknown.
type Proxy struct {
	upstream *resty.Client
}

func NewProxy(c *resty.Client) *Proxy {
	return &Proxy{upstream: c}
}

func plainProxy() *Proxy {
	return &Proxy{upstream: resty.New().SetBaseURL("https://proxy.example.com")}
}

func (p *Proxy) Forward() {
	p.upstream.R().Get("/forward")
}

// A field written through a selector anywhere is unknown.
type Relay struct {
	out *resty.Client
}

func NewRelay() *Relay {
	return &Relay{out: resty.New().SetBaseURL("https://relay.example.com")}
}

func (r *Relay) Swap(c *resty.Client) {
	r.out = c
}

func (r *Relay) Send() {
	r.out.R().Post("/send")
}

// A base set on the field itself may be any instance's: unknown.
type Queue struct {
	jobs *resty.Client
}

func NewQueue() *Queue {
	return &Queue{jobs: resty.New().SetBaseURL("https://jobs.example.com")}
}

func (q *Queue) Retarget() {
	q.jobs.SetBaseURL("https://other-jobs.example.com")
}

func (q *Queue) Push() {
	q.jobs.R().Post("/push")
}

// A literal written without field names may set the field: unknown.
type Pair struct {
	left  *resty.Client
	right *resty.Client
}

func NewPair() Pair {
	return Pair{left: resty.New().SetBaseURL("https://left.example.com")}
}

func positional() Pair {
	return Pair{resty.New(), resty.New()}
}

func (p Pair) Left() {
	p.left.R().Get("/left")
}

// A literal of a slice element sets the field too.
type Pool struct {
	member *resty.Client
}

func NewPool() *Pool {
	return &Pool{member: resty.New().SetBaseURL("https://pool.example.com")}
}

func pools() []Pool {
	return []Pool{{member: resty.New()}}
}

func (p *Pool) Lease() {
	p.member.R().Get("/ping")
}

// A field no literal sets has the client its type names, with no base.
type Unset struct {
	idle *resty.Client
}

func (u *Unset) Idle() {
	u.idle.R().Get("/poll")
}
