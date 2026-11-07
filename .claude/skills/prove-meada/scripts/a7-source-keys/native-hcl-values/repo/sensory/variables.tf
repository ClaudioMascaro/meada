variable "proof" {
  default = {
    plain = "native-secret-saffron"
    nested = ["native-secret-walnut", null]
    quantity = 7319
    enabled = true
    opaque = upper("native-secret-call")
    template = "native-secret-${var.proof}"
  }
  sensitive = true 
  description = "source-authored sensitivity control"
}
