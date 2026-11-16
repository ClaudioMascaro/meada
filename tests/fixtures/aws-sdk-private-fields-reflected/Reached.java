import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A private field `Mirror.java` reads by reflection and hands out; `Third.java` sets the builder
// it gets, held in a variable of the field's name.
class Reached {
    private final PutItemRequest.Builder reached = PutItemRequest.builder().tableName("reached-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void reachedField() {
        var r = reached.build();
        db.putItem(r);
    }
}
