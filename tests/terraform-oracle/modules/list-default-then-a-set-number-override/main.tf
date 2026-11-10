variable "s" {
  type    = list(string)
  default = ["10", "9"]
}
resource "a" "q" {
  for_each = {for k, v in tolist(var.s) : "${k}${v}" => v}
  name     = "queue-${each.key}"
}
