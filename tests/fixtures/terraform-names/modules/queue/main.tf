variable "name" {
  type = string
}

locals {
  full = "${var.name}-queue"
}

resource "aws_sqs_queue" "this" {
  name = local.full
}

output "queue_name" {
  value = local.full
}
