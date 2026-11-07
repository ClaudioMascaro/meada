import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function task() { return client.send(new SendMessageCommand({QueueUrl: process.env.TASK_QUEUE, MessageBody: 'x'})); }
export function literal() { return client.send(new SendMessageCommand({QueueUrl: process.env.LITERAL_QUEUE, MessageBody: 'x'})); }
export function unknown() { return client.send(new SendMessageCommand({QueueUrl: process.env.UNKNOWN_QUEUE, MessageBody: 'x'})); }
export function secret() { return client.send(new SendMessageCommand({QueueUrl: process.env.SECRET_QUEUE, MessageBody: 'x'})); }
