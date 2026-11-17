package software.amazon.awssdk.services.dynamodb;

import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A stub of the SDK's client for the oracle: it prints the table of each item it is sent.
public class DynamoDbClient {
    public static DynamoDbClient create() {
        return new DynamoDbClient();
    }

    public void putItem(PutItemRequest request) {
        System.out.println("putItem " + request.tableName());
    }
}
