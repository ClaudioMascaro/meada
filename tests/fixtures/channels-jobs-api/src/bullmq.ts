import { Queue } from "bullmq";

const QUEUE = "emails";
const emails = new Queue(QUEUE);
const reports = new Queue("reports");
const seen = new Set<string>();

// `add` on a binding that holds `new Queue(name)` produces to that queue.
export async function sendWelcome(to: string) {
  await emails.add("welcome", { to });
}

export async function requestReport() {
  await reports.add("monthly", {});
}

// Not a producer: `add` on a `Set`.
export function remember(id: string) {
  seen.add(id);
}
