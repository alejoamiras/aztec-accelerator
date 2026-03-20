terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = "us-east-1"
}

# CloudFront + ACM require us-east-1 — this is a single-region stack
data "aws_caller_identity" "current" {}
data "aws_region" "current" {}
