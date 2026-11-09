resource "opsgenie_team_routing_rule" "team_b" {
  name = "team-b"
}

resource "custom_rule" "hashed" {
  for_each = { "route#b" = 1 }
  name = each.key
}
