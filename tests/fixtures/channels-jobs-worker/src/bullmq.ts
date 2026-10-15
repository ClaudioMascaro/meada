import { Worker } from "bullmq";

// The processor is the handler: a definition, and a function written in place.
export function startEmails() {
  return new Worker("emails", sendEmail, { connection: {} });
}

export async function sendEmail(job: unknown) {
  return job;
}

export function startReports() {
  return new Worker("reports", async (job) => {
    return job;
  });
}
