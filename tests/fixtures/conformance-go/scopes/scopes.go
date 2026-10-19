// Package scopes holds a name bound again in every form Go has: a call through the inner
// binding never reaches the outer definition.
package scopes

import "example.com/acme/shop/store"

func outer() {}

func pick() func() { return nil }

func run(f func(func())) {}

// Not hidden: the definition.
func Plain() {
	outer()
	store.New()
}

func ShortVar() {
	if true {
		outer := pick()
		outer()
	}
}

func Var() {
	{
		var outer func()
		outer()
	}
}

func IfInit() {
	if outer := pick(); outer != nil {
		outer()
	}
}

func SwitchInit() {
	switch outer := pick(); {
	default:
		outer()
	}
}

func ForInit() {
	for outer := pick(); ; {
		outer()
		break
	}
}

func Range(all []func()) {
	for _, outer := range all {
		outer()
	}
}

func TypeSwitch(v any) {
	switch outer := v.(type) {
	case func():
		outer()
	}
}

func Select(ch chan func()) {
	select {
	case outer := <-ch:
		outer()
	}
}

func Param(outer func()) {
	outer()
}

func Named() (outer func()) {
	outer()
	return
}

func LiteralParam() {
	run(func(outer func()) {
		outer()
	})
}

func Capture() {
	outer := pick()
	run(func(func()) {
		outer()
	})
}

// An import name a local binds again.
func Import() {
	store := pick()
	store.New()
}

func Generic[outer ~int](x int) outer {
	return outer(x)
}

type Box[T ~int] struct{}

// A receiver's type argument binds the type parameter in the method.
func (b Box[outer]) Get(x int) outer {
	return outer(x)
}

type H func()

// The receiver is a name of the method.
func (outer H) Serve() {
	outer()
}

func LocalType() {
	type outer int
	_ = outer(1)
}

func LocalConst() {
	const outer = 1
	// Not valid Go: the probe needs a call through the constant.
	outer()
}

// A blank function's parameters bind in it alone.
func _(outer func()) {
	outer()
}

// So do those of a literal at the top level.
var handler = func(outer func()) {
	outer()
}
