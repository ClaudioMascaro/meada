import { producer } from "./kafka";

// The topic comes from the environment: Meada cannot know it, so this side is never linked.
export async function replay(id: string) {
  await producer.send({ topic: process.env.REPLAY_TOPIC, messages: [{ value: id }] });
}

// Built from a value only the running program has.
export async function announce(id: string) {
  await producer.send({ topic: `events.${currentRegion()}`, messages: [{ value: id }] });
}

function currentRegion() {
  return "eu";
}
