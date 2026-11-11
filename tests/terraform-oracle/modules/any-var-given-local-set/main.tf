locals {
  t = toset(["x"])
}
module "m" {
  source = "./m"
  s      = local.t
}
