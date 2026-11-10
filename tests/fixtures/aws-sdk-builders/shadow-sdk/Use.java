import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

class Use {
    private final DynamoDbClient db = DynamoDbClient.create();

    // The SDK's own type, imported by name, keeps its table.
    void control() {
        db.putItem(PutItemRequest.builder().tableName("sdk-control").build());
    }
}
