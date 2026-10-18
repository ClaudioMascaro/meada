import amqp from "amqplib";

const connection = await amqp.connect("amqp://localhost");

// A channel, and a confirm channel, both from the default import.
export const channel = await connection.createChannel();
export const confirming = await connection.createConfirmChannel();
