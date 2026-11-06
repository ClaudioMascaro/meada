import boto3
import os
sns = boto3.client('sns')
s3 = boto3.Session().client('s3')
def publish():
    sns.publish(TopicArn='arn:aws:sns:us-east-1:123456789012:orders', Message='body')
def store():
    s3.put_object(Bucket='orders-bucket', Key='a', Body=b'b')
    s3.get_object(Bucket='orders-bucket', Key='a')
    s3.get_object(Bucket=os.environ['BUCKET'], Key='a')
