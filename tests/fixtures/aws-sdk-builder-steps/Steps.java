import java.util.function.Supplier;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

// A chain that sets the table of a local builder itself and builds at once
// (`pb.tableName("t").build()`) reads its own argument, whatever statement set the builder before:
// between its step and its `build()` only the arguments of its own steps run.
class Steps {
    private final DynamoDbClient db = DynamoDbClient.create();
    private final PutItemRequest.Builder shared = PutItemRequest.builder();
    private final PutItemRequest.Builder guarded = PutItemRequest.builder();

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

    // A field builder two methods set and build, each in one chain: any method sets a field, and
    // another thread may run one chain's step between the other's step and its `build()`.
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

    // A method called in an argument of a later step sets the field after the chain's own step
    // did: the request carries `field-new`.
    void fieldSetInArgument() {
        db.putItem(guarded.tableName("field-old").conditionExpression(renamed()).build());
    }

    String renamed() {
        guarded.tableName("field-new");
        return "attribute_not_exists(pk)";
    }

    // A lambda written before the chain sets the table when an argument of a later step calls
    // it: the request carries `deferred-new`.
    void setInDeferredCode() {
        var rb = PutItemRequest.builder();
        Supplier<String> late = () -> {
            rb.tableName("deferred-new");
            return "attribute_not_exists(pk)";
        };
        db.putItem(rb.tableName("deferred-old").conditionExpression(late.get()).build());
    }

    // The lambda is handed to a method that calls it: the request carries `handed-new`.
    void setInHandedLambda() {
        var rb = PutItemRequest.builder();
        Supplier<String> late = () -> {
            rb.tableName("handed-new");
            return "attribute_not_exists(pk)";
        };
        db.putItem(rb.tableName("handed-old").conditionExpression(ask(late)).build());
    }

    String ask(Supplier<String> from) {
        return from.get();
    }

    // An anonymous class written before the chain sets the table: the request carries
    // `class-new`.
    void setInClassBody() {
        var rb = PutItemRequest.builder();
        Supplier<String> late = new Supplier<>() {
            public String get() {
                rb.tableName("class-new");
                return "attribute_not_exists(pk)";
            }
        };
        db.putItem(rb.tableName("class-old").conditionExpression(ask(late)).build());
    }

    // The same from a local class: the request carries `local-new`.
    void setInLocalClass() {
        var rb = PutItemRequest.builder();
        class Late implements Supplier<String> {
            public String get() {
                rb.tableName("local-new");
                return "attribute_not_exists(pk)";
            }
        }
        db.putItem(rb.tableName("local-old").conditionExpression(ask(new Late())).build());
    }

    // A lambda in an argument of the chain itself sets the table: it may run before `build()`.
    void setInOwnLambda() {
        var rb = PutItemRequest.builder();
        db.putItem(rb.tableName("lambda-old")
            .overrideConfiguration(c -> rb.tableName("lambda-new"))
            .build());
    }

    // The builder is handed on inside the chain: a mention the reader does not follow.
    void handedInside() {
        var rb = PutItemRequest.builder();
        db.putItem(rb.tableName("inside").conditionExpression(note(rb)).build());
    }

    String note(PutItemRequest.Builder b) {
        return "attribute_not_exists(pk)";
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
