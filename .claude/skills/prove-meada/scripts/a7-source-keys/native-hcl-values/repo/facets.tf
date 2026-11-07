resource "neutral_widget" "proof" {
  name = "native-neutral"
  sample {
    note = "first-panel"
    flag = true
  }
  sample {
    note = "second-panel"
    amount = 8
  }
  single {
    note = "only-panel"
  }
  panel "with.dot" {
    note = "first-labeled"
  }
  panel "with.dot" {
    note = "second-labeled"
  }
  panel "elsewhere" {
    note = "single-labeled"
  }
}
