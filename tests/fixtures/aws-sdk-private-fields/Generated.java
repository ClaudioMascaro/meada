import lombok.Getter;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Lombok's `@Getter` hands a private field out with no mention of it in this file: `Other.java`
// sets the builder it returns, held in a variable of the field's name.
@Getter
class Generated {
    private final PutItemRequest.Builder given = PutItemRequest.builder().tableName("given-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void givenField() {
        var r = given.build();
        db.putItem(r);
    }
}
