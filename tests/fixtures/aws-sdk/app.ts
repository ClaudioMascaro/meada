import { SNSClient, PublishCommand } from '@aws-sdk/client-sns';
const sns = new SNSClient({});
export async function publish() {
  await sns.send(new PublishCommand({ TopicArn: 'arn:aws:sns:us-east-1:123456789012:orders' }));
}
import { S3Client, PutObjectCommand, GetObjectCommand } from '@aws-sdk/client-s3';
const s3 = new S3Client({});
export async function store() {
  await s3.send(new PutObjectCommand({ Bucket: 'orders-bucket', Key: 'a' }));
  await s3.send(new GetObjectCommand({ Bucket: 'orders-bucket', Key: 'a' }));
  await s3.send(new GetObjectCommand({ Bucket: process.env.BUCKET }));
}
