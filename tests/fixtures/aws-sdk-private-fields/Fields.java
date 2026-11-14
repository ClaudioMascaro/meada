import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// A `private` field is named in its own file alone: a variable of its name in another file
// (`Other.java`) is another variable.
class Fields {
    private static final PutItemRequest.Builder kept = PutItemRequest.builder().tableName("kept-private");
    private final PutItemRequest.Builder own = PutItemRequest.builder().tableName("own-private");
    // Not private: another file may set each of these, so a call on its name there counts.
    static final PutItemRequest.Builder open = PutItemRequest.builder().tableName("open-old");
    static final PutItemRequest.Builder plain = PutItemRequest.builder().tableName("plain-old");
    protected final PutItemRequest.Builder guarded = PutItemRequest.builder().tableName("guarded-old");
    public final PutItemRequest.Builder exposed = PutItemRequest.builder().tableName("exposed-old");
    // Private, and set in this file by a nested class.
    private static final PutItemRequest.Builder nested = PutItemRequest.builder().tableName("nested-old");
    // Private, and handed out by a method another file sets it through.
    private static final PutItemRequest.Builder leaked = PutItemRequest.builder().tableName("leaked-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void keptField() {
        var r = kept.build();
        db.putItem(r);
    }

    void ownField() {
        var r = own.build();
        db.putItem(r);
    }

    void openField() {
        var r = open.build();
        db.putItem(r);
    }

    void plainField() {
        var r = plain.build();
        db.putItem(r);
    }

    void guardedField() {
        var r = guarded.build();
        db.putItem(r);
    }

    void exposedField() {
        var r = exposed.build();
        db.putItem(r);
    }

    void nestedField() {
        var r = nested.build();
        db.putItem(r);
    }

    void leakedField() {
        var r = leaked.build();
        db.putItem(r);
    }

    static PutItemRequest.Builder handOut() {
        return leaked;
    }

    static class Inner {
        void set() {
            nested.tableName("nested-new");
        }
    }
}

// One name, private in this class and not in the next: the file's fields are judged by name.
class Twin {
    private static final PutItemRequest.Builder twin = PutItemRequest.builder().tableName("twin-private");
    private final DynamoDbClient db = DynamoDbClient.create();

    void twinField() {
        var r = twin.build();
        db.putItem(r);
    }
}

class Open {
    static final PutItemRequest.Builder twin = PutItemRequest.builder().tableName("twin-open");
}

// `shared` is private in `Shadow` and, in `Sub`, the field `Base.java` declares: another file
// can name that one, whatever this file makes private under the same name.
class Shadow {
    private static final PutItemRequest.Builder shared = PutItemRequest.builder().tableName("shared-private");
}

class Sub extends Base {
    private final DynamoDbClient db = DynamoDbClient.create();

    void sharedField() {
        var r = shared.build();
        db.putItem(r);
    }
}
