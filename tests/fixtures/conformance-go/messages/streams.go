package messages

import (
	"context"

	"github.com/redis/go-redis/v9"
)

func RedisProduce(ctx context.Context) {
	rdb := redis.NewClient(&redis.Options{Addr: "redis:6379"})
	rdb.XAdd(ctx, &redis.XAddArgs{Stream: "orders", Values: map[string]any{"id": 1}})
}

func RedisConsume(ctx context.Context, rdb *redis.Client) {
	rdb.XReadGroup(ctx, &redis.XReadGroupArgs{Group: "api", Consumer: "a", Streams: []string{"orders", "payments", ">", ">"}})
	// Without a consumer group a read is not a consumer.
	rdb.XRead(ctx, &redis.XReadArgs{Streams: []string{"ignored", "0"}})
}
