output "s3_bucket_name" {
  value = aws_s3_bucket.site.id
}

output "cloudfront_distribution_id" {
  value = aws_cloudfront_distribution.site.id
}

output "cloudfront_domain_name" {
  value = aws_cloudfront_distribution.site.domain_name
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
