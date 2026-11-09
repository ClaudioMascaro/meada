variable "environment" {
  default = "dev"
}

module "billing" {
  source = "../../modules/queue"
  name   = "shop-${var.environment}-billing"
}
