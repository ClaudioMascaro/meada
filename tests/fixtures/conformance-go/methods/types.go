// Package methods calls methods through every receiver a Go source can write, and through
// receivers it does not.
package methods

import "example.com/acme/shop/internal/shapes"

type Counter struct {
	n int
}

func (c *Counter) Bump() int {
	c.n++
	return c.n
}

// The receiver is typed, so a call on it is exact.
func (c *Counter) Twice() {
	c.Bump()
	c.Bump()
}

type Log struct{}

func (l *Log) Add(msg string) {}

type Gauge struct{}

func (g Gauge) Add(msg string) {}

// Satisfied by Disk and Mem without saying so.
type Sink interface {
	Flush() error
}

type Disk struct{}

func (Disk) Flush() error { return nil }

type Mem struct{}

func (Mem) Flush() error { return nil }

// A field is typed at its declaration.
type Holder struct {
	counter *Counter
	log     Log
	sink    Sink
	circle  *shapes.Circle
	names   []string
}

func (h *Holder) Run() {
	h.counter.Bump()
	h.log.Add("run")
	h.sink.Flush()
	h.circle.Area()
}

func (h *Holder) Counter() *Counter {
	return h.counter
}

func NewCounter() *Counter {
	return &Counter{}
}

// A result list names its types; the first one is the value.
func Open() (*Counter, error) {
	return NewCounter(), nil
}

func Pair() (Log, Gauge) {
	return Log{}, Gauge{}
}

var shared = &Counter{}

var global Counter
