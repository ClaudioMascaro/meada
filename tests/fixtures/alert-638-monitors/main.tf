resource "datadog_monitor" "app" {
  name = "[app] health check failing"
  type = "query alert"
  query = "avg(last_5m):avg:system.load.1{service:app} > 5"
  message = "Check the service"
  tags = ["service:app", "support:team-b"]
}
