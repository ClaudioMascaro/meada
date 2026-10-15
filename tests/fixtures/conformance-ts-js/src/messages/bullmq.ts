import * as bull from "bullmq";
import { Queue, Worker } from "bullmq";
import { Queue as FakeQueue, Worker as FakeWorker } from "not-bullmq";

const REGION = "eu";
const JOBS = "jobs";

const jobs = new Queue(JOBS, { connection: { host: "redis" } });
const mail = new bull.Queue(`mail-${REGION}`);
const dynamic = new Queue(process.env.QUEUE_NAME);
const fake = new FakeQueue("jobs");
const seen = new Set<string>();

// `add` on a binding that holds `new Queue(name)`; the name is folded from constants.
export async function enqueueAll() {
  await jobs.add("run", {});
  await mail.add("send", {});
  await dynamic.add("send", {});
  await fake.add("run", {});
  seen.add("run");
}

// A client wrapper does not change the queue: the binding names it.
function enqueue(name: string) {
  return jobs.add(name, {});
}

export function enqueueTwice() {
  enqueue("first");
  enqueue("second");
}

// The processor is the handler: a definition, and a function written in place.
export function startJobs() {
  return new Worker("jobs", runJob, { connection: { host: "redis" } });
}

export function startMail() {
  return new bull.Worker(`mail-${REGION}`, async (job) => {
    record(job);
  });
}

// A queue name Meada cannot evaluate is an unresolved side, never linked.
export function startDynamic() {
  return new Worker(process.env.QUEUE_NAME, runJob);
}

// Not channels: a worker and a queue of another package.
export function notBullmq() {
  new FakeWorker("jobs", runJob);
}

function runJob(job: unknown) {
  return job;
}

function record(value: unknown) {
  return value;
}
