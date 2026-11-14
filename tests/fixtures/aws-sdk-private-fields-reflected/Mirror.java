import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Reads the private field of `Reached` by reflection and hands its builder out.
class Mirror {
    static PutItemRequest.Builder of(Reached target) throws Exception {
        var field = Reached.class.getDeclaredField("reached");
        field.setAccessible(true);
        return (PutItemRequest.Builder) field.get(target);
    }
}
