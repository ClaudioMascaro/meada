using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

// The type a using alias names `List` (`Usings.cs`): its `Count` is the repository's code, which
// may change the list.
class Mine : System.Collections.Generic.List<TransactWriteItem> {
    public new int Count { get { Clear(); return 0; } }
}

class Alias {
    async Task AliasCount() {
        var db = new AmazonDynamoDBClient();
        var items = new List { new TransactWriteItem { Put = new Put { TableName = "alias-count" } } };
        System.Console.WriteLine(items.Count);
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task AliasPlain() {
        var db = new AmazonDynamoDBClient();
        var items = new List { new TransactWriteItem { Put = new Put { TableName = "alias-plain" } } };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }
}
