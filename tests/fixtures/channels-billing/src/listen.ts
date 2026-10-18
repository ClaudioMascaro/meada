import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "billing", brokers: ["kafka:9092"] });
const consumer = kafka.consumer({ groupId: "billing" });

// One handler for two topics, written in place.
export async function listen() {
  await consumer.subscribe({ topics: ["orders.created", "billing.invoices"] });
  await consumer.run({
    eachMessage: async ({ topic }) => {
      charge(topic);
    },
  });
}

function charge(topic: string) {
  return topic;
}
