# Single S3 bucket for static site hosting
# Landing page → /landing/ prefix
# Playground   → /playground/ prefix
resource "aws_s3_bucket" "site" {
  bucket = "aztec-accelerator-site"
}

resource "aws_s3_bucket_public_access_block" "site" {
  bucket = aws_s3_bucket.site.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# F-005 (Codex C6): a compromised deploy token can start multipart uploads and leave incomplete parts,
# which stay billable after the OIDC session expires. Abort incomplete multipart uploads after 1 day.
resource "aws_s3_bucket_lifecycle_configuration" "site" {
  bucket = aws_s3_bucket.site.id

  rule {
    id     = "abort-incomplete-multipart-uploads"
    status = "Enabled"

    filter {} # whole bucket

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }

  # H3 (full-branch audit): versioning (below) keeps every prior copy on overwrite/delete, and CI has no
  # DeleteObjectVersion — so a compromised deploy token that repeatedly uploads+deletes large objects
  # would accumulate NONCURRENT versions that stay billable INDEFINITELY (storage-cost exhaustion). Expire
  # noncurrent versions after a bounded recovery window, and cap how many are retained per object, so the
  # anti-tamper recovery guarantee is preserved without unbounded cost. Current (live) versions are never
  # touched by this rule.
  rule {
    id     = "expire-noncurrent-versions"
    status = "Enabled"

    filter {} # whole bucket

    noncurrent_version_expiration {
      noncurrent_days           = 30 # 30-day recovery window for an accidental/compromised overwrite
      newer_noncurrent_versions = 10 # cap retained noncurrent copies per object (bounds churn abuse)
    }
  }
}

# F-005 (Ask A8): versioning so an accidental/compromised site overwrite is recoverable WITHOUT granting
# CI any DeleteObjectVersion. CI roles have no version permissions; recovery is an owner/admin action.
resource "aws_s3_bucket_versioning" "site" {
  bucket = aws_s3_bucket.site.id

  versioning_configuration {
    status = "Enabled"
  }
}

# Allow CloudFront OAC to read from the bucket
resource "aws_s3_bucket_policy" "site" {
  bucket = aws_s3_bucket.site.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "AllowCloudFrontOAC"
        Effect = "Allow"
        Principal = {
          Service = "cloudfront.amazonaws.com"
        }
        Action   = "s3:GetObject"
        Resource = "${aws_s3_bucket.site.arn}/*"
        Condition = {
          StringEquals = {
            "AWS:SourceArn" = aws_cloudfront_distribution.site.arn
          }
        }
      }
    ]
  })
}
