import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A field `Sub`, in `Fields.java`, inherits: it is this file's, and no private field of that one.
class Base {
    protected static final PutItemRequest.Builder shared = PutItemRequest.builder().tableName("shared-old");
}
