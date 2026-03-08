terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
  required_version = ">= 1.0"
}

# Point at openstack instead of real AWS
provider "aws" {
  region                      = "us-east-1"
  access_key                  = "test"
  secret_key                  = "test"
  skip_credentials_validation = true
  skip_metadata_api_check     = true
  skip_requesting_account_id  = true

  endpoints {
    s3       = var.openstack_endpoint
    sqs      = var.openstack_endpoint
    dynamodb = var.openstack_endpoint
    iam      = var.openstack_endpoint
    sts      = var.openstack_endpoint
    sns      = var.openstack_endpoint
    kms      = var.openstack_endpoint
    ssm      = var.openstack_endpoint
  }
}

variable "openstack_endpoint" {
  description = "openstack endpoint URL"
  default     = "http://localhost:4566"
}

# ── S3 Bucket ─────────────────────────────────────────────────────────────────
resource "aws_s3_bucket" "terraform_test" {
  bucket = "terraform-compat-bucket"
}

resource "aws_s3_bucket_versioning" "terraform_test" {
  bucket = aws_s3_bucket.terraform_test.id
  versioning_configuration {
    status = "Enabled"
  }
}

# ── SQS Queue ─────────────────────────────────────────────────────────────────
resource "aws_sqs_queue" "terraform_test" {
  name                       = "terraform-compat-queue"
  delay_seconds              = 0
  max_message_size           = 262144
  message_retention_seconds  = 86400
  receive_wait_time_seconds  = 0
}

resource "aws_sqs_queue" "terraform_dlq" {
  name = "terraform-compat-dlq"
}

resource "aws_sqs_queue_redrive_policy" "terraform_test" {
  queue_url = aws_sqs_queue.terraform_test.id
  redrive_policy = jsonencode({
    deadLetterTargetArn = aws_sqs_queue.terraform_dlq.arn
    maxReceiveCount     = 3
  })
}

# ── DynamoDB Table ────────────────────────────────────────────────────────────
resource "aws_dynamodb_table" "terraform_test" {
  name         = "terraform-compat-table"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "pk"
  range_key    = "sk"

  attribute {
    name = "pk"
    type = "S"
  }
  attribute {
    name = "sk"
    type = "S"
  }
  attribute {
    name = "gsi_pk"
    type = "S"
  }

  global_secondary_index {
    name            = "gsi-pk-index"
    hash_key        = "gsi_pk"
    projection_type = "ALL"
  }
}

# ── SNS Topic ─────────────────────────────────────────────────────────────────
resource "aws_sns_topic" "terraform_test" {
  name = "terraform-compat-topic"
}

# SNS → SQS subscription
resource "aws_sns_topic_subscription" "terraform_test" {
  topic_arn = aws_sns_topic.terraform_test.arn
  protocol  = "sqs"
  endpoint  = aws_sqs_queue.terraform_test.arn
}

# ── IAM Role ──────────────────────────────────────────────────────────────────
resource "aws_iam_role" "terraform_test" {
  name = "terraform-compat-role"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action    = "sts:AssumeRole"
      Effect    = "Allow"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy" "terraform_test" {
  name = "terraform-compat-policy"
  role = aws_iam_role.terraform_test.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["s3:GetObject", "s3:PutObject"]
      Resource = "${aws_s3_bucket.terraform_test.arn}/*"
    }]
  })
}

# ── SSM Parameter ─────────────────────────────────────────────────────────────
resource "aws_ssm_parameter" "terraform_test" {
  name  = "/terraform/compat/param"
  type  = "String"
  value = "terraform-value"
}

# ── Outputs ───────────────────────────────────────────────────────────────────
output "s3_bucket_name" {
  value = aws_s3_bucket.terraform_test.bucket
}

output "sqs_queue_url" {
  value = aws_sqs_queue.terraform_test.url
}

output "dynamodb_table_name" {
  value = aws_dynamodb_table.terraform_test.name
}

output "sns_topic_arn" {
  value = aws_sns_topic.terraform_test.arn
}

output "iam_role_arn" {
  value = aws_iam_role.terraform_test.arn
}
