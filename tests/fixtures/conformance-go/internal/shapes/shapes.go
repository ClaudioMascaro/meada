package shapes

import "math"

// Shape is satisfied by every figure.
type Shape interface {
	Area() float64
	Perimeter() float64
}

type Circle struct {
	Radius float64
}

type ID string

type Label = ID

func NewCircle(r float64) *Circle {
	return &Circle{Radius: clamp(r)}
}

func (c Circle) Area() float64 {
	return math.Pi * c.Radius * c.Radius
}

func (c *Circle) Perimeter() float64 {
	return 2 * math.Pi * c.Radius
}

func clamp(r float64) float64 {
	if r < 0 {
		return 0
	}
	return r
}

func Each(shapes []Shape, visit func(Shape)) {
	for _, s := range shapes {
		visit(s)
	}
	// A function literal is no definition: its calls belong to Each.
	defer func() {
		clamp(0)
	}()
}
