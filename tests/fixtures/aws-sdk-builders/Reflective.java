import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A file that reads a field by its name as a string may change any field builder.
class Reflective {
    private final PutItemRequest.Builder byName = PutItemRequest.builder().tableName("reflect-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void reflected() throws Exception {
        ((PutItemRequest.Builder) getClass().getDeclaredField("byName").get(this)).tableName("reflect-new");
        db.putItem(byName.build());
    }
}
