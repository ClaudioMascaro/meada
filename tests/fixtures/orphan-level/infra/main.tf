resource "aws_sqs_queue" "jobs" {
  name = "acme-jobs"
}

resource "aws_dynamodb_table" "sessions" {
  name     = "acme-sessions"
  hash_key = "id"
  attribute {
    name = "id"
    type = "S"
  }
}
