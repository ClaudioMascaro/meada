import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.*;

class Nested {
    private final DynamoDbClient db = DynamoDbClient.create();

    // A nested type hides the type the wildcard import names.
    static class PutItemRequest {
        static Builder builder() { return new Builder(); }
    }

    static class Builder {
        Builder tableName(String name) { return this; }
        software.amazon.awssdk.services.dynamodb.model.PutItemRequest build() { return null; }
    }

    void nested() {
        db.putItem(PutItemRequest.builder().tableName("nested-old").build());
    }
}
