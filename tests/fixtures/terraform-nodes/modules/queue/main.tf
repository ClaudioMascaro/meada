variable "name" {
  type = string
}

variable "topic_arn" {
  type = string
}

resource "aws_sqs_queue" "this" {
  name = var.name
}

resource "aws_sns_topic_subscription" "this" {
  topic_arn = var.topic_arn
  protocol  = "sqs"
  endpoint  = aws_sqs_queue.this.arn
}

output "queue_arn" {
  value = aws_sqs_queue.this.arn
}

output "queue_name" {
  value = aws_sqs_queue.this.name
}
