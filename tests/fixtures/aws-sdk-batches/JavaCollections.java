import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.BatchGetItemRequest;
import software.amazon.awssdk.services.dynamodb.model.KeysAndAttributes;
import software.amazon.awssdk.services.ssm.SsmClient;
import software.amazon.awssdk.services.ssm.model.GetParametersRequest;

class JavaCollections {
    void read() {
        var db = DynamoDbClient.create();
        db.batchGetItem(BatchGetItemRequest.builder().requestItems(java.util.Map.of(
            "orders", KeysAndAttributes.builder().build(),
            "customers", KeysAndAttributes.builder().build()
        )).build());
        var parameters = SsmClient.create();
        parameters.getParameters(GetParametersRequest.builder().names(java.util.List.of(
            "/java/one", System.getenv("JAVA_PARAMETER")
        )).build());
    }
}
