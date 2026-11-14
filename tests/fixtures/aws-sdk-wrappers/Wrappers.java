import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.AttributeValue;
import software.amazon.awssdk.services.dynamodb.model.Get;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItem;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItemsRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

// A list of transaction entries a wrapper takes in a parameter and passes on (#772): its tables
// are named when no mention, in the wrapper or in its caller, can change it before the request
// reads it.
class Wrappers {
    void send(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void one(TransactWriteItem item) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(item).build());
    }

    void read(List<TransactGetItem> items) {
        var db = DynamoDbClient.create();
        db.transactGetItems(TransactGetItemsRequest.builder().transactItems(items).build());
    }

    // The builder copies the list: a change after `build()` is not in the request.
    void sendAfterBuild(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        var request = TransactWriteItemsRequest.builder().transactItems(items).build();
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("after-build-new").build()).build());
        db.transactWriteItems(request);
    }

    void sendChanged(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("wrapper-new").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void outer(List<TransactWriteItem> items) {
        send(items);
    }

    void outerChanged(List<TransactWriteItem> items) {
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("outer-new").build()).build());
        send(items);
    }

    // The second parameter is the first list too when a caller passes it twice.
    void sendWith(List<TransactWriteItem> items, List<TransactWriteItem> other) {
        var db = DynamoDbClient.create();
        other.set(0, TransactWriteItem.builder().put(Put.builder().tableName("twice-new").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void sendVarargs(TransactWriteItem... items) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void sendAliasOnly(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        var same = items;
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(same).build());
    }

    // Read when the lambda runs, which may be after the caller changed the list.
    void sendLater(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        Runnable later = () -> db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
        later.run();
    }

    void write(String table, Map<String, AttributeValue> item) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(
            TransactWriteItem.builder().put(Put.builder().tableName(table).item(item).build()).build()).build());
    }

    void listOf() {
        send(List.of(TransactWriteItem.builder().put(Put.builder().tableName("list-of").build()).build()));
    }

    void asList() {
        send(Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("as-list").build()).build()));
    }

    void entry() {
        one(TransactWriteItem.builder().put(Put.builder().tableName("entry").build()).build());
    }

    void gets() {
        read(List.of(TransactGetItem.builder().get(Get.builder().tableName("gets").build()).build()));
    }

    // Sorting a list changes the order of its entries, not their tables.
    void sorted() {
        var items = Arrays.asList(
            TransactWriteItem.builder().put(Put.builder().tableName("sorted-a").build()).build(),
            TransactWriteItem.builder().put(Put.builder().tableName("sorted-b").build()).build());
        items.sort((x, y) -> -1);
        send(items);
    }

    void afterBuild() {
        sendAfterBuild(Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("after-build").build()).build()));
    }

    // The call has read the list before it changes.
    void afterCall() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("after-call").build()).build());
        send(items);
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("after-call-new").build()).build());
    }

    void twoLevels() {
        outer(List.of(TransactWriteItem.builder().put(Put.builder().tableName("two-levels").build()).build()));
    }

    void filled() {
        Map<String, AttributeValue> item = new HashMap<>();
        item.put("id", AttributeValue.builder().s("1").build());
        write("filled", item);
    }

    // Changed in place before the request reads it: no table.
    void callerSet() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("caller-old").build()).build());
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("caller-new").build()).build());
        send(items);
    }

    void wrapperSet() {
        sendChanged(Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("wrapper-old").build()).build()));
    }

    // In a loop, the change after one call runs before the next.
    void loopAfterCall() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("loop-old").build()).build());
        for (int i = 0; i < 2; i++) {
            send(items);
            items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("loop-new").build()).build());
        }
    }

    // Handed to another method first, which may change it.
    void handedFirst() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("handed-old").build()).build());
        reorder(items);
        send(items);
    }

    void outerSet() {
        outerChanged(List.of(TransactWriteItem.builder().put(Put.builder().tableName("outer-old").build()).build()));
    }

    void twice() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("twice-old").build()).build());
        sendWith(items, items);
    }

    void varargs() {
        sendVarargs(TransactWriteItem.builder().put(Put.builder().tableName("varargs").build()).build());
    }

    void aliasOnly() {
        sendAliasOnly(List.of(TransactWriteItem.builder().put(Put.builder().tableName("alias-only").build()).build()));
    }

    void lambdaRead() {
        sendLater(List.of(TransactWriteItem.builder().put(Put.builder().tableName("lambda-read").build()).build()));
    }

    void reorder(List<TransactWriteItem> items) {
        items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("handed-new").build()).build());
    }
}
