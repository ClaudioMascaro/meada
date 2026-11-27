import { Config } from '@backstage/config';
import { KafkaClient } from './KafkaClient';

// A route publishing through the client: `send` is a method several classes define, so the call
// is `ambiguous` and the topic does not lift to here.
export async function publishFromRoute(config: Config) {
  const client = new KafkaClient(['broker:9092']);
  await client.send(config.getString('acme.orders.topics.approved'), 'approved');
}
