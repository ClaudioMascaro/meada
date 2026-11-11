module "m" {
  source = "./m"
  s      = toset(["x"])
}
