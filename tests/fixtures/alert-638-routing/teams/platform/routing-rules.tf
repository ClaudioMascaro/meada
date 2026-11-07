resource "opsgenie_team_routing_rule" "app" {
  team_id = opsgenie_team.platform.id
  criteria {
    type = "match-all-conditions"
    conditions {
      field          = "tags"
      operation      = "matches"
      expected_value = "service:app"
    }
  }
  notify {
    type = "escalation"
    id   = opsgenie_escalation.platform.id
  }
}
