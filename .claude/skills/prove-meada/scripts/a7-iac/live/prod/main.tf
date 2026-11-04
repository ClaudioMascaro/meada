data "aws_iam_policy_document" "read" {
  count = 2
}

locals {
  base   = "acme-${var.env}"
  first  = aws_sns_topic.alerts.arn
  second = local.first
  loop_a = local.loop_b
  loop_b = local.loop_a
}

variable "env" {
  default = "prod"
}

resource "aws_sns_topic" "alerts" {
  name = "alerts-prod"
}

module "logs" {
  source = "../../modules/bucket"
  name   = "acme-logs"
  topic  = local.second
}

module "assets" {
  source = "../../modules/bucket"
  name   = "acme-assets"
  topic  = aws_sns_topic.alerts.arn
}

resource "aws_s3_bucket_policy" "logs" {
  bucket = module.logs.id
  policy = data.aws_iam_policy_document.read[1].json
}

resource "aws_cloudwatch_metric_alarm" "regions" {
  for_each   = { eu = "eu-west-1", "us-east" = "us-east-1" }
  alarm_name = "size-${each.key}"
  dimensions = {
    Buckets = join(",", module.assets.topics[*])
  }
  depends_on = [module.logs, aws_sns_topic.alerts]
}

resource "aws_sqs_queue" "open" {
  for_each = var.queues
  name     = each.key
}

resource "aws_sqs_queue" "none" {
  count = 0
}

resource "aws_sqs_queue" "loop" {
  name = local.loop_a
}

module "vpc" {
  source = "terraform-aws-modules/vpc/aws"
}

module "remote" {
  source = "git::https://example.com/acme/modules.git//queue"
}

module "outside" {
  source = "../../../outside"
}
