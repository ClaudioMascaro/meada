import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

class Mirror {
    void set(Reached target) throws Exception {
        var field = Reached.class.getDeclaredField("reached");
        field.setAccessible(true);
        var reached = (PutItemRequest.Builder) field.get(target);
        reached.tableName("reached-new");
    }
}
