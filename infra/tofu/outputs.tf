output "s3_bucket_name" {
  value = aws_s3_bucket.site.id
}

output "cloudfront_distribution_id" {
  value = aws_cloudfront_distribution.site.id
}

output "cloudfront_domain_name" {
  value = aws_cloudfront_distribution.site.domain_name
}

# Legacy shared role — removed in the F-005 Phase-3 PR after the per-pipeline roles are cut over.
output "ci_role_arn" {
  value = aws_iam_role.ci.arn
}

# F-005 per-pipeline deploy roles. The human wires each into its own GitHub Actions secret:
# landing -> AWS_ROLE_ARN_LANDING, release -> AWS_ROLE_ARN_RELEASE, playground -> AWS_ROLE_ARN_PLAYGROUND.
output "landing_deploy_role_arn" {
  value = aws_iam_role.landing_deploy.arn
}

output "release_feed_role_arn" {
  value = aws_iam_role.release_feed.arn
}

output "playground_testnet_role_arn" {
  value = aws_iam_role.playground_testnet.arn
}
