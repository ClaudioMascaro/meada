import java.lang.reflect.Field;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Reads the private static field `kept` of `Fields` by reflection and hands its builder out.
class Mirror {
    static PutItemRequest.Builder kept() throws Exception {
        Field field = Fields.class.getDeclaredField("kept");
        field.setAccessible(true);
        return (PutItemRequest.Builder) field.get(null);
    }
}
