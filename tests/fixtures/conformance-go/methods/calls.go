package methods

import (
	"fmt"

	"example.com/acme/shop/internal/shapes"
)

func run(f func()) {}

// A type written in a parameter, by pointer or by value.
func FromParam(c *Counter) {
	c.Bump()
}

func FromValueParam(c Counter) {
	c.Bump()
}

func FromVar() {
	var c Counter
	c.Bump()
	var p *Counter
	p.Bump()
}

func FromLiteral() {
	c := Counter{}
	c.Bump()
	p := &Counter{}
	p.Bump()
}

func FromVarLiteral() {
	var c = Counter{}
	c.Bump()
}

// A call to a function whose result type is written, by name or through a package.
func FromResult() {
	c := NewCounter()
	c.Bump()
	d, err := Open()
	fmt.Println(err)
	d.Bump()
	e := shapes.NewCircle(1)
	e.Area()
}

// The second result is not known.
func FromSecondResult() {
	l, g := Pair()
	l.Add("x")
	g.Add("y")
}

func FromMethodResult(h *Holder) {
	c := h.Counter()
	c.Bump()
}

func FromQualifiedType(c *shapes.Circle) {
	c.Area()
}

func FromSibling(c *Counter) {
	c.Reset()
}

func FromPackageVar() {
	shared.Bump()
	global.Bump()
}

// A call through an interface stops at the interface method.
func FromInterface(s Sink) {
	s.Flush()
}

func FromInterfaceAsShape(s shapes.Shape) {
	s.Area()
}

// A closure sees the receivers around it, and its own parameters.
func FromClosure(c *Counter) {
	run(func() {
		c.Bump()
	})
	var t = func(c *Log) {
		c.Add("closure")
	}
	t(nil)
}

// A receiver whose type is not written: one method of that name is inferred.
func NotWritten(all []Counter) {
	for _, c := range all {
		c.Bump()
	}
}

// Several types have a method of that name: ambiguous.
func NotWrittenMany(all []Log) {
	for _, l := range all {
		l.Add("x")
	}
}

// A name bound twice in a function, to different types, is not known.
func Rebound() {
	c := Counter{}
	{
		c := Log{}
		c.Add("x")
	}
	c.Bump()
}

func ReboundByParam(c *Counter) {
	{
		c := Log{}
		c.Add("x")
	}
	c.Bump()
}

// A local type hides the package's.
func LocalType() {
	type Counter struct{}
	var c Counter
	c.Bump()
}

// A type parameter is no type of the repository.
func TypeParam[T Sink](s T) {
	s.Flush()
}

func Capture() {
	c := Counter{}
	run(func() {
		c := Log{}
		c.Add("x")
	})
	c.Bump()
}
