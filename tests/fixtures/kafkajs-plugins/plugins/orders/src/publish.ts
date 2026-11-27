import { Config } from '@backstage/config';
import { KafkaClient } from './KafkaClient';

// The route constructs this client, so its call of send supplies a second producer site.
export async function publishFromRoute(config: Config) {
  const client = new KafkaClient(['broker:9092']);
  await client.send(config.getString('acme.orders.topics.approved'), 'approved');
}
