resource "opsgenie_team_routing_rule" "transformed" {
  team_id = "team-c-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "target:team-c"
    }
  }
  notify { type = "none" }
}
resource "pagerduty_event_orchestration_service" "checkout" {
  service = "checkout-id"
  set {
    id = "start"
    rule {
      condition { expression = "event.custom_details.support matches 'team-b'" }
      actions {}
    }
  }
  catch_all {
    actions {}
  }
}
