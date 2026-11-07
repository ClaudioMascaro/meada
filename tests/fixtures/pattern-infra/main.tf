resource "aws_sqs_queue" "orders" { name = "${terraform.workspace}-orders" }
resource "aws_s3_bucket" "several" { bucket = "archive-${terraform.workspace}" }
resource "aws_s3_bucket" "short" { bucket = "a${terraform.workspace}" }
resource "aws_s3_bucket" "inside" { bucket = "arch${terraform.workspace}ive" }
resource "aws_s3_bucket" "both" { bucket = "both-${terraform.workspace}" }
