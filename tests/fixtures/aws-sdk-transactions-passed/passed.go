package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A wrapper that mentions its parameter twice, where it declares it and where the request reads
// it, sends what each caller passes. The caller's request, list or entry is followed there, by
// the caller's own mentions (#774).

func sendList(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func sendRequest(ctx context.Context, in *dynamodb.TransactWriteItemsInput) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, in)
}

func sendEntry(ctx context.Context, p *types.Put) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: p}}})
}

func sendGets(ctx context.Context, items []types.TransactGetItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactGetItems(ctx, &dynamodb.TransactGetItemsInput{TransactItems: items})
}

func relay(ctx context.Context, items []types.TransactWriteItem) {
	sendList(ctx, items)
}

type store struct{}

func (s *store) send(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func passedList(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-list")}}}
	sendList(ctx, items)
}

func passedInline(ctx context.Context) {
	sendList(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-inline")}}})
}

func passedRequest(ctx context.Context) {
	sendRequest(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-request")}}}})
}

func passedEntry(ctx context.Context) {
	sendEntry(ctx, &types.Put{TableName: aws.String("passed-entry")})
}

func passedGets(ctx context.Context) {
	sendGets(ctx, []types.TransactGetItem{{Get: &types.Get{TableName: aws.String("passed-gets")}}})
}

func passedTwice(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-twice")}}}
	relay(ctx, items)
}

func passedMethod(ctx context.Context) {
	s := &store{}
	s.send(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-method")}}})
}

// The twins: a third mention of the parameter in the wrapper, or of the list in the caller.

func sendChanged(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	items[0].Put.TableName = aws.String("wrapper-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func changedByWrapper(ctx context.Context) {
	sendChanged(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("wrapper-old")}}})
}

func sendHanded(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	swap(items)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func swap(items []types.TransactWriteItem) {
	items[0] = types.TransactWriteItem{Put: &types.Put{TableName: aws.String("handed-new")}}
}

func handedByWrapper(ctx context.Context) {
	sendHanded(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("handed-old")}}})
}

func sendReset(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	items = []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("reset-new")}}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func resetByWrapper(ctx context.Context) {
	sendReset(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("reset-old")}}})
}

func changedByCaller(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("caller-old")}}}
	items[0].Put.TableName = aws.String("caller-new")
	sendList(ctx, items)
}

func changedRequest(ctx context.Context) {
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("request-old")}}}}
	in.TransactItems[0].Put.TableName = aws.String("request-new")
	sendRequest(ctx, in)
}

func changedEntry(ctx context.Context) {
	p := &types.Put{TableName: aws.String("entry-old")}
	p.TableName = aws.String("entry-new")
	sendEntry(ctx, p)
}

func changedGets(ctx context.Context) {
	items := []types.TransactGetItem{{Get: &types.Get{TableName: aws.String("gets-old")}}}
	items[0].Get.TableName = aws.String("gets-new")
	sendGets(ctx, items)
}

func changedMethod(ctx context.Context) {
	s := &store{}
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("method-old")}}}
	items[0].Put.TableName = aws.String("method-new")
	s.send(ctx, items)
}

// The wrapper in the middle of two writes its parameter.

func relayChanged(ctx context.Context, items []types.TransactWriteItem) {
	items[0].Put.TableName = aws.String("relay-new")
	sendList(ctx, items)
}

func changedInRelay(ctx context.Context) {
	relayChanged(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("relay-old")}}})
}

// A variadic parameter. A spread passes the caller's slice itself, and one element is the whole
// list: both are followed. Two elements are more arguments than parameters: not followed.

func sendAll(ctx context.Context, items ...types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func passedSlice(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-slice")}}}
	sendAll(ctx, items...)
}

func passedOne(ctx context.Context) {
	sendAll(ctx, types.TransactWriteItem{Put: &types.Put{TableName: aws.String("passed-one")}})
}

func changedOne(ctx context.Context) {
	p := &types.Put{TableName: aws.String("one-old")}
	p.TableName = aws.String("one-new")
	sendAll(ctx, types.TransactWriteItem{Put: p})
}

func passedTwo(ctx context.Context) {
	sendAll(ctx, types.TransactWriteItem{Put: &types.Put{TableName: aws.String("two-a")}}, types.TransactWriteItem{Put: &types.Put{TableName: aws.String("two-b")}})
}

// A method whose name another definition spells is not followed.

type first struct{}

type second struct{}

func (f *first) push(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func (s *second) push(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	items[0].Put.TableName = aws.String("second-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func passedShared(ctx context.Context) {
	f := &first{}
	f.push(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("shared-old")}}})
}

func otherShared(ctx context.Context) {
	s := &second{}
	s.push(ctx, []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("second-old")}}})
}

// `len(items)` reads the list and changes nothing: the wrapper still mentions its parameter twice
// (#774, the reads of #869 in a wrapper of #879).
func sendCounted(ctx context.Context, items []types.TransactWriteItem) {
	if len(items) == 0 {
		return
	}
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func passedCounted(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("passed-counted")}}}
	sendCounted(ctx, items)
}
