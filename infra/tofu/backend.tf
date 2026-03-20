terraform {
  required_version = ">= 1.6.0"

  backend "s3" {
    bucket       = "aztec-accelerator-tfstate"
    key          = "terraform.tfstate"
    region       = "us-east-1"
    encrypt      = true
    use_lockfile = true
  }
}
