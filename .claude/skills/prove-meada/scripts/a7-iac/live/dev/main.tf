module "logs" {
  source = "../../modules/bucket"
  name   = "acme-logs-dev"
  topic  = "arn:aws:sns:eu-west-1:123456789012:dev"
}

output "logs" {
  value = module.logs.id
}
