# -----------------------------------------------------------------------------
# GitHub Actions OIDC Provider
# -----------------------------------------------------------------------------

resource "aws_iam_openid_connect_provider" "github" {
  url             = "https://token.actions.githubusercontent.com"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = [var.github_oidc_thumbprint]
}

# -----------------------------------------------------------------------------
# F-005 (C5): least-privilege deploy trust.
#
# The legacy single `ci` role (below) trusted 4 refs and had whole-bucket write, shared by every
# pipeline. It is REPLACED by three per-pipeline roles, each trusting ONLY its own workflow (by the
# GitHub OIDC `workflow` NAME claim) running on `main`, and scoped to only its S3 prefix. Only the
# release pipeline may write the F-004-critical `landing/releases/latest.json`.
#
# `workflow` NAME claim (not `job_workflow_ref`): the two design audits disagreed on whether
# `job_workflow_ref` is emitted for these TOP-LEVEL jobs; `workflow` is AWS-supported and present for
# all jobs. A rename-to-impersonate attack requires merging to `main` — a subset of the already-accepted
# "malicious code on main" residual. See implementations-plan/security-hardening/clusters/C5-CONSOLIDATED.md (D1).
#
# Cutover is human-applied and staged (see the C5 runbook): apply roles (legacy retained, trust-narrowed)
# -> set the 3 secrets -> land the workflow cutover on main -> smoke -> delete the legacy role (separate PR).
# -----------------------------------------------------------------------------

locals {
  # Only `main` is trusted (F-005: nightlies + chore/aztec-* refs dropped). Solo public repo.
  github_sub_main = "repo:alejoamiras/aztec-accelerator:ref:refs/heads/main"
}

# Assume-role trust for each per-pipeline role: exact aud + exact main sub + exact workflow NAME.
data "aws_iam_policy_document" "assume_landing" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [aws_iam_openid_connect_provider.github.arn]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values   = [local.github_sub_main]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:workflow"
      values   = ["Deploy Landing Page"]
    }
  }
}

data "aws_iam_policy_document" "assume_release" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [aws_iam_openid_connect_provider.github.arn]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values   = [local.github_sub_main]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:workflow"
      values   = ["Release Accelerator"]
    }
  }
}

data "aws_iam_policy_document" "assume_playground" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [aws_iam_openid_connect_provider.github.arn]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values   = [local.github_sub_main]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:workflow"
      values   = ["Publish Testnet"]
    }
  }
}

# ── Landing role: writes landing/* but is explicitly DENIED the release-feed prefix ──
resource "aws_iam_role" "landing_deploy" {
  name               = "aztec-accelerator-ci-landing"
  assume_role_policy = data.aws_iam_policy_document.assume_landing.json
}

resource "aws_iam_role_policy" "landing_deploy" {
  name = "aztec-accelerator-ci-landing-policy"
  role = aws_iam_role.landing_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "LandingWrite"
        Effect   = "Allow"
        Action   = ["s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"]
        Resource = "${aws_s3_bucket.site.arn}/landing/*"
      },
      {
        # SECURITY BOUNDARY (F-005/F-004): the landing pipeline must never write/delete the update feed.
        # Explicit Deny beats the landing/* Allow and any future Allow. The workflow's `sync --delete`
        # must `--exclude "releases"`/`"releases/*"` or the deploy fails on this Deny (the fail-loud property).
        Sid    = "DenyReleaseFeed"
        Effect = "Deny"
        Action = ["s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"]
        Resource = [
          "${aws_s3_bucket.site.arn}/landing/releases",
          "${aws_s3_bucket.site.arn}/landing/releases/*",
        ]
      },
      {
        Sid       = "LandingList"
        Effect    = "Allow"
        Action    = "s3:ListBucket"
        Resource  = aws_s3_bucket.site.arn
        Condition = { StringLike = { "s3:prefix" = ["landing/*"] } }
      },
      {
        Sid      = "BucketLocation"
        Effect   = "Allow"
        Action   = "s3:GetBucketLocation"
        Resource = aws_s3_bucket.site.arn
      },
      {
        Sid      = "Invalidate"
        Effect   = "Allow"
        Action   = "cloudfront:CreateInvalidation"
        Resource = aws_cloudfront_distribution.site.arn
      },
    ]
  })
}

# ── Release-feed role: may ONLY put the exact latest.json object (no List, no Delete) ──
resource "aws_iam_role" "release_feed" {
  name               = "aztec-accelerator-ci-release-feed"
  assume_role_policy = data.aws_iam_policy_document.assume_release.json
}

resource "aws_iam_role_policy" "release_feed" {
  name = "aztec-accelerator-ci-release-feed-policy"
  role = aws_iam_role.release_feed.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "PutFeed"
        Effect   = "Allow"
        Action   = ["s3:PutObject", "s3:AbortMultipartUpload"]
        Resource = "${aws_s3_bucket.site.arn}/landing/releases/latest.json"
      },
      {
        Sid      = "BucketLocation"
        Effect   = "Allow"
        Action   = "s3:GetBucketLocation"
        Resource = aws_s3_bucket.site.arn
      },
      {
        Sid      = "Invalidate"
        Effect   = "Allow"
        Action   = "cloudfront:CreateInvalidation"
        Resource = aws_cloudfront_distribution.site.arn
      },
    ]
  })
}

# ── Playground (testnet) role: writes playground/* only ──
resource "aws_iam_role" "playground_testnet" {
  name               = "aztec-accelerator-ci-playground-testnet"
  assume_role_policy = data.aws_iam_policy_document.assume_playground.json
}

resource "aws_iam_role_policy" "playground_testnet" {
  name = "aztec-accelerator-ci-playground-testnet-policy"
  role = aws_iam_role.playground_testnet.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "PlaygroundWrite"
        Effect   = "Allow"
        Action   = ["s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"]
        Resource = "${aws_s3_bucket.site.arn}/playground/*"
      },
      {
        Sid       = "PlaygroundList"
        Effect    = "Allow"
        Action    = "s3:ListBucket"
        Resource  = aws_s3_bucket.site.arn
        Condition = { StringLike = { "s3:prefix" = ["playground/*"] } }
      },
      {
        Sid      = "BucketLocation"
        Effect   = "Allow"
        Action   = "s3:GetBucketLocation"
        Resource = aws_s3_bucket.site.arn
      },
      {
        Sid      = "Invalidate"
        Effect   = "Allow"
        Action   = "cloudfront:CreateInvalidation"
        Resource = aws_cloudfront_distribution.site.arn
      },
    ]
  })
}

# -----------------------------------------------------------------------------
# LEGACY CI Role — F-005: trust NARROWED to `main` only (nightlies + chore/aztec-* dropped). Broad policy
# retained TEMPORARILY so live workflows keep deploying until the PR-2 workflow cutover lands on main;
# this role + policy are removed in the SEPARATE Phase-3 PR after the new roles are smoked. Do NOT add
# refs back here.
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
            "token.actions.githubusercontent.com:sub" = local.github_sub_main
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
