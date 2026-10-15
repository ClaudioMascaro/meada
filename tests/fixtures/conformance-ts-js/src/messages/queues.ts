import { onPayment } from "./handlers";
import { channel, confirming } from "./rabbit";

const TASKS = "tasks";

// `sendToQueue` produces to the queue it names.
export async function enqueue() {
  channel.sendToQueue(TASKS, Buffer.from("{}"));
  confirming.sendToQueue("invoices", Buffer.from("{}"));
}

// A queue wrapper: each call site of `queue` produces to its own queue.
function queue(name: string) {
  return channel.sendToQueue(name, Buffer.from("{}"));
}

export function enqueueReports() {
  return queue("reports");
}

// `consume` with a function written in place, and with an imported one.
export async function work() {
  await channel.consume(TASKS, async (message) => {
    record(message);
  });
  await confirming.consume("invoices", onPayment);
}

// A handler that is no function Meada can follow: the function that consumes handles.
export async function workReports(handler: (m: unknown) => void) {
  await channel.consume("reports", handler);
}

function record(value: unknown) {
  return value;
}
