import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Case b (#775): the fields of case a, in a repository where `Mirror.java` reads one by
// reflection and hands its builder to `Other.java`, which sets it through a variable of its name.
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
