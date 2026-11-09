resource "datadog_monitor" "checkout" {
  name = "checkout"
  tags = ["service:checkout", "support:team-b"]
}
resource "opsgenie_team_routing_rule" "checkout" {
  team_id = "platform-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "starts-with"
      expected_value = "service:check"
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
