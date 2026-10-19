package methods

// A method of a type declared in another file of the package.
func (c *Counter) Reset() {
	c.n = 0
}
