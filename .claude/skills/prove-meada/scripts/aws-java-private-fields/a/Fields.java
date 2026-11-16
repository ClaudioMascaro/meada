import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Case a (#775): two private final field builders. No file of this repository reads a field by
// reflection, so no other file can reach either one.
class Fields {
    private static final PutItemRequest.Builder kept = PutItemRequest.builder().tableName("kept-private");
    private final PutItemRequest.Builder own = PutItemRequest.builder().tableName("own-private");
    private final DynamoDbClient db = DynamoDbClient.create();

    void keptField() {
        var r = kept.build();
        db.putItem(r);
    }

    void ownField() {
        var r = own.build();
        db.putItem(r);
    }
}
