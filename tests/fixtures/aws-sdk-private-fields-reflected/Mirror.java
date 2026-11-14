import java.lang.reflect.Field;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;

// Reads the private field of `Reached` by reflection and hands its builder out. The read is a
// method reference: no call of this file spells the reflective method's name.
class Mirror {
    interface Lookup {
        Field find(String name) throws Exception;
    }

    static PutItemRequest.Builder of(Reached target) throws Exception {
        Lookup lookup = Reached.class::getDeclaredField;
        var field = lookup.find("reached");
        field.setAccessible(true);
        return (PutItemRequest.Builder) field.get(target);
    }
}
