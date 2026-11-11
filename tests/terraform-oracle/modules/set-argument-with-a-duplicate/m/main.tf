variable "s" {
  type = set(string)
}
resource "a" "q" {
  for_each = {for k, v in tolist(var.s) : "${k}${v}" => v}
  name     = "queue-${each.key}"
}
