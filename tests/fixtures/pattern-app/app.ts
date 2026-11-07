import { SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";
const client = new SQSClient({});
export async function send() {
  await client.send(new SendMessageCommand({ QueueUrl: "https://sqs.us-east-1.amazonaws.com/123456789012/prod-orders", MessageBody: "x" }));
}
