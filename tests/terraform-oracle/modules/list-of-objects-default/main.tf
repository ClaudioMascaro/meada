variable "queues" {
  type    = list(object({ n = string }))
  default = [{ n = "b" }, { n = "a" }]
}

resource "a" "q" {
  for_each = { for q in var.queues : q.n => q }
  name     = "queue-${each.key}"
}
