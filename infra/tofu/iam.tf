# -----------------------------------------------------------------------------
# GitHub Actions OIDC Provider
# -----------------------------------------------------------------------------

resource "aws_iam_openid_connect_provider" "github" {
  url             = "https://token.actions.githubusercontent.com"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = [var.github_oidc_thumbprint]
}

# -----------------------------------------------------------------------------
# CI Role (GitHub Actions → AWS via OIDC)
# Permissions: S3 deploy + CloudFront invalidation only
# -----------------------------------------------------------------------------

resource "aws_iam_role" "ci" {
  name = "aztec-accelerator-ci-github"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Principal = {
          Federated = aws_iam_openid_connect_provider.github.arn
        }
        Action = "sts:AssumeRoleWithWebIdentity"
        Condition = {
          StringEquals = {
            "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          }
          StringLike = {
            "token.actions.githubusercontent.com:sub" = [
              "repo:alejoamiras/aztec-accelerator:ref:refs/heads/main",
              "repo:alejoamiras/aztec-accelerator:ref:refs/heads/nightlies",
              "repo:alejoamiras/aztec-accelerator:ref:refs/heads/chore/aztec-nightlies-*",
              "repo:alejoamiras/aztec-accelerator:ref:refs/heads/chore/aztec-stable-*",
            ]
          }
        }
      }
    ]
  })
}

resource "aws_iam_role_policy" "ci" {
  name = "aztec-accelerator-ci-policy"
  role = aws_iam_role.ci.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "S3Deploy"
        Effect = "Allow"
        Action = [
          "s3:PutObject",
          "s3:DeleteObject",
          "s3:ListBucket",
          "s3:GetBucketLocation",
        ]
        Resource = [
          aws_s3_bucket.site.arn,
          "${aws_s3_bucket.site.arn}/*",
        ]
      },
      {
        Sid      = "CloudFrontInvalidation"
        Effect   = "Allow"
        Action   = "cloudfront:CreateInvalidation"
        Resource = aws_cloudfront_distribution.site.arn
      },
    ]
  })
}
