import { SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";

const client = new SQSClient({});
const BASE = process.env.SQS_BASE;

// The account and the region of the URL are dynamic; only the queue name is read.
export async function enqueueOrder(body: string) {
  await client.send(
    new SendMessageCommand({ QueueUrl: `${BASE}/orders`, MessageBody: body }),
  );
}

// A producer in a client wrapper, a literal URL at its call site.
function enqueue(queueUrl: string, body: string) {
  return client.send(new SendMessageCommand({ QueueUrl: queueUrl, MessageBody: body }));
}

export function enqueueRefund() {
  return enqueue("https://sqs.eu-west-1.amazonaws.com/123456789012/refunds", "{}");
}
