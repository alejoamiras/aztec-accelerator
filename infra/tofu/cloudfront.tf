# -----------------------------------------------------------------------------
# Origin Access Control
# -----------------------------------------------------------------------------

resource "aws_cloudfront_origin_access_control" "site" {
  name                              = "aztec-accelerator-site-oac"
  description                       = "OAC for aztec-accelerator static site"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# -----------------------------------------------------------------------------
# Response Headers Policy (COOP/COEP for SharedArrayBuffer)
# -----------------------------------------------------------------------------

resource "aws_cloudfront_response_headers_policy" "coop_coep" {
  name = "aztec-accelerator-coop-coep"

  custom_headers_config {
    items {
      header   = "Cross-Origin-Opener-Policy"
      value    = "same-origin"
      override = true
    }
    items {
      header   = "Cross-Origin-Embedder-Policy"
      value    = "credentialless"
      override = true
    }
  }
}

# -----------------------------------------------------------------------------
# CloudFront Function — subdomain-based request routing
# aztec-accelerator.dev                    → S3 /landing/
# playground.aztec-accelerator.dev         → S3 /playground/
# nightly-playground.aztec-accelerator.dev → S3 /playground-nightly/
# Also handles SPA fallback for the playground (no extension → index.html)
# -----------------------------------------------------------------------------

resource "aws_cloudfront_function" "subdomain_router" {
  name    = "aztec-accelerator-subdomain-router"
  runtime = "cloudfront-js-2.0"
  publish = true
  code    = <<-EOF
    function handler(event) {
      var request = event.request;
      var host = request.headers.host.value;
      var uri = request.uri;

      var prefix;
      if (host.startsWith('nightly-playground.')) {
        prefix = '/playground-nightly';
      } else if (host.startsWith('playground.')) {
        prefix = '/playground';
      } else {
        prefix = '/landing';
      }

      // SPA fallback: no file extension → serve index.html
      if (uri.endsWith('/') || uri.lastIndexOf('.') <= uri.lastIndexOf('/')) {
        request.uri = prefix + '/index.html';
      } else {
        request.uri = prefix + uri;
      }

      return request;
    }
  EOF
}

# -----------------------------------------------------------------------------
# CloudFront Distribution
# aztec-accelerator.dev                    → landing page
# playground.aztec-accelerator.dev         → proving comparison app
# nightly-playground.aztec-accelerator.dev → nightly playground
# -----------------------------------------------------------------------------

resource "aws_cloudfront_distribution" "site" {
  comment         = "aztec-accelerator — landing + playground + nightly playground"
  enabled         = true
  is_ipv6_enabled = true
  http_version    = "http2"
  price_class     = "PriceClass_100"
  aliases         = ["aztec-accelerator.dev", "playground.aztec-accelerator.dev", "nightly-playground.aztec-accelerator.dev"]

  origin {
    domain_name              = aws_s3_bucket.site.bucket_regional_domain_name
    origin_id                = "s3-site"
    origin_access_control_id = aws_cloudfront_origin_access_control.site.id
  }

  default_cache_behavior {
    allowed_methods            = ["GET", "HEAD", "OPTIONS"]
    cached_methods             = ["GET", "HEAD"]
    target_origin_id           = "s3-site"
    viewer_protocol_policy     = "redirect-to-https"
    response_headers_policy_id = aws_cloudfront_response_headers_policy.coop_coep.id
    compress                   = true

    # Route requests to correct S3 prefix based on Host header
    function_association {
      event_type   = "viewer-request"
      function_arn = aws_cloudfront_function.subdomain_router.arn
    }

    forwarded_values {
      query_string = false
      cookies {
        forward = "none"
      }
    }

    min_ttl     = 0
    default_ttl = 86400
    max_ttl     = 31536000
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    acm_certificate_arn      = var.acm_certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}
