package example

// The same package is visible without an import.
fun same(): Int = helper() + Registry.all().size + inside()

// A private top-level declaration is seen in its own file only.
fun notSecret(): Int = secret()
