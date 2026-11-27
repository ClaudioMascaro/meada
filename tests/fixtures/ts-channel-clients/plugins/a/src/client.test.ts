import { KafkaClient, listenTopic } from './client';

export function sample() {
  listenTopic(new KafkaClient(), 'test.topic');
}
