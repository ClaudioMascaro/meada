resource "google_pubsub_topic" "orders" { name = "orders" }
resource "google_pubsub_subscription" "orders" {
  name = "orders"
  topic = google_pubsub_topic.orders.id
  filter = "attributes.kind = \"order\""
}
resource "google_storage_bucket" "assets" { name = "assets" }
resource "google_sql_database_instance" "primary" { name = "orders-db" }
resource "google_sql_database_instance" "audit" { name = "audit-db" }
resource "google_sql_database" "orders" {
  name = "orders"
  instance = google_sql_database_instance.primary.name
}
resource "google_sql_database" "audit" {
  name = "orders"
  instance = google_sql_database_instance.audit.id
}
resource "google_secret_manager_secret" "token" { secret_id = "api-key" }
resource "google_sql_database_instance" "computed" {}
resource "google_pubsub_topic" "wrong" { name = "projects/p/subscriptions/orders" }
