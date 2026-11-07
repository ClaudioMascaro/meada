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
resource "aws_apigatewayv2_api" "public" {
  name = "orbits-quick"
  protocol_type = "HTTP"
  route_key = "ANY /quick-orbits/{orbitId}"
  target = aws_lambda_function.orbits.arn
}

