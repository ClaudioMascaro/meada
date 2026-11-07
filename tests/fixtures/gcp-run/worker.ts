import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function legacy() { return client.send(new SendMessageCommand({QueueUrl: process.env.QUEUE_URL})); }
export function current() { return client.send(new SendMessageCommand({QueueUrl: process.env.V2_QUEUE})); }
export function batch() { return client.send(new SendMessageCommand({QueueUrl: process.env.JOB_QUEUE})); }
export function secret() { return client.send(new SendMessageCommand({QueueUrl: process.env.SECRET_QUEUE})); }
export function v2secret() { return client.send(new SendMessageCommand({QueueUrl: process.env.V2_SECRET})); }
export function unknown() { return client.send(new SendMessageCommand({QueueUrl: process.env.UNKNOWN_QUEUE})); }
export function sidecar() { return client.send(new SendMessageCommand({QueueUrl: process.env.SIDE_QUEUE})); }
export function wrongKind() { return client.send(new SendMessageCommand({QueueUrl: process.env.BUCKET_AS_QUEUE})); }
export function duplicate() { return client.send(new SendMessageCommand({QueueUrl: process.env.DUP_QUEUE})); }
