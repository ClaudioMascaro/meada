import Redis from "ioredis";

const redis = new Redis();

// Every stream named after STREAMS is consumed, the entries' ids after them are not; the
// enclosing function handles the entries.
export async function readOrders() {
  return redis.xreadgroup(
    "GROUP", "billing", "worker-1",
    "COUNT", 10,
    "STREAMS", "orders", "refunds", ">", ">",
  );
}
