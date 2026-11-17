import java.lang.reflect.Field;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Case c (#775, the tighter guard): one file, alone in its repository, reads its own field by
// reflection through a method reference, so no call of the file is named `getDeclaredField`,
// and sets the table of the builder it gets.
class Held {
    interface Lookup {
        Field find(String name) throws Exception;
    }

    private final PutItemRequest.Builder held = PutItemRequest.builder().tableName("held-old");
    private final DynamoDbClient db = DynamoDbClient.create();

    void retarget() throws Exception {
        Lookup lookup = Held.class::getDeclaredField;
        Field field = lookup.find("held");
        field.setAccessible(true);
        ((PutItemRequest.Builder) field.get(this)).tableName("held-new");
    }

    void heldField() {
        var r = held.build();
        db.putItem(r);
    }
}
