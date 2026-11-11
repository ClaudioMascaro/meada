variable "queue" { default = "registered-parcels" }
variable "unknown_create" {}
module "queue" {
  source = "terraform-aws-modules/sqs/aws"
  name = var.queue
}
module "topic" {
  source = "terraform-aws-modules/sns/aws"
  name = "registered-notices"
}
module "bucket" {
  source = "terraform-aws-modules/s3-bucket/aws"
  bucket = "registered-assets"
}
module "table" {
  source = "terraform-aws-modules/dynamodb-table/aws"
  name = "registered-ledger"
}
module "function" {
  source = "terraform-aws-modules/lambda/aws"
  function_name = "registered-worker"
  image_uri = "registry.example.com/registry/worker:1"
  package_type = "Image"
  create_package = false
}
module "service" {
  source = "terraform-aws-modules/ecs/aws//modules/service"
  name = "registered-service"
  container_definitions = {
    worker = { image = "registry.example.com/registry/service:1" }
  }
}
module "disabled" {
  source = "terraform-aws-modules/sqs/aws"
  name = "disabled-parcels"
  create = false
}
module "disabled_bucket" {
  source = "terraform-aws-modules/s3-bucket/aws"
  bucket = "disabled-assets"
  create_bucket = false
}
module "gated_bucket" {
  source = "terraform-aws-modules/s3-bucket/aws"
  bucket = "gated-assets"
  putin_khuylo = false
}
module "confirmed_bucket" {
  source = "terraform-aws-modules/s3-bucket/aws"
  bucket = "confirmed-assets"
  putin_khuylo = true
}
module "unknown_gate_bucket" {
  source = "terraform-aws-modules/s3-bucket/aws"
  bucket = "unconfirmed-assets"
  putin_khuylo = var.unknown_create
}
module "unknown_create" {
  source = "terraform-aws-modules/sqs/aws"
  name = "unconfirmed-parcels"
  create = var.unknown_create
}
module "unknown_name" {
  source = "terraform-aws-modules/sqs/aws"
  name = var.missing
}
module "zero" {
  source = "terraform-aws-modules/sqs/aws"
  name = "zero-parcels"
  count = 0
}
module "neighbor" {
  source = "terraform-aws-modules/sqs/aws-neighbor"
  name = "neighbor-parcels"
}
module "computed" {
  source = "terraform-aws-modules/sqs/aws"
  name = "generated-parcels"
  use_name_prefix = true
}
