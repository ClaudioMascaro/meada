locals {
  t = toset(["x"])
  s = local.t
}
resource "a" "q" {
  for_each = {for k, v in local.s : k => v}
  name     = "queue-${each.key}"
}
