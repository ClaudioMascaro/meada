import java.util.Arrays;
import java.util.List;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.ConditionCheck;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;
import software.amazon.awssdk.services.eventbridge.EventBridgeClient;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequest;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequestEntry;

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

    // A loop inside another loop or inside a lambda, and one whose init clause declares the
    // builder, build from what their last turn set just the same. A builder made in the innermost
    // loop, or set only after the loop that builds from it, is not changed.
    void loopNested(java.util.List<java.util.List<String>> groups) {
        var db = DynamoDbClient.create();
        for (var g : groups) {
            var b = PutItemRequest.builder().tableName("nested-old");
            for (String n : g) {
                PutItemRequest r = b.build();
                db.putItem(r);
                b.tableName(n);
            }
        }
    }

    void loopInLambda(java.util.List<java.util.List<String>> groups) {
        var db = DynamoDbClient.create();
        for (var g : groups) {
            Runnable run = () -> {
                var b = PutItemRequest.builder().tableName("in-lambda-old");
                for (String n : g) {
                    PutItemRequest r = b.build();
                    db.putItem(r);
                    b.tableName(n);
                }
            };
            run.run();
        }
    }

    void loopInit(int turns) {
        var db = DynamoDbClient.create();
        for (var b = PutItemRequest.builder().tableName("init-old"); turns > 0; turns--) {
            PutItemRequest r = b.build();
            db.putItem(r);
            b.tableName("init-new");
        }
    }

    void loopInnerFresh(java.util.List<java.util.List<String>> groups) {
        var db = DynamoDbClient.create();
        for (var g : groups) {
            for (String n : g) {
                var b = PutItemRequest.builder().tableName("inner-fresh");
                PutItemRequest r = b.build();
                b.tableName(n);
                db.putItem(r);
            }
        }
    }

    void loopInnerAfter(java.util.List<java.util.List<String>> groups) {
        var db = DynamoDbClient.create();
        for (var g : groups) {
            var b = PutItemRequest.builder().tableName("inner-after");
            for (String n : g) {
                PutItemRequest r = b.build();
                db.putItem(r);
            }
            b.tableName("inner-after-new");
        }
    }

    // A builder set again inside the expression that builds from it: by a lambda a step of the
    // chain runs, by an argument of a step, by another entry of the same request. Only the steps
    // of the chain itself are read in order.
    void ownLambda() {
        var db = DynamoDbClient.create();
        var b = PutItemRequest.builder();
        db.putItem(b.tableName("own-old").overrideConfiguration(o -> b.tableName("own-new")).build());
    }

    void ownArgument() {
        var db = DynamoDbClient.create();
        var b = PutItemRequest.builder().tableName("argument-old");
        db.putItem(b.returnValues(String.valueOf(b.tableName("argument-new"))).build());
    }

    void ownNamed() {
        var db = DynamoDbClient.create();
        var b = PutItemRequest.builder();
        PutItemRequest r = b.tableName("named-old").returnValues(String.valueOf(b.tableName("named-new"))).build();
        db.putItem(r);
    }

    void ownEntry() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("entry-old");
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(pb.tableName("entry-new").build()).build(),
            TransactWriteItem.builder().put(pb.build()).build()).build());
    }

    void ownChain() {
        var db = DynamoDbClient.create();
        var b = PutItemRequest.builder().tableName("chain-old");
        db.putItem(b.tableName("own-chain").returnValues("NONE").build());
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

    // A list or an array changes in place through any name that holds it: on a transaction's
    // path one is followed only when its method writes the name twice, where it is declared and
    // where the request reads it. `List.of` cannot change.
    void listPlain() {
        var db = DynamoDbClient.create();
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("list-plain").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void listSet() {
        var db = DynamoDbClient.create();
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("list-old").build()).build());
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("list-new").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void listAlias() {
        var db = DynamoDbClient.create();
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("alias-old").build()).build());
        var same = items;
        same.set(0, TransactWriteItem.builder().put(Put.builder().tableName("alias-new").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(same).build());
    }

    void listImmutable() {
        var db = DynamoDbClient.create();
        var items = List.of(TransactWriteItem.builder().put(Put.builder().tableName("list-of").build()).build());
        System.out.println(items);
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void arrayPlain() {
        var db = DynamoDbClient.create();
        TransactWriteItem[] items = { TransactWriteItem.builder().put(Put.builder().tableName("array-plain").build()).build() };
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void arraySet() {
        var db = DynamoDbClient.create();
        TransactWriteItem[] items = { TransactWriteItem.builder().put(Put.builder().tableName("array-set-old").build()).build() };
        items[0] = TransactWriteItem.builder().put(Put.builder().tableName("array-set-new").build()).build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    // A shorter path keeps the name it had: its lists are not judged.
    void listOther() {
        var events = EventBridgeClient.create();
        var entries = Arrays.asList(PutEventsRequestEntry.builder().eventBusName("list-other").build());
        System.out.println(entries);
        events.putEvents(PutEventsRequest.builder().entries(entries).build());
    }
}
