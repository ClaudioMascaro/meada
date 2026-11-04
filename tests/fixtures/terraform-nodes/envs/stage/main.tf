module "orders" {
  source    = "../../modules/queue"
  name      = "orders-stage"
  topic_arn = "arn:aws:sns:eu-west-1:123456789012:orders-stage"
}
