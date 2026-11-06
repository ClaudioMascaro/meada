import { SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";

const client = new SQSClient({});

export async function sendOrders() {
  await client.send(new SendMessageCommand({ QueueUrl: "https://sqs.eu-west-1.amazonaws.com/123456789012/orders", MessageBody: "{}" }));
}

export async function sendUnmanaged() {
  await client.send(new SendMessageCommand({ QueueUrl: "https://sqs.eu-west-1.amazonaws.com/123456789012/unmanaged", MessageBody: "{}" }));
}
