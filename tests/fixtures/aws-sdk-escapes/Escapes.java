import java.util.Arrays;
import java.util.List;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

// javac translates unicode escapes before it lexes (JLS 3.3): an escaped line terminator ends a
// line comment, and the rest of the line is code that changes the table before the request.
class Escapes {
    void local() {
        var db = DynamoDbClient.create();
        List<TransactWriteItem> items = Arrays.asList(
            TransactWriteItem.builder().put(Put.builder().tableName("local-old").build()).build());
        // note \u000a items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("local-new").build()).build());
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void setter() {
        var db = DynamoDbClient.create();
        var b = PutItemRequest.builder().tableName("setter-old");
        // note \uuu000D b.tableName("setter-new");
        db.putItem(b.build());
    }

    void send(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }

    void caller() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("caller-old").build()).build());
        // note \u000A items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("caller-new").build()).build());
        send(items);
    }

    void dispatched(Base base) {
        base.store(List.of(TransactWriteItem.builder().put(Put.builder().tableName("dispatched-old").build()).build()));
    }

    // A backslash after an odd run of backslashes starts no escape: the whole line is a comment.
    void kept() {
        var items = Arrays.asList(TransactWriteItem.builder().put(Put.builder().tableName("kept").build()).build());
        // note \\u000a items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("kept-new").build()).build());
        send(items);
    }
}

class Base {
    void store(List<TransactWriteItem> items) {
        var db = DynamoDbClient.create();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(items).build());
    }
}

// note \u000a class Sub extends Base { @Override void store(List<TransactWriteItem> items) { items.set(0, TransactWriteItem.builder().put(Put.builder().tableName("dispatched-new").build()).build()); super.store(items); } }
