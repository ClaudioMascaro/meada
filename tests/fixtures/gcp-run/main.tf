variable "image" { default = "ghcr.io/acme/gcp-worker:v2" }
variable "queue" { default = "tasks-v2" }
variable "unknown" {}

resource "google_cloud_run_service" "v1" {
  name = "legacy-service"
  template {
    spec {
      containers {
        name = "worker"
        image = "ghcr.io/acme/gcp-worker:v1"
        env {
          name = "QUEUE_URL"
          value = "tasks-v1"
        }
        env {
          name = "SECRET_QUEUE"
          value_from {
            secret_key_ref {
              name = "api-key"
              key = "latest"
            }
          }
        }
        env {
          name = "BUCKET_AS_QUEUE"
          value = google_storage_bucket.assets.name
        }
        env {
          name = "DUP_QUEUE"
          value = "first-queue"
        }
        env {
          name = "DUP_QUEUE"
          value = "second-queue"
        }
      }
      containers {
        name = "sidecar"
        image = "ghcr.io/acme/gcp-sidecar:v1"
        env {
          name = "SIDE_QUEUE"
          value = "other-container"
        }
      }
    }
  }
}

resource "google_cloud_run_v2_service" "v2" {
  name = "current-service"
  template {
    containers {
      name = "worker"
      image = var.image
      env {
        name = "V2_QUEUE"
        value = var.queue
      }
      env {
        name = "V2_SECRET"
        value_source {
          secret_key_ref {
            secret = google_secret_manager_secret.token.id
            version = "latest"
          }
        }
      }
    }
  }
}

resource "google_cloud_run_v2_job" "batch" {
  name = "batch-job"
  template {
    template {
      containers {
        name = "worker"
        image = "ghcr.io/acme/gcp-worker:v1"
        env {
          name = "JOB_QUEUE"
          value = "tasks-job"
        }
        env {
          name = "UNKNOWN_QUEUE"
          value = var.unknown
        }
      }
    }
  }
}
