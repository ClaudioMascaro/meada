resource "aws_sqs_queue" "shadowed" {
  name = "never-read"
}
