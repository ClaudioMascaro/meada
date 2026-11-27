class KafkaClient { send(topic: string) {} }
export function publish(config) {
  const client = new KafkaClient();
  client.send(config.getConfig('a').getString('topic'));
}
