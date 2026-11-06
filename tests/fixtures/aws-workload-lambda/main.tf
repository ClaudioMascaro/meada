resource "aws_sqs_queue" "jobs" {
  name = "jobs"
}
data "archive_file" "bundle" {
  type = "zip"
  source_dir = "worker"
  output_path = "bundle.zip"
}
resource "aws_lambda_function" "local" {
  function_name = "local"
  filename = data.archive_file.bundle.output_path
  handler = "send.dispatch"
  environment {
    variables = { QUEUE_NAME = aws_sqs_queue.jobs.name }
  }
}
resource "aws_lambda_function" "zip" {
  function_name = "zip"
  filename = "worker.zip"
}
resource "aws_lambda_function" "remote" {
  function_name = "remote"
  s3_bucket = "remote"
  s3_key = "remote.zip"
}
