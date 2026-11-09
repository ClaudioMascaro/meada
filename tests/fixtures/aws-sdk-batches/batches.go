package application

import (
    "context"
    "github.com/aws/aws-sdk-go-v2/service/dynamodb"
    "github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

func writeTables() {
    db := dynamodb.NewFromConfig(cfg)
    db.BatchWriteItem(context.TODO(), &dynamodb.BatchWriteItemInput{
        RequestItems: map[string][]types.WriteRequest{
            "orders": {},
            "customers": {},
        },
    })
}
