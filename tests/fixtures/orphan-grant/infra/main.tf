resource "aws_s3_bucket" "logs" {
  bucket = "acme-shop-logs"
}

resource "aws_dynamodb_table" "sessions" {
  name     = "acme-sessions"
  hash_key = "id"
  attribute {
    name = "id"
    type = "S"
  }
}

resource "aws_sns_topic" "events" {
  name = "acme-events"
}

data "aws_iam_policy_document" "reader" {
  statement {
    actions   = ["s3:GetObject", "dynamodb:GetItem"]
    resources = [
      "arn:aws:s3:::acme-reports/*",
      "arn:aws:dynamodb:us-east-1:123456789012:table/acme-sessions",
    ]
  }
}
