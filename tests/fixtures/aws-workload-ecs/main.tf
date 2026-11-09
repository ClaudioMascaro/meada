resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_ecs_task_definition" "worker" {
  family = "worker"
  container_definitions = jsonencode([
    { name = "worker", image = "ghcr.io/acme/aws-worker:1", environment = [{ name = "QUEUE_NAME", value = "orders" }] },
    { name = "sidecar", image = "ghcr.io/acme/aws-sidecar:1", environment = [{ name = "QUEUE_NAME", value = "sidecar" }] },
    { name = "plain", image = "redis:7" }
  ])
}
