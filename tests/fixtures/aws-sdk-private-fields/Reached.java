import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A private field `Mirror.java` reaches by reflection and sets, held in a variable of its name.
class Reached {
    private final PutItemRequest.Builder reached = PutItemRequest.builder().tableName("reached-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void reachedField() {
        var r = reached.build();
        db.putItem(r);
    }
}
