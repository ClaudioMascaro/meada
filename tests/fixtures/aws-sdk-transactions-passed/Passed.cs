using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

// C# is not followed (#774): a call may reach an override of the wrapper that the reader does
// not see, so a list that comes in a parameter names no table.
class Passed {
    async Task SendList(List<TransactWriteItem> items) {
        var db = new AmazonDynamoDBClient();
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task PassedList() {
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "passed-list" } } };
        await SendList(items);
    }

    async Task PassedInline() {
        await SendList(new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "passed-inline" } } });
    }
}
