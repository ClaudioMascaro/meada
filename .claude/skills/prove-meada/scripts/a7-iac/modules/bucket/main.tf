variable "name" {}
variable "topic" {}

resource "aws_s3_bucket" "this" {
  bucket = var.name
}

resource "aws_s3_bucket_notification" "this" {
  bucket = aws_s3_bucket.this.id
  topic {
    topic_arn = var.topic
  }
}

module "topic" {
  source = "../topic"
  count  = 2
  bucket = aws_s3_bucket.this.arn
}

output "id" {
  value = aws_s3_bucket.this.id
}

output "topics" {
  value = module.topic[*].arn
}

output "nothing" {
  value = "literal"
}
