import boto3
queue = boto3.client("sqs")
def sns_receive():
    return queue.receive_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/sns-jobs")
def filtered_receive():
    return queue.receive_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/filtered-jobs")
def bus_receive():
    return queue.receive_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/bus-jobs")
def pattern_receive():
    return queue.receive_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/pattern-jobs")
