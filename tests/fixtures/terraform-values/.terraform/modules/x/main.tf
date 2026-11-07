resource "aws_sqs_queue" "leak" {
  name = "leaked-hidden-queue"
}
