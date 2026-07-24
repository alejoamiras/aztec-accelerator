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
  # DeleteObjectVersion — so without expiry, noncurrent versions stay billable indefinitely.
  #
  # NOTE on S3 semantics (re-audit correction): `noncurrent_days` and `newer_noncurrent_versions` are
  # AND-combined — a version is expired only once it is BOTH older than `noncurrent_days` AND beyond the
  # newest N. So this rule bounds the STEADY STATE (after the window, at most ~10 noncurrent per object)
  # and caps the transient to a 7-day window, but it does NOT strictly bound cost against an adversary
  # churning many overwrites WITHIN 7 days. That is inherent: a strict count-only cap would instead let
  # churn EVICT the good recovery version, defeating the anti-tamper intent — the two goals conflict under
  # a compromised deploy token. The real fix for adversarial churn is CI LEAST-PRIVILEGE (object-count /
  # rate limits, or S3 Object Lock), tracked in the owner runbook; S3 versioning here is a
  # redeploy-from-source *convenience*, not the primary recovery. Current (live) versions are untouched.
  rule {
    id     = "expire-noncurrent-versions"
    status = "Enabled"

    filter {} # whole bucket

    noncurrent_version_expiration {
      noncurrent_days           = 7  # recovery window; also caps the unbounded-churn transient to 7 days
      newer_noncurrent_versions = 10 # steady-state cap: keep ~10 recent noncurrent copies per object
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
