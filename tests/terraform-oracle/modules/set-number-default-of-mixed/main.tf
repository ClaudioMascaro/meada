variable "s" {
  type    = set(number)
  default = [2, "10"]
}
resource "a" "q" {
  for_each = {for k, v in tolist(var.s) : "${k}${v}" => v}
  name     = "queue-${each.key}"
}
