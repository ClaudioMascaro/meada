resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_sns_topic" "events" { name = "events" }
resource "aws_s3_bucket" "archive" { bucket = "archive" }
resource "aws_dynamodb_table" "orders" { name = "orders" }
