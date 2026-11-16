package software.amazon.awssdk.services.dynamodb.model;

// A stub of the SDK's request for the oracle: a builder that keeps the last table it was given.
public class PutItemRequest {
    private final String table;

    private PutItemRequest(String table) {
        this.table = table;
    }

    public String tableName() {
        return table;
    }

    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        private String table;

        public Builder tableName(String table) {
            this.table = table;
            return this;
        }

        public PutItemRequest build() {
            return new PutItemRequest(table);
        }
    }
}
