data "archive_file" "orbits" {
  type = "zip"
  source_dir = "runtime"
  output_path = "orbits.zip"
}
resource "aws_lambda_function" "orbits" {
  function_name = "orbits"
  role = "arn:aws:iam::123456789012:role/placeholder"
  runtime = "nodejs20.x"
  handler = "index.handleOrbits"
  filename = data.archive_file.orbits.output_path
}
resource "aws_api_gateway_rest_api" "public" {
  name = "orbits-rest"
}
resource "aws_api_gateway_resource" "orbits" {
  rest_api_id = aws_api_gateway_rest_api.public.id
  parent_id = aws_api_gateway_rest_api.public.root_resource_id
  path_part = "orbits"
}
resource "aws_api_gateway_resource" "one" {
  rest_api_id = aws_api_gateway_rest_api.public.id
  parent_id = aws_api_gateway_resource.orbits.id
  path_part = "{orbitId}"
}
resource "aws_api_gateway_method" "read" {
  rest_api_id = aws_api_gateway_rest_api.public.id
  resource_id = aws_api_gateway_resource.one.id
  http_method = "GET"
  authorization = "NONE"
}
resource "aws_api_gateway_integration" "orbits" {
  rest_api_id = aws_api_gateway_rest_api.public.id
  resource_id = aws_api_gateway_method.read.resource_id
  http_method = aws_api_gateway_method.read.http_method
  integration_http_method = "POST"
  type = "AWS_PROXY"
  uri = aws_lambda_function.orbits.arn
}

