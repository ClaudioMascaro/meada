import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.ConditionCheck;
import software.amazon.awssdk.services.dynamodb.model.Delete;
import software.amazon.awssdk.services.dynamodb.model.Get;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItem;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItemsRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;
import software.amazon.awssdk.services.dynamodb.model.Update;

class Transactions {
    void write() {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(Put.builder().tableName("orders").build()).build(),
            TransactWriteItem.builder().update(Update.builder().tableName("totals").build()).build(),
            TransactWriteItem.builder().delete(Delete.builder().tableName("archive").build()).build(),
            TransactWriteItem.builder().conditionCheck(ConditionCheck.builder().tableName("customers").build()).build()
        ).build());
    }

    void read() {
        var db = DynamoDbClient.create();
        db.transactGetItems(TransactGetItemsRequest.builder().transactItems(
            TransactGetItem.builder().get(Get.builder().tableName("orders").build()).build(),
            TransactGetItem.builder().get(Get.builder().tableName("customers").build()).build()
        ).build());
    }

    void fromEnvironment() {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(Put.builder().tableName(System.getenv("TABLE")).build()).build(),
            TransactWriteItem.builder().conditionCheck(ConditionCheck.builder().tableName(System.getenv("CHECKED")).build()).build()
        ).build());
    }

    void computed() {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(Put.builder().tableName(table()).build()).build(),
            TransactWriteItem.builder().conditionCheck(ConditionCheck.builder().tableName(table()).build()).build()
        ).build());
    }
}
