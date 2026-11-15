import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// `this.name` is the field, whatever local or parameter of the method has its name. The bare
// name there is not told apart by where it stands, so it reads no field and a step on it counts
// as one on the field.
class Hidden {
    private final DynamoDbClient db = DynamoDbClient.create();
    private final PutItemRequest.Builder beside = PutItemRequest.builder().tableName("beside-field");
    private final PutItemRequest.Builder taken = PutItemRequest.builder().tableName("taken-field");
    private final PutItemRequest.Builder apart = PutItemRequest.builder().tableName("apart-field");
    private final PutItemRequest.Builder bare = PutItemRequest.builder().tableName("bare-field");
    private final PutItemRequest.Builder given = PutItemRequest.builder().tableName("given-field");
    private final PutItemRequest.Builder early = PutItemRequest.builder().tableName("early-field");
    private final PutItemRequest.Builder stepped = PutItemRequest.builder().tableName("stepped-field");

    void thisBesideLocal() {
        var beside = PutItemRequest.builder().tableName("beside-local");
        db.putItem(this.beside.build());
    }

    void thisBesideParameter(String taken) {
        db.putItem(this.taken.build());
    }

    // The local is another method's.
    void localApart() {
        int apart = 1;
    }

    void thisApart() {
        db.putItem(this.apart.build());
    }

    // The bare name is the local here: the request carries `bare-local`, not the field's table.
    void bareBesideLocal() {
        var bare = PutItemRequest.builder().tableName("bare-local");
        db.putItem(bare.build());
    }

    // The bare name is the parameter: the request carries what the caller passes.
    void bareBesideParameter(PutItemRequest.Builder given) {
        db.putItem(given.build());
    }

    // Before the local is declared the bare name is the field: the request carries `early-new`.
    void setBeforeLocal() {
        early.tableName("early-new");
        var early = PutItemRequest.builder().tableName("early-local");
        db.putItem(this.early.build());
    }

    // A step on the local of the field's name counts as one on the field.
    void localStepped() {
        var stepped = PutItemRequest.builder().tableName("stepped-local");
        stepped.tableName("stepped-local-new");
        db.putItem(this.stepped.build());
    }
}
