import IORedis, { Redis } from "ioredis";
import * as noderedis from "redis";
import { createClient } from "redis";
import { createClient as fakeClient } from "not-redis";

const ioredis = new IORedis();
const named = new Redis({ host: "redis" });
const client = createClient();
const other = noderedis.createClient();
const fake = fakeClient();
const PREFIX = "events";

// ioredis, through the default and the named export; a stream in a template literal.
export async function appendIoredis() {
  await ioredis.xadd("orders", "*", "id", "1");
  await named.xadd(`${PREFIX}.audit`, "*", "id", "1");
}

// node-redis, through a named and a namespace import.
export async function appendNodeRedis() {
  await client.xAdd("shipments", "*", { id: "1" });
  await other.xAdd("returns", "*", { id: "1" });
}

// A client wrapper: each call site of `append` produces, with its stream.
function append(stream: string) {
  return ioredis.xadd(stream, "*", "id", "1");
}

export function appendBilling() {
  append("billing");
}

// A stream Meada cannot evaluate is an unresolved side, never linked.
export async function appendDynamic() {
  await ioredis.xadd(process.env.STREAM, "*", "id", "1");
}

// Every stream named after STREAMS is consumed; the enclosing function handles the entries.
export async function readIoredis() {
  return ioredis.xreadgroup(
    "GROUP", "workers", "w1",
    "COUNT", 10,
    "STREAMS", "orders", `${PREFIX}.audit`, ">", ">",
  );
}

export async function readNamed() {
  return named.xreadgroup("GROUP", "workers", "w1", "streams", "billing", ">");
}

// node-redis: one key, and a list of keys.
export async function readNodeRedis() {
  return client.xReadGroup("workers", "w1", { key: "shipments", id: ">" });
}

export async function readMany() {
  return other.xReadGroup("workers", "w1", [
    { key: "returns", id: ">" },
    { key: "orders", id: ">" },
  ]);
}

// A client wrapper on the consuming side.
function read(stream: string) {
  return client.xReadGroup("workers", "w1", { key: stream, id: ">" });
}

export function readBilling() {
  return read("billing");
}

// Not channels: a client of another package.
export async function notRedis() {
  await fake.xadd("orders", "*", "id", "1");
  await fake.xReadGroup("workers", "w1", { key: "orders", id: ">" });
}
