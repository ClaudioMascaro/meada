import { ReceiveMessageCommand, SQSClient } from "@aws-sdk/client-sqs";
import { Consumer } from "sqs-consumer";

const client = new SQSClient({});

// The enclosing function of the receive call handles the messages.
export async function drain() {
  const out = await client.send(
    new ReceiveMessageCommand({
      QueueUrl: `${process.env.SQS_BASE}/orders`,
    }),
  );
  return out.Messages;
}

export function startRefunds() {
  return Consumer.create({
    queueUrl: "https://sqs.eu-west-1.amazonaws.com/123456789012/refunds",
    handleMessage: handleRefund,
  });
}

export async function handleRefund(message: unknown) {
  return message;
}
