import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function dispatch() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.QUEUE_NAME, MessageBody: 'hello'}));
}

export function typedDispatch() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.TYPED_QUEUE, MessageBody: 'hello'}));
}
