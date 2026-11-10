import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.ConditionCheck;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

// Every way a builder or a built value reaches a call. A name is exact only when each mention of
// its builder is a form the reader follows; the others name no table.
class Forms {
    private final Put.Builder field = Put.builder().tableName("field-old");

    void transact(DynamoDbClient db, Put p) {
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    // A method of the builder that runs caller code changes it in place.
    void apply() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("apply-old");
        pb.applyMutation(b -> b.tableName("apply-new"));
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    // A built value copied with a consumer is a new value the reader cannot see.
    void copyNamed() {
        var db = DynamoDbClient.create();
        Put p = Put.builder().tableName("copy-old").build();
        Put q = p.copy(b -> b.tableName("copy-new"));
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(q).build()).build());
    }

    void copyInline() {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(Put.builder().tableName("inline-old").build().copy(b -> b.tableName("inline-new"))).build()).build());
    }

    // An alias is a mention of the builder, however deep, and a builder set through it changes.
    void deep() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("deep-old");
        var b1 = pb;
        var b2 = b1;
        b2.tableName("deep-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void reassigned() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("reassigned-old");
        var pb2 = pb;
        pb2 = pb2.tableName("reassigned-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void fromAlias() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("alias-old");
        var b3 = pb;
        pb.tableName("alias-new");
        Put r = b3.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(r).build()).build());
    }

    void fieldAlias() {
        var db = DynamoDbClient.create();
        var f = this.field;
        f.tableName("field-new");
        Put p = field.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    // The same forms on the other builders: a plain request, a condition check (the reader), and a
    // request builder.
    void plainDeep() {
        var db = DynamoDbClient.create();
        var pb = PutItemRequest.builder().tableName("plain-old");
        var b1 = pb;
        var b2 = b1;
        b2.tableName("plain-new");
        PutItemRequest req = pb.build();
        db.putItem(req);
    }

    void checkApply() {
        var db = DynamoDbClient.create();
        var cb = ConditionCheck.builder().tableName("check-old");
        cb.applyMutation(b -> b.tableName("check-new"));
        ConditionCheck cc = cb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().conditionCheck(cc).build()).build());
    }

    void requestReassigned() {
        var db = DynamoDbClient.create();
        var rb = TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(Put.builder().tableName("request-old").build()).build());
        var rb2 = rb;
        rb2 = rb2.transactItems(TransactWriteItem.builder().put(Put.builder().tableName("request-new").build()).build());
        var req = rb.build();
        db.transactWriteItems(req);
    }

    // Still followed: a builder set on another member, an inline copy of a builder made first,
    // and a builder started again from a built value, which names what it sets.
    void otherMember() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("other-member");
        pb.conditionExpression("attribute_not_exists(id)");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void restarted() {
        var db = DynamoDbClient.create();
        Put base = Put.builder().tableName("restarted-base").build();
        Put p = base.toBuilder().tableName("restarted").build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    // A loop builds from what its last turn set: a builder that outlives the loop is changed by a
    // mention after the build inside it. One made in the loop, or set after the loop, is not.
    void loopLater(java.util.List<String> names) {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("loop-old");
        for (String n : names) {
            Put p = pb.build();
            db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
            pb.tableName(n);
        }
    }

    void loopFresh(java.util.List<String> names) {
        var db = DynamoDbClient.create();
        for (String n : names) {
            var pb = Put.builder().tableName("loop-fresh");
            Put p = pb.build();
            pb.tableName(n);
            db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
        }
    }

    void afterLoop(java.util.List<String> names) {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("after-loop");
        for (String n : names) {
            Put p = pb.build();
            db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
        }
        pb.tableName("after-loop-new");
    }

    void lambdaLater(java.util.List<String> names) {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("lambda-old");
        names.forEach(n -> {
            Put p = pb.build();
            db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
            pb.tableName(n);
        });
    }

    // Forms the reader's model of the syntax does not show: the builder is written somewhere it
    // cannot follow, and that could be any change.
    void castAlias() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("cast-old");
        Put.Builder q = (Put.Builder) pb;
        q.tableName("cast-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void ternaryAlias(boolean flag) {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("ternary-old");
        Put.Builder q = flag ? pb : Put.builder();
        q.tableName("ternary-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void supplierAlias() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("supplier-old");
        java.util.function.Supplier<Put.Builder> supplier = () -> pb;
        supplier.get().tableName("supplier-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }

    void arrayAlias() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("array-old");
        Put.Builder[] all = { pb };
        all[0].tableName("array-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }
}
