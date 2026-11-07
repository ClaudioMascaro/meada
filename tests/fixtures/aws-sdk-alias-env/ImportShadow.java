import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.BatchGetItemRequest;
import software.amazon.awssdk.services.dynamodb.model.KeysAndAttributes;
import vendor.java;
class ImportShadow {

void read() {
var client = DynamoDbClient.create();
client.batchGetItem(BatchGetItemRequest.builder().requestItems(java.util.Map.of("shadowed", KeysAndAttributes.builder().build())).build());
}
}
