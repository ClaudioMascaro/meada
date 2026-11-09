resource "aws_sns_topic" "alerts" { name = "alerts" }
resource "aws_cloudwatch_event_bus" "events" { name = "events" }
resource "aws_sqs_queue" "sns" { name = "sns-jobs" }
resource "aws_sqs_queue" "filtered" { name = "filtered-jobs" }
resource "aws_sqs_queue" "bus" { name = "bus-jobs" }
resource "aws_sqs_queue" "pattern" { name = "pattern-jobs" }
resource "aws_sns_topic_subscription" "plain" {
  topic_arn = aws_sns_topic.alerts.arn
  protocol = "sqs"
  endpoint = aws_sqs_queue.sns.arn
}
resource "aws_sns_topic_subscription" "filtered" {
  topic_arn = "arn:aws:sns:us-east-1:123456789012:alerts"
  protocol = "sqs"
  endpoint = "arn:aws:sqs:us-east-1:123456789012:filtered-jobs"
  filter_policy = jsonencode({event = ["created"]})
}
resource "aws_cloudwatch_event_rule" "plain" {
  name = "plain-rule"
  event_bus_name = aws_cloudwatch_event_bus.events.name
  # Synthetic unfiltered representation from #658: no event pattern or timer.
}
resource "aws_cloudwatch_event_target" "plain" {
  rule = aws_cloudwatch_event_rule.plain.name
  event_bus_name = aws_cloudwatch_event_bus.events.name
  arn = aws_sqs_queue.bus.arn
}
resource "aws_cloudwatch_event_rule" "pattern" {
  name = "pattern-rule"
  event_bus_name = "events"
  event_pattern = jsonencode({source = ["orders"]})
}
resource "aws_cloudwatch_event_target" "pattern" {
  rule = "pattern-rule"
  event_bus_name = "events"
  arn = "arn:aws:sqs:us-east-1:123456789012:pattern-jobs"
}
# These must never invent a route.
resource "aws_sns_topic_subscription" "email" {
  topic_arn = aws_sns_topic.alerts.arn
  protocol = "email"
  endpoint = aws_sqs_queue.sns.arn
}
resource "aws_sns_topic_subscription" "unknown" {
  topic_arn = "arn:aws:sns:us-east-1:123456789012:${terraform.workspace}"
  protocol = "sqs"
  endpoint = aws_sqs_queue.sns.arn
}
resource "aws_cloudwatch_event_target" "missing" {
  rule = "missing"
  event_bus_name = "events"
  arn = aws_sqs_queue.sns.arn
}
resource "aws_cloudwatch_event_target" "other_bus" {
  rule = "plain-rule"
  event_bus_name = "other-bus"
  arn = aws_sqs_queue.sns.arn
}
