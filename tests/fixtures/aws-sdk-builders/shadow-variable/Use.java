import software.amazon.awssdk.services.dynamodb.DynamoDbClient;

class Use {
    private final DynamoDbClient db = DynamoDbClient.create();

    // A variable named like the type hides it.
    void variable() {
        Factory PutItemRequest = new Factory();
        db.putItem(PutItemRequest.builder().tableName("variable-old").build());
    }
}
