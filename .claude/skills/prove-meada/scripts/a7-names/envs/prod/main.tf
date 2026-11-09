variable "environment" {
  type    = string
  default = "dev"
}

variable "team" {
  default = "shop"
}

variable "regions" {
  default = ["eu"]
}

locals {
  prefix  = "${var.team}-${var.environment}"
  service = "orders"
}

resource "aws_sqs_queue" "orders" {
  name = "${local.prefix}-${local.service}"
}

resource "aws_sns_topic" "events" {
  name = upper(local.service)
}

resource "aws_sqs_queue" "regional" {
  for_each = toset(["eu", "us"])
  name     = format("%s-%s", local.service, each.key)
}

resource "aws_sqs_queue" "replica" {
  for_each = toset(var.regions)
  name     = "${local.service}-replica-${each.key}"
}

resource "aws_sqs_queue" "workspace" {
  name = "${local.service}-${terraform.workspace}"
}

data "aws_caller_identity" "current" {}

resource "aws_sns_topic" "account" {
  name = "alerts-${data.aws_caller_identity.current.account_id}"
}

module "billing" {
  source = "../../modules/queue"
  name   = "${local.prefix}-billing"
}

resource "aws_sqs_queue" "billing_dlq" {
  name = "${module.billing.queue_name}-dlq"
}

module "audit" {
  source = "terraform-aws-modules/sqs/aws"
  name   = "audit"
}

resource "aws_sqs_queue" "audit_dlq" {
  name = "${module.audit.queue_name}-dlq"
}

variable "dlq" {
  default = true
}

resource "aws_sqs_queue" "optional" {
  count = var.dlq ? 2 : 0
  name  = "${local.service}-optional-${count.index}"
}

module "per_region" {
  source   = "../../modules/queue"
  for_each = toset(var.regions)
  name     = "${local.prefix}-${each.key}"
}

resource "aws_sqs_queue" "titled" {
  name = title(local.service)
}

variable "token" {
  sensitive = true
  default   = "s3cr3t"
}

resource "aws_sqs_queue" "secret" {
  name = "q-${var.token}"
}
