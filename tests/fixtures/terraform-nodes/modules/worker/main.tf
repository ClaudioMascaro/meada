variable "queue_arn" {
  type = string
}

resource "aws_lambda_event_source_mapping" "this" {
  event_source_arn = var.queue_arn
  function_name    = "orders-worker"
}
