import Redis from "ioredis";

const redis = new Redis();

export async function appendOrder(id: string) {
  await redis.xadd("orders", "*", "id", id);
}

export async function appendRefund(id: string) {
  await redis.xadd("refunds", "*", "id", id);
}
