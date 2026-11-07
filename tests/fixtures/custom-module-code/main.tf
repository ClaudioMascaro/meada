variable "unknown" {}
resource "aws_sqs_queue" "tasks" { name = "module-tasks" }
module "local_function" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "module-local"
  source_path = "worker"
  handler = "send.dispatch"
  environment_variables = { QUEUE_NAME = aws_sqs_queue.tasks.name }
}
module "package_function" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "module-package"
  create_package = false
  local_existing_package = "deploy/function.zip"
}
module "unpacked" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "wrong-directory-package"
  create_package = false
  local_existing_package = "worker"
}
module "disabled" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "disabled-local"
  create_function = false
  source_path = "worker"
}
module "package_off" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "disabled-package"
  create_package = false
  source_path = "worker"
}
module "list_source" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "unsupported-list"
  source_path = ["worker"]
}
module "parent_source" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "outside-source"
  source_path = "../outside"
}
module "unknown_source" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "unknown-source"
  source_path = var.unknown
}
module "unknown_create" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "unknown-create"
  source_path = "worker"
  create = var.unknown
}
module "remote_package" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "remote-package"
  create_package = false
  local_existing_package = "s3://artifacts/function.zip"
}
