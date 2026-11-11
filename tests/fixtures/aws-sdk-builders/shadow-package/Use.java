package app;

import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.*;

class Use {
    private final DynamoDbClient db = DynamoDbClient.create();

    void packaged() {
        db.putItem(PutItemRequest.builder().tableName("package-old").build());
    }
}
