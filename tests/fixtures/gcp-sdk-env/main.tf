resource "google_pubsub_topic" "orders" { name = "orders-env" }
resource "google_pubsub_subscription" "orders" {
  name = "orders-env"
  topic = google_pubsub_topic.orders.id
  filter = "attributes.type = \"orders\""
}
resource "google_pubsub_topic" "path" { name = "orders-path" }
resource "google_pubsub_subscription" "path" {
  name = "orders-path"
  topic = google_pubsub_topic.path.id
}
resource "google_storage_bucket" "uploads" { name = "uploads-env" }
resource "google_secret_manager_secret" "token" { secret_id = "token-env" }
resource "google_cloud_run_v2_service" "worker" {
  name = "sdk-worker"
  template {
    containers {
      image = "ghcr.io/acme/gcp-sdk-worker:v1"
      env {
        name = "TOPIC_ID"
        value = "orders-env"
      }
      env {
        name = "SUB_ID"
        value = "orders-env"
      }
      env {
        name = "TOPIC_PATH"
        value = "projects/acme/topics/orders-path"
      }
      env {
        name = "SUB_PATH"
        value = "projects/acme/subscriptions/orders-path"
      }
      env {
        name = "TYPED_TOPIC"
        value = google_pubsub_topic.orders.id
      }
      env {
        name = "TYPED_SUB"
        value = google_pubsub_subscription.orders.id
      }
      env {
        name = "BUCKET"
        value = "gs://uploads-env/object"
      }
      env {
        name = "SECRET"
        value = "projects/acme/secrets/token-env/versions/latest"
      }
    }
  }
}
