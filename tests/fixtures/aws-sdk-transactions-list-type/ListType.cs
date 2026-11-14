using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

// The repository's own `List`: its `Count` is the repository's code, which may change the list.
class List<T> {
    public T First;
    public int Count { get { First = default; return 1; } }
}

class ListType {
    async Task OwnList() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "own-list" } } };
        System.Console.WriteLine(items.Count);
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }

    async Task OwnListPlain() {
        var db = new AmazonDynamoDBClient();
        var items = new List<TransactWriteItem> { new TransactWriteItem { Put = new Put { TableName = "own-list-plain" } } };
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest { TransactItems = items });
    }
}
