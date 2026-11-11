using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;

class Transactions {
    async Task Write() {
        var db = new AmazonDynamoDBClient();
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest {
            TransactItems = new List<TransactWriteItem> {
                new TransactWriteItem { Put = new Put { TableName = "orders" } },
                new TransactWriteItem { Update = new Update { TableName = "totals" } },
                new TransactWriteItem { Delete = new Delete { TableName = "archive" } },
                new TransactWriteItem { ConditionCheck = new ConditionCheck { TableName = "customers" } },
            }
        });
    }

    async Task Read() {
        var db = new AmazonDynamoDBClient();
        await db.TransactGetItemsAsync(new TransactGetItemsRequest {
            TransactItems = new List<TransactGetItem> {
                new TransactGetItem { Get = new Get { TableName = "orders" } },
                new TransactGetItem { Get = new Get { TableName = "customers" } },
            }
        });
    }

    async Task FromEnvironment() {
        var db = new AmazonDynamoDBClient();
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest {
            TransactItems = new List<TransactWriteItem> {
                new TransactWriteItem { Put = new Put { TableName = Environment.GetEnvironmentVariable("TABLE") } },
                new TransactWriteItem { ConditionCheck = new ConditionCheck { TableName = Environment.GetEnvironmentVariable("CHECKED") } },
            }
        });
    }

    async Task Computed() {
        var db = new AmazonDynamoDBClient();
        await db.TransactWriteItemsAsync(new TransactWriteItemsRequest {
            TransactItems = new List<TransactWriteItem> {
                new TransactWriteItem { Put = new Put { TableName = Table() } },
                new TransactWriteItem { ConditionCheck = new ConditionCheck { TableName = Table() } },
            }
        });
    }
}
