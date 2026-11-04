variable "bucket" {}

resource "aws_sns_topic" "this" {
  name = "bucket-events"
  tags = {
    bucket = var.bucket
  }
}

output "arn" {
  value = aws_sns_topic.this.arn
}
