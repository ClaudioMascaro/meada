data "archive_file" "orbits" {
  type        = "zip"
  source_dir  = "runtime"
  output_path = "orbits.zip"
}
resource "aws_lambda_function" "orbits" {
  function_name = "orbits"
  role          = "arn:aws:iam::123456789012:role/placeholder"
  runtime       = "nodejs20.x"
  handler       = "index.handleOrbits"
  filename      = data.archive_file.orbits.output_path
}
resource "aws_apigatewayv2_api" "public" {
  name          = "orbits-http"
  protocol_type = "HTTP"
}
resource "aws_apigatewayv2_integration" "orbits" {
  api_id                 = aws_apigatewayv2_api.public.id
  integration_type       = "AWS_PROXY"
  integration_uri        = var.rollout ? aws_lambda_function.orbits.invoke_arn : aws_lambda_function.other.invoke_arn
  integration_method     = "POST"
  payload_format_version = "2.0"
}
resource "aws_apigatewayv2_route" "read" {
  api_id    = aws_apigatewayv2_api.public.id
  route_key = "GET /orbits/{orbitId}"
  target    = "integrations/${aws_apigatewayv2_integration.orbits.id}"
}

variable "rollout" { type = bool }
resource "aws_lambda_function" "other" {
  function_name = "other"
  role = "arn:aws:iam::123456789012:role/placeholder"
  handler = "index.handleOther"
  filename = data.archive_file.orbits.output_path
}

