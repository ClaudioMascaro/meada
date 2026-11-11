variable "s" {
  type = list(string)
}
resource "a" "q" {
  for_each = {for k, v in var.s : k => v}
  name     = "queue-${each.key}"
}
