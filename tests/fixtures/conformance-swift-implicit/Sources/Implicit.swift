enum Duration { case seconds(Int) }

func wait(_ d: Duration) {}

func seconds(_ n: Int) -> Int { n }

struct Config {
  var x: Int
  init(x: Int) { self.x = x }
  static func make() -> Config { Config(x: 0) }
  static func build() -> Config { Config(x: 1) }
}

struct Other {
  static func make() -> Other { Other() }
}

func sleepy() {
  wait(.seconds(1))
}

func configure() -> Config {
  let c: Config = .make()
  let b: Config = .build()
  return c
}

func fresh() -> Config {
  return .init(x: 1)
}

func swapped() {
  var a = 1
  var b = 2
  swap(&a, &b)
}
