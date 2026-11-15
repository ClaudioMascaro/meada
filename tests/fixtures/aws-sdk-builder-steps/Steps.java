import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

// A chain that sets the table itself and builds at once (`pb.tableName("t").build()`) reads its
// own argument, whatever set the builder before: nothing runs between its step and its `build()`.
class Steps {
    private final DynamoDbClient db = DynamoDbClient.create();
    private final PutItemRequest.Builder shared = PutItemRequest.builder();

    // One builder set and built twice inside one request: each entry has its own table.
    void builtTwice() {
        var pb = Put.builder();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(pb.tableName("twice-a").build()).build(),
            TransactWriteItem.builder().put(pb.tableName("twice-b").build()).build()).build());
    }

    // The same through two named values, each built from the builder the other also sets.
    void builtTwiceNamed() {
        var pb = Put.builder();
        Put first = pb.tableName("named-a").build();
        Put second = pb.tableName("named-b").build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(first).build(),
            TransactWriteItem.builder().put(second).build()).build());
    }

    // A builder set in statements of its own, then set again by the chain that builds it.
    void setAgain() {
        var rb = PutItemRequest.builder().tableName("again-old");
        rb.tableName("again-stale");
        db.putItem(rb.tableName("again-new").build());
    }

    // A field builder two methods set and build, each in one chain.
    void sharedFirst() {
        db.putItem(shared.tableName("shared-a").build());
    }

    void sharedSecond() {
        db.putItem(shared.tableName("shared-b").build());
    }

    // The chain does not set the table: a statement did, and statement order is not followed.
    void builtAfterSet() {
        var rb = PutItemRequest.builder().tableName("after-old");
        rb.tableName("after-new");
        db.putItem(rb.build());
    }

    // An argument of a later step sets the table after the chain's own step did: Java evaluates
    // it between that step and `build()`, so the request carries `argument-new`.
    void setInArgument() {
        var rb = PutItemRequest.builder();
        db.putItem(rb.tableName("argument-old")
            .conditionExpression(rb.tableName("argument-new").toString())
            .build());
    }

    // The chain is kept unbuilt in a name, and the builder is set before that name is built.
    void keptUnbuilt() {
        var rb = PutItemRequest.builder();
        var kept = rb.tableName("kept-old");
        rb.tableName("kept-new");
        db.putItem(kept.build());
    }

    // The builder is handed to a method: a mention the reader does not follow.
    void handedBetween() {
        var rb = PutItemRequest.builder();
        keep(rb);
        db.putItem(rb.tableName("handed").build());
    }

    void keep(PutItemRequest.Builder b) {
    }
}
