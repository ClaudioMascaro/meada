variable "s" {
  type    = set(string)
  default = ["b", "a", "a"]
}
resource "a" "q" {
  for_each = {for k, v in var.s : "${k}${v}" => v}
  name     = "queue-${each.key}"
}
