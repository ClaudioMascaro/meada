resource "azurerm_servicebus_queue" "orders" { name = "orders" }
resource "azurerm_servicebus_queue" "slash" { name = "orders/subscriptions/audit" }
resource "azurerm_servicebus_topic" "events" { name = "events" }
resource "azurerm_servicebus_subscription" "audit" {
  name = "audit"
  topic_id = azurerm_servicebus_topic.events.id
}
resource "azurerm_eventhub" "events" { name = "events" }
resource "azurerm_storage_account" "assets" { name = "assetsstore" }
resource "azurerm_storage_container" "assets" {
  name = "assetsstore"
  storage_account_id = azurerm_storage_account.assets.id
}
resource "azurerm_key_vault_secret" "token" {
  name = "api-key"
  key_vault_id = "/subscriptions/s/resourceGroups/g/providers/Microsoft.KeyVault/vaults/keys"
  value = "not-exported-secret-content"
}
