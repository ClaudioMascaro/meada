import { SNSClient, PublishCommand } from '@aws-sdk/client-sns';
const client = new SNSClient({});
export function javascript() { client.send(new PublishCommand({ TopicArn: 'js-topic' })); }
