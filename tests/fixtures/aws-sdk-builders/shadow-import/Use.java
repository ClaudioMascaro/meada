import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import util.PutItemRequest;

class Use {
    private final DynamoDbClient db = DynamoDbClient.create();

    void imported() {
        db.putItem(PutItemRequest.builder().tableName("import-old").build());
    }

    void qualified() {
        db.putItem(util.PutItemRequest.builder().tableName("qualified-old").build());
    }
}
