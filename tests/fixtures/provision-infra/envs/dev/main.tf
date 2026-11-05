resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_s3_bucket" "archive" { bucket = "archive" }
module "shared" { source = "../../modules/queue" }
data "aws_sqs_queue" "existing" { name = "orders" }
resource "aws_sqs_queue" "unknown" { name = terraform.workspace }
resource "aws_sns_topic" "unlisted" { name = "orders" }
resource "aws_sqs_queue" "arn" { name = "arn:aws:sqs:eu-west-1:123456789012:refunds" }
