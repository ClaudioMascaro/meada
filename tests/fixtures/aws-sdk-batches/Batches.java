import software.amazon.awssdk.services.eventbridge.EventBridgeClient;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequest;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequestEntry;

class Batches {
    void send() {
        var events = EventBridgeClient.create();
        events.putEvents(PutEventsRequest.builder().entries(
            PutEventsRequestEntry.builder().eventBusName("orders").build(),
            PutEventsRequestEntry.builder().eventBusName(System.getenv("BUS")).build()
        ).build());
    }
}
