import * as aws from "@aws-sdk/client-sqs";
import { ReceiveMessageCommand, SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";
import { Consumer } from "sqs-consumer";
import { SendMessageCommand as Fake } from "not-aws";
import { Consumer as FakeConsumer } from "not-sqs-consumer";

const client = new SQSClient({});
const REGION_URL = `https://sqs.${process.env.AWS_REGION}.amazonaws.com/${process.env.AWS_ACCOUNT}`;
const JOBS = "https://sqs.eu-west-1.amazonaws.com/123456789012/jobs";

// A dynamic prefix: only the queue name, the last segment, is read.
export async function sendJob() {
  await client.send(new SendMessageCommand({ QueueUrl: JOBS, MessageBody: "{}" }));
  await client.send(new SendMessageCommand({ QueueUrl: `${REGION_URL}/mail`, MessageBody: "{}" }));
}

// A namespace import reaches the same command.
export async function sendThroughNamespace() {
  await client.send(new aws.SendMessageCommand({ QueueUrl: `${REGION_URL}/audit` }));
}

// A client wrapper: each call site of `enqueue` produces, with its URL.
function enqueue(queueUrl: string) {
  return client.send(new SendMessageCommand({ QueueUrl: queueUrl, MessageBody: "{}" }));
}

export function enqueueBilling() {
  return enqueue(`${REGION_URL}/billing`);
}

// The enclosing function of the receive call handles the messages.
export async function receiveJobs() {
  return client.send(new ReceiveMessageCommand({ QueueUrl: JOBS }));
}

export async function receiveBilling() {
  return client.send(new ReceiveMessageCommand({ QueueUrl: `${REGION_URL}/billing` }));
}

// `sqs-consumer`: `handleMessage` written in place, and an imported function.
export function consumeMail() {
  return Consumer.create({
    queueUrl: `${REGION_URL}/mail`,
    handleMessage: async (message) => {
      record(message);
    },
  });
}

export function consumeAudit() {
  return Consumer.create({ queueUrl: `${REGION_URL}/audit`, handleMessage: onAudit });
}

function onAudit(message: unknown) {
  return message;
}

// Not channels: a command and a consumer of other packages.
export function notQueued() {
  new Fake({ QueueUrl: `${REGION_URL}/ignored` });
  FakeConsumer.create({ queueUrl: `${REGION_URL}/ignored`, handleMessage: onAudit });
}

function record(value: unknown) {
  return value;
}
