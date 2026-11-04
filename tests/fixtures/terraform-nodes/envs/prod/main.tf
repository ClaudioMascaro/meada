data "aws_caller_identity" "current" {}

resource "aws_sns_topic" "orders" {
  name = "orders-events"
  tags = {
    account = data.aws_caller_identity.current.account_id
  }
}

module "orders" {
  source    = "../../modules/queue"
  name      = "orders"
  topic_arn = aws_sns_topic.orders.arn
}

module "payments" {
  source    = "../../modules/queue"
  name      = "payments"
  topic_arn = aws_sns_topic.orders.arn
}

module "worker" {
  source    = "../../modules/worker"
  queue_arn = module.orders.queue_arn
}

resource "aws_cloudwatch_metric_alarm" "depth" {
  for_each   = toset(["orders", "payments"])
  alarm_name = "${each.key}-depth"
  dimensions = {
    QueueName = module.payments.queue_name
  }
}

resource "aws_sqs_queue" "dlq" {
  count = 2
  name  = "dead-letters"
}

resource "aws_sqs_queue_redrive_allow_policy" "dlq" {
  queue_url = aws_sqs_queue.dlq[1].url
}

module "audit" {
  source  = "terraform-aws-modules/sqs/aws"
  version = "4.0.0"
  name    = "audit"
}
