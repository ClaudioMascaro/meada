using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

// A request, a list, an item or an entry held in a name changes in place. Its tables are exact
// only when the method writes the name twice: where it is declared and where the request reads
// it. Any other mention is a change the reader cannot follow.
class Named {
    async Task PlainRequest() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "plain-request" } } } };
        await db.TransactWriteItemsAsync(request);
    }

    async Task PlainParts() {
        var db = new AmazonDynamoDBClient();
        var put = new Put { TableName = "plain-parts" };
        var item = new TransactWriteItem { Put = put };
        var items = new List<TransactWriteItem> { item };
        var request = new TransactWriteItemsRequest { TransactItems = items };
        await db.TransactWriteItemsAsync(request);
    }

    async Task PlainGet() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactGetItemsRequest { TransactItems = new List<TransactGetItem> { new TransactGetItem { Get = new Get { TableName = "plain-get" } } } };
        await db.TransactGetItemsAsync(request);
    }

    async Task ThroughRequest() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "through-old" } } } };
        request.TransactItems[0].Put.TableName = "through-new";
        await db.TransactWriteItemsAsync(request);
    }

    async Task ElementReplaced() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "element-old" } } } };
        request.TransactItems[0] = new TransactWriteItem { Put = new Put { TableName = "element-new" } };
        await db.TransactWriteItemsAsync(request);
    }

    async Task RequestCleared() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "cleared-old" } } } };
        request.TransactItems.Clear();
        request.TransactItems.Add(new TransactWriteItem { Put = new Put { TableName = "cleared-new" } });
        await db.TransactWriteItemsAsync(request);
    }

    async Task ListWritten() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "list-old" } } };
        var request = new TransactWriteItemsRequest { TransactItems = items };
        items[0].Put.TableName = "list-new";
        await db.TransactWriteItemsAsync(request);
    }

    async Task ListReplaced() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "replaced-old" } } };
        items[0] = new TransactWriteItem { Put = new Put { TableName = "replaced-new" } };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task ListRemoved() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "removed-old" } }, new TransactWriteItem { Put = new Put { TableName = "removed-kept" } } };
        items.RemoveAt(0);
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task ListHanded() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "handed-old" } } };
        Swap(items);
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    void Swap(List<TransactWriteItem> items) {
        items[0] = new TransactWriteItem { Put = new Put { TableName = "handed-new" } };
    }

    async Task HeldEntry() {
        var db = new AmazonDynamoDBClient();
        var put = new Put { TableName = "held-old" };
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = put } } };
        put.TableName = "held-new";
        await db.TransactWriteItemsAsync(request);
    }

    async Task ItemWritten() {
        var db = new AmazonDynamoDBClient();
        var item = new TransactWriteItem { Put = new Put { TableName = "item-old" } };
        item.Put.TableName = "item-new";
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { item } });
    }

    async Task SharedItem() {
        var db = new AmazonDynamoDBClient();
        var item = new TransactWriteItem { Put = new Put { TableName = "shared-old" } };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { item } });
        item.Put.TableName = "shared-new";
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { item } });
    }

    async Task GetReplaced() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactGetItemsRequest { TransactItems = new List<TransactGetItem> { new TransactGetItem { Get = new Get { TableName = "get-old" } } } };
        request.TransactItems[0] = new TransactGetItem { Get = new Get { TableName = "get-new" } };
        await db.TransactGetItemsAsync(request);
    }
}
