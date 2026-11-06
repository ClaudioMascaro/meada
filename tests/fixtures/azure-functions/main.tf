variable "tag" { default = "v1" }
resource "aws_sqs_queue" "tasks" { name = "tasks" }
resource "azurerm_linux_function_app" "image" {
  name = "image-worker"
  app_settings = {
    TASK_QUEUE = aws_sqs_queue.tasks.id
    LITERAL_QUEUE = "https://sqs.us-east-1.amazonaws.com/123456789012/tasks"
    UNKNOWN_QUEUE = var.no_default
    SECRET_QUEUE = "@Microsoft.KeyVault(SecretUri=https://keys.vault.azure.net/secrets/api-key/version)"
  }
  site_config {
    application_stack {
      docker {
        registry_url = "https://ghcr.io"
        image_name = "azure/functions"
        image_tag = var.tag
      }
    }
  }
}
resource "azurerm_windows_function_app" "archive" {
  name = "archive-worker"
  zip_deploy_file = "worker.zip"
  app_settings = { LOCAL_QUEUE = "tasks" }
}
resource "azurerm_linux_function_app" "local" {
  name = "local-worker"
  zip_deploy_file = "local.zip"
  app_settings = { LOCAL_QUEUE = "tasks" }
}
resource "azurerm_linux_function_app" "remote" {
  name = "remote-worker"
  app_settings = { WEBSITE_RUN_FROM_PACKAGE = "https://storage.example/remote.zip" }
}
