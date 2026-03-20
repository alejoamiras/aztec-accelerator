# ACM certificate ARN for aztec-accelerator.dev (must be in us-east-1)
# Create manually: ACM → Request certificate → aztec-accelerator.dev + *.aztec-accelerator.dev
# Validate via Cloudflare DNS (add the CNAME record ACM provides)
variable "acm_certificate_arn" {
  description = "ACM certificate ARN for aztec-accelerator.dev (us-east-1)"
  type        = string
}

# GitHub OIDC thumbprint — unlikely to change but needed for provider setup
variable "github_oidc_thumbprint" {
  description = "GitHub Actions OIDC provider certificate thumbprint"
  type        = string
  default     = "6938fd4d98bab03faadb97b34396831e3780aea1"
}
