resource "azurerm_servicebus_topic" "orders" { name = "orders" }
resource "azurerm_servicebus_subscription" "audit" {
 name = "audit"
 topic_id = azurerm_servicebus_topic.orders.id
}
resource "azurerm_servicebus_subscription_rule" "filter" {
 name = "important"
 subscription_id = azurerm_servicebus_subscription.audit.id
 filter_type = "SqlFilter"
 sql_filter = "importance = 'high'"
}
resource "azurerm_servicebus_queue" "slash" { name = "orders/subscriptions/audit" }
resource "azurerm_linux_function_app" "worker" {
 name = "sdk-worker"
 app_settings = {
  TOPIC = azurerm_servicebus_topic.orders.id
  SUB = azurerm_servicebus_subscription.audit.id
  QUEUE = azurerm_servicebus_queue.slash.id
  WRONG = "/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/orders"
 }
 site_config {
  application_stack {
   docker {
    registry_url = "https://ghcr.io"
    image_name = "probe/azure-sdk-worker"
    image_tag = "v1"
   }
  }
 }
}
