variable "s" {
  type    = set(string)
  default = ["x"]
}

resource "aws_sqs_queue" "q" {
  for_each = { for k, v in var.s : k => v }
  name     = "queue-${each.key}-${each.value}"
}
