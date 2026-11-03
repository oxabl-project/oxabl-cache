# One data region of the hosted service: two API nodes, a Postgres node and an R2 bucket.
# North America by default; for the EU set location = "fsn1" and r2_jurisdiction = "eu".

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

variable "r2_jurisdiction" {
  type    = string
  default = "default"
}

variable "ssh_public_keys" {
  type = list(string)
}

variable "ssh_allowed_cidrs" {
  type = list(string)
}

variable "tls_certificate_pem" {
  type      = string
  sensitive = true
}

variable "tls_private_key_pem" {
  type      = string
  sensitive = true
}

module "hosted" {
  source = "../.."

  mode                  = "cluster"
  api_node_count        = 2
  domain                = var.domain
  location              = var.location
  server_type           = var.server_type
  volume_size_gb        = 50
  floating_ip           = true
  tls_mode              = "certificate"
  tls_certificate_pem   = var.tls_certificate_pem
  tls_private_key_pem   = var.tls_private_key_pem
  ssh_public_keys       = var.ssh_public_keys
  ssh_allowed_cidrs     = var.ssh_allowed_cidrs
  cloudflare_account_id = var.cloudflare_account_id
  r2_bucket_name        = var.location == "ash" || var.location == "hil" ? "oxabl-cache-na" : "oxabl-cache-eu"
  r2_jurisdiction       = var.r2_jurisdiction
}

output "public_ipv4" {
  value = module.hosted.public_ipv4
}

output "next_steps" {
  value = module.hosted.next_steps
}
