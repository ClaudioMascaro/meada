import java.util.Arrays;
import java.util.List;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.Get;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItem;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItemsRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

class TransactionExamples {
    void submitBatch(List<TransactWriteItem> rows) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
    }

    void submitOne(TransactWriteItem row) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(row).build());
    }

    void submitReads(List<TransactGetItem> rows) {
        var db = DynamoDbClient.create();
        db.transactGetItems(TransactGetItemsRequest.builder().transactItems(rows).build());
    }

    void bridge(List<TransactWriteItem> rows) {
        submitBatch(rows);
    }

    void immutableList() {
        submitBatch(List.of(TransactWriteItem.builder().put(Put.builder().tableName("immutable").build()).build()));
    }

    void arrayListView() {
        submitBatch(Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("array-view").build()).build()));
    }

    void oneEntry() {
        submitOne(TransactWriteItem.builder().put(Put.builder().tableName("one-entry").build()).build());
    }

    void getList() {
        submitReads(List.of(TransactGetItem.builder().get(Get.builder().tableName("lookup").build()).build()));
    }

    void sortedEntries() {
        var rows = Arrays.asList(
            TransactWriteItem.builder().put(Put.builder().tableName("sorted-cedar").build()).build(),
            TransactWriteItem.builder().put(Put.builder().tableName("sorted-elm").build()).build());
        rows.sort((left, right) -> 0);
        submitBatch(rows);
    }

    void editAfterCall() {
        var rows = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("before-edit").build()).build());
        submitBatch(rows);
        rows.set(0, TransactWriteItem.builder().put(Put.builder().tableName("after-edit").build()).build());
    }

    void twiceWrapped() {
        bridge(List.of(TransactWriteItem.builder().put(Put.builder().tableName("sends-through-bridge").build()).build()));
    }

    void changingWrapper(List<TransactWriteItem> rows) {
        var db = DynamoDbClient.create();
        rows.set(0, TransactWriteItem.builder().put(Put.builder().tableName("overwritten").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
    }

    void mutatedBeforeRead() {
        changingWrapper(Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("overwritten-initial").build()).build()));
    }

    void twoParameters(List<TransactWriteItem> rows, List<TransactWriteItem> second) {
        var db = DynamoDbClient.create();
        second.set(0, TransactWriteItem.builder().put(Put.builder().tableName("second-argument").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
    }

    void sameValueTwice() {
        var rows = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("first-argument").build()).build());
        twoParameters(rows, rows);
    }

    void writeVarargs(TransactWriteItem... rows) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
    }

    void varargsCall() {
        writeVarargs(TransactWriteItem.builder().put(Put.builder().tableName("varargs").build()).build());
    }

    void aliasWrapper(List<TransactWriteItem> rows) {
        var db = DynamoDbClient.create();
        var alias = rows;
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(alias).build());
    }

    void withAlias() {
        aliasWrapper(List.of(TransactWriteItem.builder().put(Put.builder().tableName("aliased").build()).build()));
    }

    void lambdaWrapper(List<TransactWriteItem> rows) {
        var db = DynamoDbClient.create();
        Runnable later = () -> db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
        later.run();
    }

    void deferredRead() {
        lambdaWrapper(List.of(TransactWriteItem.builder().put(Put.builder().tableName("lambda").build()).build()));
    }

    void dispatch(List<TransactWriteItem> rows) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(rows).build());
    }

    void overridden() {
        dispatch(List.of(TransactWriteItem.builder().put(Put.builder().tableName("virtual").build()).build()));
    }
}

class TransactionOverride extends TransactionExamples {
    @Override void dispatch(List<TransactWriteItem> rows) {
        // Another implementation may use or change the value differently.
    }
}
