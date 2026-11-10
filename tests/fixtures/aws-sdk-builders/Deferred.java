import java.util.function.Consumer;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Code that runs when its caller says, a name assigned again, a field hidden by a local, and a
// field set while the object is made. Each moves a table the reader would otherwise take from the
// builder's first write; the ones that cannot are left to keep it.
class Deferred {
    private final DynamoDbClient db = DynamoDbClient.create();
    private Runnable keep;

    // A lambda or a class builds from the builder when it runs, after the builder is set again.
    void lambdaRun() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("lambda-old");
        Runnable run = () -> {
            var r = b.build();
            db.putItem(r);
        };
        b.tableName("lambda-new");
        run.run();
    }

    void lambdaCond(boolean flag) {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("cond-old");
        Runnable run = () -> {
            var r = b.build();
            db.putItem(r);
        };
        if (flag) b.tableName("cond-new");
        run.run();
    }

    void lambdaStored() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("stored-old");
        Runnable run = () -> {
            var r = b.build();
            db.putItem(r);
        };
        keep = run;
        b.tableName("stored-new");
    }

    void lambdaConsumer() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("consumer-old");
        Consumer<String> run = s -> {
            var r = b.build();
            db.putItem(r);
        };
        b.applyMutation(x -> x.tableName("consumer-new"));
        run.accept("go");
    }

    void localClass() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("class-old");
        class Later {
            void run() {
                var r = b.build();
                db.putItem(r);
            }
        }
        b.tableName("class-new");
        new Later().run();
    }

    void anonymousClass() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("anonymous-old");
        Runnable run = new Runnable() {
            public void run() {
                var r = b.build();
                db.putItem(r);
            }
        };
        b.tableName("anonymous-new");
        run.run();
    }

    void nestedLambda() {
        Runnable outer = () -> {
            PutItemRequest.Builder b = PutItemRequest.builder().tableName("nested-old");
            Runnable inner = () -> {
                var r = b.build();
                db.putItem(r);
            };
            b.tableName("nested-new");
            inner.run();
        };
        outer.run();
    }

    // A builder made inside the lambda is built and set there, in the order of its statements.
    void lambdaOwn() {
        Runnable run = () -> {
            PutItemRequest.Builder b = PutItemRequest.builder().tableName("lambda-own");
            var r = b.build();
            b.tableName("lambda-own-later");
            db.putItem(r);
        };
        run.run();
    }

    // A parameter assigned in the helper is not what the caller passed.
    private void copied(PutItemRequest r) {
        r = r.copy(x -> x.tableName("param-copy-new"));
        db.putItem(r);
    }

    void callCopied() {
        copied(PutItemRequest.builder().tableName("param-copy-old").build());
    }

    private void rebuilt(PutItemRequest r) {
        r = r.toBuilder().tableName("param-builder-new").build();
        db.putItem(r);
    }

    void callRebuilt() {
        rebuilt(PutItemRequest.builder().tableName("param-builder-old").build());
    }

    private void replaced(PutItemRequest r) {
        r = PutItemRequest.builder().tableName("param-new").build();
        db.putItem(r);
    }

    void callReplaced() {
        replaced(PutItemRequest.builder().tableName("param-old").build());
    }

    private void maybe(PutItemRequest r, boolean flag) {
        if (flag) r = r.copy(x -> x.tableName("param-maybe-new"));
        db.putItem(r);
    }

    void callMaybe(boolean flag) {
        maybe(PutItemRequest.builder().tableName("param-maybe-old").build(), flag);
    }

    private void passed(PutItemRequest r) {
        db.putItem(r);
    }

    void callPassed() {
        passed(PutItemRequest.builder().tableName("param-plain").build());
    }

    // The request a helper reads is what its caller passes, changed or not before it is passed.
    void passedSet() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("passed-old");
        b.tableName("passed-new");
        var r = b.build();
        passed(r);
    }

    void passedInline() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("inline-old");
        b.tableName("inline-new");
        passed(b.build());
    }

    void passedBuiltFirst() {
        PutItemRequest.Builder b = PutItemRequest.builder().tableName("built-first");
        var r = b.build();
        b.tableName("built-first-later");
        passed(r);
    }

    // A local or a parameter named like a field hides it from the bare name, not from `this`.
    private final PutItemRequest.Builder hidden = PutItemRequest.builder().tableName("hidden-old");

    void hiddenAlias() {
        var hidden = PutItemRequest.builder().tableName("hidden-local");
        this.hidden.tableName("hidden-alias");
        var r = this.hidden.build();
        db.putItem(r);
    }

    void hiddenParameter(PutItemRequest.Builder hidden) {
        this.hidden.tableName("hidden-parameter");
        var r = this.hidden.build();
        db.putItem(r);
    }

    void hiddenNull() {
        PutItemRequest.Builder hidden = null;
        this.hidden.tableName("hidden-null");
        var r = this.hidden.build();
        db.putItem(r);
    }

    void hiddenThroughLocal() {
        var hidden = this.hidden;
        hidden.tableName("hidden-through");
        var r = this.hidden.build();
        db.putItem(r);
    }

    // A field built while the object is made, from a builder set before it, in an initializer.
    private final PutItemRequest.Builder byField = PutItemRequest.builder().tableName("field-old");
    private final PutItemRequest.Builder byFieldAlias = byField.tableName("field-new");
    private final PutItemRequest byFieldBuilt = byField.build();

    void fieldInitializer() {
        db.putItem(byFieldBuilt);
    }

    private final PutItemRequest.Builder byBlock = PutItemRequest.builder().tableName("block-old");
    {
        byBlock.tableName("block-new");
    }
    private final PutItemRequest byBlockBuilt = byBlock.build();

    void initializerBlock() {
        db.putItem(byBlockBuilt);
    }

    private final PutItemRequest.Builder byApply = PutItemRequest.builder().tableName("apply-old");
    private final Object byApplySet = byApply.applyMutation(x -> x.tableName("apply-new"));
    private final PutItemRequest byApplyBuilt = byApply.build();

    void initializerApply() {
        db.putItem(byApplyBuilt);
    }

    private final PutItemRequest.Builder byConstructor = PutItemRequest.builder().tableName("constructor-old");
    private final PutItemRequest byConstructorBuilt;

    Deferred() {
        byConstructor.tableName("constructor-new");
        byConstructorBuilt = byConstructor.build();
    }

    void constructorBuilt() {
        db.putItem(byConstructorBuilt);
    }

    // A field built from a builder nothing sets again keeps its table.
    private final PutItemRequest.Builder untouched = PutItemRequest.builder().tableName("field-plain");
    private final PutItemRequest untouchedBuilt = untouched.build();

    void fieldPlain() {
        db.putItem(untouchedBuilt);
    }
}
