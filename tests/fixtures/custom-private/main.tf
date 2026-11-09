variable "stage" { default = "production" }
module "private" {
  source = "git::https://example.com/platform/modules/queue?ref=v2"
  queue_name = "private-${var.stage}"
}
module "exact" {
  source = "git::https://example.com/platform/modules/priority"
  queue_name = "wrong-short-prefix"
  topic_name = "private-priority"
}
module "longest" {
  source = "git::https://example.com/platform/modules/storage/object"
  queue_name = "wrong-short-prefix-storage"
  settings = { bucket = "private-assets" }
}
module "neighbor" {
  source = "git::https://example.com/platform/modules-neighbor/queue"
  queue_name = "wrong-neighbor"
}
module "disabled" {
  source = "git::https://example.com/platform/modules/queue"
  queue_name = "disabled-private"
  create = false
}
