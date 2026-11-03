# One small node, SQLite on a volume, one R2 bucket. About 7 USD a month plus VAT.

terraform {
  required_version = ">= 1.9.0"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.28"
    }
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.70"
    }
  }
}

provider "cloudflare" {
  # CLOUDFLARE_API_TOKEN
}

provider "hcloud" {
  # HCLOUD_TOKEN
}

variable "domain" {
  type = string
}

variable "cloudflare_account_id" {
  type = string
}

variable "server_type" {
  type = string
}

variable "location" {
  type    = string
  default = "ash"
}

variable "ssh_public_keys" {
  type = list(string)
}

variable "ssh_allowed_cidrs" {
  type = list(string)
}

module "hosted" {
  source = "../.."

  mode                  = "single"
  domain                = var.domain
  location              = var.location
  server_type           = var.server_type
  volume_size_gb        = 10
  ssh_public_keys       = var.ssh_public_keys
  ssh_allowed_cidrs     = var.ssh_allowed_cidrs
  cloudflare_account_id = var.cloudflare_account_id
  r2_bucket_name        = "oxabl-cache-proto"
}

output "public_ipv4" {
  value = module.hosted.public_ipv4
}

output "next_steps" {
  value = module.hosted.next_steps
}
