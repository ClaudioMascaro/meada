variable "queue" { default = "orders" }
variable "bucket" { default = "archive" }
resource "aws_sqs_queue" "orders" { name = var.queue }
resource "aws_s3_bucket" "archive" { bucket = var.bucket }
module "shared" { source = "../../modules/queue" }
resource "aws_sqs_queue" "url" {
  name = "https://sqs.eu-west-1.amazonaws.com/123456789012/refunds"
}
resource "aws_s3_bucket" "arn" { bucket = "arn:aws:s3:::archive-arn" }
