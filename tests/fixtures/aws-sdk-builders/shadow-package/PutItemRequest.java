package app;

// A type of the package named like the SDK's: the wildcard import loses to it.
class PutItemRequest {
    static Builder builder() { return new Builder(); }

    static class Builder {
        Builder tableName(String name) { return this; }
        software.amazon.awssdk.services.dynamodb.model.PutItemRequest build() { return null; }
    }
}
