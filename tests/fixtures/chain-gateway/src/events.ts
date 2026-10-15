import { producer } from "./kafka";

// The client wrapper around the Kafka producer: its topic comes from its parameter.
export function publish(topic: string, value: string) {
  return producer.send({ topic, messages: [{ value }] });
}
