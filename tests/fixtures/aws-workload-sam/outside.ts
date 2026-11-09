import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function outside() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.QUEUE_NAME, MessageBody: 'hello'}));
}
