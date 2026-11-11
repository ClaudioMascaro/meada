package app;

import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

class Explicit {
    private final DynamoDbClient db = DynamoDbClient.create();

    // An import by name beats a type of the package.
    void explicit() {
        db.putItem(PutItemRequest.builder().tableName("explicit-control").build());
    }
}
