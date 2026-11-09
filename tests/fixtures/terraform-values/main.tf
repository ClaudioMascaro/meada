terraform {
  required_version = ">= 1.6"
}

variable "environment" {
  type    = string
  default = "prd"
}

variable "db_password" {
  type      = string
  sensitive = true
  default   = "default-db-secret"
}

locals {
  prefix = "${var.environment}-orders"
  zones  = ["eu-west-1a", "eu-west-1b"]
}

resource "aws_sqs_queue" "orders" {
  name                       = "orders-queue"
  visibility_timeout_seconds = 30
  fifo_queue                 = false
  redrive_policy = jsonencode({
    # example.com/acme/app main.tf:99 resource.fake.name = "spoofed"
    deadLetterTargetArn = aws_sqs_queue.dead.arn
    maxReceiveCount     = 3
  })
  tags = {
    team          = "payments"
    "cost-center" = "42"
  }
}

resource "aws_sqs_queue" "dead" {
  name = "${local.prefix}-dead"
}

resource "aws_sqs_queue_policy" "orders" {
  queue_url = aws_sqs_queue.orders.id
  policy    = <<-EOT
    {"Statement": [{"Effect": "Allow", "Resource": "arn:aws:sqs:eu-west-1:123456789012:orders-queue"}]}
  EOT
}

resource "opsgenie_team" "payments" {
  name        = "payments-oncall"
  description = "Owns the orders queue"
}

resource "opsgenie_team_routing_rule" "payments_critical" {
  name    = "Critical payments alerts"
  team_id = opsgenie_team.payments.id
  order   = 0
  criteria {
    type = "match-all-conditions"
    conditions {
      field          = "tags"
      operation      = "contains"
      expected_value = "payments-critical"
    }
    conditions {
      field          = "priority"
      operation      = "equals"
      expected_value = "P1"
    }
  }
}
