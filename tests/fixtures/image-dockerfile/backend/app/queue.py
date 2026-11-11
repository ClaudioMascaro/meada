import os

import boto3

queue = boto3.client("sqs")


def publish():
    queue.send_message(QueueUrl=os.getenv("QUEUE_URL"), MessageBody="x")
