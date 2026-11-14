using System.Collections.Generic;
using Amazon.DynamoDBv2.Model;
using Amazon.EventBridge.Model;

// Writes through a parameter, in another class: they reach a value a caller hands over, never
// one a method holds in a local it passes to nobody.
static class Other {
    static void Rename(Put q) { q.TableName = "other"; }
    static void Replace(TransactWriteItem item) { item.Put = new Put { TableName = "other-item" }; }
    static void Refill(TransactWriteItemsRequest request) { request.TransactItems = new List<TransactWriteItem>(); }
    static void Reread(TransactGetItem item) { item.Get = new Get { TableName = "other-get" }; }
    static void Reroute(PutEventsRequestEntry entry) { entry.EventBusName = "other-bus"; }
    static void Relist(PutEventsRequest request) { request.Entries = new List<PutEventsRequestEntry>(); }
}

// A class of the repository's own with the SDK's property names.
class Record {
    public string TableName;
    public Put Put;
    public Get Get;
    public List<TransactWriteItem> TransactItems;
    public string EventBusName;
    public List<PutEventsRequestEntry> Entries;

    static void Clear(Record r) {
        r.TableName = "record";
        r.Put = null;
        r.Get = null;
        r.TransactItems = null;
        r.EventBusName = "record-bus";
        r.Entries = null;
    }
}
