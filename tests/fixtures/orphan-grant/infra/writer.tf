data "aws_iam_policy_document" "writer" {
  statement {
    actions       = ["sqs:SendMessage"]
    not_resources = ["arn:aws:sqs:us-east-1:123456789012:acme-dead"]
  }
  statement {
    actions   = ["sns:Publish"]
    resources = ["arn:aws:sns:us-east-1:123456789012:acme-events"]
  }
}
