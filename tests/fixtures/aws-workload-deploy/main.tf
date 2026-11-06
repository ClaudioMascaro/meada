resource "aws_sqs_queue" "orders" {
  name = "orders"
}
resource "aws_lambda_function" "worker" {
  function_name = "worker"
  package_type = "Image"
  image_uri = "ghcr.io/acme/aws-worker:v1"
  environment {
    variables = { QUEUE_NAME = "orders" }
  }
}
