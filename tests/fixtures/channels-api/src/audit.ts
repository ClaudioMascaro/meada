import { consumer } from "./kafka";

export async function auditLog() {
  await consumer.subscribe({ topic: "audit.log" });
  await consumer.run({
    async eachBatch({ batch }) {
      store(batch.messages.length);
    },
  });
}

function store(count: number) {
  return count;
}
