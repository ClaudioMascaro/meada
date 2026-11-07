resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_sqs_queue" "unused" { name = "unused" }
resource "aws_s3_bucket" "archive" { bucket = "archive" }
resource "aws_s3_bucket" "unused" { bucket = "unused" }
