func Check(_ x: Int) {}
func check(_ x: Int) {}
func run() { Check(1); check(1) }

struct Holder {
  func Pick() {}
  func pick() {}
  func use() { Pick(); pick() }
}
