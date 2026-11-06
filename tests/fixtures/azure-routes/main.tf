resource "azurerm_servicebus_topic" "orders" { name = "orders" }
resource "azurerm_servicebus_topic" "other" { name = "other" }
resource "azurerm_servicebus_queue" "slash" { name = "orders/subscriptions/audit" }
resource "azurerm_servicebus_subscription" "same" {
  name = "orders"
  topic_id = azurerm_servicebus_topic.orders.id
}
resource "azurerm_servicebus_subscription_rule" "filter" {
  name = "region"
  subscription_id = azurerm_servicebus_subscription.same.id
  filter_type = "SqlFilter"
  sql_filter = "region = 'west'"
}
resource "azurerm_servicebus_subscription" "audit" {
  name = "audit"
  topic_id = azurerm_servicebus_topic.orders.id
}
resource "azurerm_servicebus_subscription" "other" {
  name = "audit"
  topic_id = azurerm_servicebus_topic.other.id
}
