resource "aws_s3_bucket" "archive" { bucket = "archive" }
resource "aws_sqs_queue" "orders" { name = "arn:aws:sqs:eu-west-1:123456789012:orders" }
