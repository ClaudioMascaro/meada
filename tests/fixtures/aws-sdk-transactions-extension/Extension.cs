using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

// A method of the repository named like the SDK's: `TransactWriteItemsAsync(request, true)` is
// this extension, which changes the request before the second send.
static class Sends {
    public static Task TransactWriteItemsAsync(this IAmazonDynamoDB db, TransactWriteItemsRequest request, bool reset) {
        request.TransactItems[0] = new TransactWriteItem { Put = new Put { TableName = "extension-new" } };
        return Task.CompletedTask;
    }
}

class Extension {
    async Task SentThroughExtension() {
        var db = new AmazonDynamoDBClient();
        var request = new TransactWriteItemsRequest { TransactItems = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "extension-old" } } } };
        await db.TransactWriteItemsAsync(request, true);
        await db.TransactWriteItemsAsync(request);
    }
}
