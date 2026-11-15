import java.io.StringReader;
import java.util.List;
import java.util.function.Function;
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
    private final PutItemRequest.Builder source = PutItemRequest.builder().tableName("source-field");
    private final PutItemRequest.Builder alias = source;
    private final PutItemRequest.Builder typed = PutItemRequest.builder().tableName("typed-field");
    private final PutItemRequest.Builder caught = PutItemRequest.builder().tableName("caught-field");
    private final PutItemRequest.Builder looped = PutItemRequest.builder().tableName("looped-field");
    private final PutItemRequest.Builder opened = PutItemRequest.builder().tableName("opened-field");
    private final PutItemRequest.Builder matched = PutItemRequest.builder().tableName("matched-field");
    private final PutItemRequest.Builder inferred = PutItemRequest.builder().tableName("inferred-field");
    private final PutItemRequest.Builder spread = PutItemRequest.builder().tableName("spread-field");
    private final PutItemRequest.Builder kept = PutItemRequest.builder().tableName("kept-field");
    private final PutItemRequest.Builder returned = PutItemRequest.builder().tableName("returned-field");
    private final PutItemRequest.Builder renamed = PutItemRequest.builder().tableName("renamed-field");

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

    // The field is another name of a builder: judged as it is where no local has its name.
    void thisAliasBesideLocal() {
        int alias = 1;
        db.putItem(this.alias.build());
    }

    // A step on the local of the field's name counts as one on the field.
    void localStepped() {
        var stepped = PutItemRequest.builder().tableName("stepped-local");
        stepped.tableName("stepped-local-new");
        db.putItem(this.stepped.build());
    }

    // A lambda's parameter that writes its type is a declaration like a method's.
    void thisBesideTypedLambda() {
        Function<String, String> same = (String typed) -> "x";
        db.putItem(this.typed.build());
    }

    // The declarations below are not told from any other mention of the name, so the field has no
    // table beside one: a `catch` parameter, a loop's variable, a `try` resource, a pattern
    // variable, a lambda's parameter with no type, and a varargs parameter.
    void thisBesideCatch() {
        try {
            db.putItem(this.caught.build());
        } catch (RuntimeException caught) {
        }
    }

    void thisBesideLoop(List<String> names) {
        for (String looped : names) {
        }
        db.putItem(this.looped.build());
    }

    void thisBesideResource() {
        try (StringReader opened = new StringReader("")) {
        }
        db.putItem(this.opened.build());
    }

    void thisBesidePattern(Object value) {
        if (value instanceof String matched) {
        }
        db.putItem(this.matched.build());
    }

    void thisBesideUntypedLambda() {
        Function<String, String> same = inferred -> "x";
        db.putItem(this.inferred.build());
    }

    void thisBesideVarargs(String... spread) {
        db.putItem(this.spread.build());
    }

    // The local of the field's name is handed on: the bare name counts as the field's.
    void localHandedOn() {
        var kept = PutItemRequest.builder().tableName("kept-local");
        keep(kept);
        db.putItem(this.kept.build());
    }

    void keep(PutItemRequest.Builder b) {
    }

    // The local of the field's name is returned: a mention the reader does not follow.
    PutItemRequest.Builder localReturned() {
        var returned = PutItemRequest.builder().tableName("returned-local");
        db.putItem(this.returned.build());
        return returned;
    }

    // The local of the field's name is the field itself: the request carries `renamed-new`.
    void fieldUnderItsOwnName() {
        var renamed = this.renamed;
        renamed.tableName("renamed-new");
        db.putItem(this.renamed.build());
    }
}
