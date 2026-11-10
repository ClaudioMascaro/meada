locals {
  s = []
}
resource "a" "q" {
  for_each = {for k, v in local.s : k => v}
  name     = "queue-${each.key}"
}
