"""boto3: `send_message`, `send_message_batch` and `receive_message` of an SQS client."""

import os

import boto3
from not_boto import client as fake_client

REGION_URL = f"https://sqs.{os.environ['AWS_REGION']}.amazonaws.com/{os.environ['AWS_ACCOUNT']}"
JOBS = "https://sqs.eu-west-1.amazonaws.com/123456789012/jobs"

sqs = boto3.client("sqs")
storage = boto3.client("s3")
session = boto3.Session(region_name="eu-west-1")
queues = session.client("sqs")
fake = fake_client("sqs")


def send_job():
    # A constant URL, and a dynamic prefix: only the queue name, the last segment, is read.
    sqs.send_message(QueueUrl=JOBS, MessageBody="{}")
    sqs.send_message(QueueUrl=f"{REGION_URL}/mail", MessageBody="{}")


def send_joined():
    # Two literals side by side after the folding: the queue name straddles them.
    sqs.send_message(QueueUrl=JOBS + "-dlq", MessageBody="{}")


def send_batch():
    sqs.send_message_batch(QueueUrl=f"{REGION_URL}/audit", Entries=[])


# A client wrapper: each call site of `enqueue` produces, with its URL.
def enqueue(queue_url):
    return sqs.send_message(QueueUrl=queue_url, MessageBody="{}")


def enqueue_billing():
    return enqueue(f"{REGION_URL}/billing")


def send_session():
    queues.send_message(QueueUrl=f"{REGION_URL}/session", MessageBody="{}")


def send_chained():
    # Clients written as a chain of calls: a client of a session made in place, and a method
    # called on a client made in place. The service decides, as for any client.
    chained = boto3.Session(region_name="eu-west-1").client("sqs")
    chained.send_message(QueueUrl=f"{REGION_URL}/chained", MessageBody="{}")
    boto3.client("sqs").send_message(QueueUrl=f"{REGION_URL}/inline", MessageBody="{}")
    boto3.client("s3").send_message(QueueUrl=f"{REGION_URL}/other", MessageBody="{}")


def receive_jobs():
    # The function that receives handles.
    return sqs.receive_message(QueueUrl=JOBS, MaxNumberOfMessages=10)


def receive_mail():
    return sqs.receive_message(QueueUrl=f"{REGION_URL}/mail")


def receive_billing():
    return sqs.receive_message(QueueUrl=f"{REGION_URL}/billing")


def receive_session():
    return queues.receive_message(QueueUrl=f"{REGION_URL}/session")


def receive_unknown():
    # A URL read from the environment names no queue: an unresolved consumer.
    return sqs.receive_message(QueueUrl=os.environ["QUEUE_URL"])


def not_queued():
    # A client of another service, one of another package, and `send_message` on any object.
    storage.send_message(QueueUrl=JOBS)
    fake.send_message(QueueUrl=JOBS)
    os.send_message(QueueUrl=JOBS)
