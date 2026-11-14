using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;
using Amazon.EventBridge;
using Amazon.EventBridge.Model;

// Unlike Go, C# keeps the rule that a property written anywhere leaves the first name on a
// transaction's path unresolved (#774): a class derived from an SDK type can keep `this`
// (`TrackedPut` in Other.cs), so a write through another name may reach a local's value. The
// writes are in `Record`, `PutWritten`, `TrackedPut`, the twins and the class `Other`.
class Fields {
    async Task HeldEntry() {
        var db = new AmazonDynamoDBClient();
        var put = new Put { TableName = "held-entry" };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = put } } });
    }

    async Task HeldList() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "held-list" } } };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task HeldRequest() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "held-request" } } } };
        await db.TransactWriteItemsAsync(request);
    }

    async Task HeldGet() {
        var db = new AmazonDynamoDBClient();
        var held = new Get { TableName = "held-get" };
        await db.TransactGetItemsAsync(new TransactGetItemsRequest { TransactItems = new List<TransactGetItem> { new TransactGetItem { Get = held } } });
    }

    async Task HeldBus() {
        var events = new AmazonEventBridgeClient();
        var entries = new List<PutEventsRequestEntry> { new PutEventsRequestEntry { EventBusName = "held-bus" } };
        await events.PutEventsAsync(new PutEventsRequest { Entries = entries });
    }

    // The entry is a class of the repository that keeps each instance: `Retarget` writes it
    // through that list, with no third mention of `put`.
    async Task TrackedEntry() {
        var db = new AmazonDynamoDBClient();
        var put = new TrackedPut { TableName = "tracked-old" };
        TrackedPut.Retarget();
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = put } } });
    }

    // The twins: the same values written in place, a third mention of their names.

    async Task WrittenEntry() {
        var db = new AmazonDynamoDBClient();
        var put = new Put { TableName = "written-entry-old" };
        put.TableName = "written-entry-new";
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = put } } });
    }

    async Task WrittenList() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "written-list-old" } } };
        items[0].Put = new Put { TableName = "written-list-new" };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task WrittenRequest() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "written-request-old" } } } };
        request.TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "written-request-new" } } };
        await db.TransactWriteItemsAsync(request);
    }

    // The entry written through a second local: the alias is a third mention of `put`.
    async Task AliasedEntry() {
        var db = new AmazonDynamoDBClient();
        var put = new Put { TableName = "aliased-entry-old" };
        var alias = put;
        alias.TableName = "aliased-entry-new";
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = put } } });
    }

    async Task WrittenGet() {
        var db = new AmazonDynamoDBClient();
        var held = new Get { TableName = "written-get-old" };
        held.TableName = "written-get-new";
        await db.TransactGetItemsAsync(new TransactGetItemsRequest { TransactItems = new List<TransactGetItem> { new TransactGetItem { Get = held } } });
    }

    async Task WrittenBus() {
        var events = new AmazonEventBridgeClient();
        var entries = new List<PutEventsRequestEntry> { new PutEventsRequestEntry { EventBusName = "written-bus-old" } };
        entries[0].EventBusName = "written-bus-new";
        await events.PutEventsAsync(new PutEventsRequest { Entries = entries });
    }

    // A request that is no list of entries keeps the rule it had: a `TableName` written anywhere
    // leaves a named one unresolved.

    async Task PlainPut() {
        var db = new AmazonDynamoDBClient();
        var request = new PutItemRequest { TableName = "plain-put" };
        await db.PutItemAsync(request);
    }

    async Task PutWritten() {
        var db = new AmazonDynamoDBClient();
        var request = new PutItemRequest { TableName = "put-old" };
        request.TableName = "put-new";
        await db.PutItemAsync(request);
    }
}
