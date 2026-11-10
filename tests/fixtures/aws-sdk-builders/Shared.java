import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A field builder another file calls a method on is set from there.
class Shared {
    static final PutItemRequest.Builder shared = PutItemRequest.builder().tableName("shared-old");
    static final PutItemRequest.Builder alone = PutItemRequest.builder().tableName("shared-alone");
    private final DynamoDbClient db = DynamoDbClient.create();

    void sharedField() {
        var r = shared.build();
        db.putItem(r);
    }

    void aloneField() {
        var r = alone.build();
        db.putItem(r);
    }
}
