# Certificate for aztec-accelerator.dev (us-east-1, required by CloudFront)
# Import-only: manually created and validated via Cloudflare DNS.
# lifecycle ignore_changes = all prevents OpenTofu from modifying it.

resource "aws_acm_certificate" "site" {
  domain_name               = "aztec-accelerator.dev"
  subject_alternative_names = ["*.aztec-accelerator.dev"]
  validation_method         = "DNS"

  lifecycle {
    prevent_destroy = true
    ignore_changes  = all
  }
}
