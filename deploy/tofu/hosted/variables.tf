variable "name" {
  description = "Prefix of every resource name."
  type        = string
  default     = "oxabl-cache"
}

variable "mode" {
  description = "single: one node with SQLite on a volume. cluster: api_node_count stateless nodes and one Postgres node."
  type        = string
  default     = "single"

  validation {
    condition     = contains(["single", "cluster"], var.mode)
    error_message = "mode is single or cluster."
  }
}

variable "domain" {
  description = "Public host name of the service, for example cache.example.com. DNS is left to the operator."
  type        = string

  validation {
    condition     = can(regex("^([a-z0-9-]+\\.)+[a-z]{2,}$", var.domain))
    error_message = "domain is a lower-case host name."
  }
}

variable "image" {
  description = "Server image. Pin a digest in production."
  type        = string
  default     = "ghcr.io/oxabl-project/oxabl-cache-server:latest"
}

variable "caddy_image" {
  description = "Reverse proxy image used when tls_mode is caddy."
  type        = string
  default     = "docker.io/library/caddy:2"
}

variable "postgres_image" {
  description = "Postgres image of the database node (cluster mode)."
  type        = string
  default     = "docker.io/library/postgres:18"
}

variable "tls_mode" {
  description = "caddy: Caddy obtains a certificate for the domain (single API node). certificate: the server terminates TLS from the supplied PEM files."
  type        = string
  default     = "caddy"

  validation {
    condition     = contains(["caddy", "certificate"], var.tls_mode)
    error_message = "tls_mode is caddy or certificate."
  }
}

variable "tls_certificate_pem" {
  description = "Certificate chain, when tls_mode is certificate."
  type        = string
  default     = ""
  sensitive   = true
}

variable "tls_private_key_pem" {
  description = "Private key, when tls_mode is certificate."
  type        = string
  default     = ""
  sensitive   = true
}

variable "location" {
  description = "Hetzner Cloud location: ash or hil (North America), fsn1, nbg1 or hel1 (EU)."
  type        = string
  default     = "ash"

  validation {
    condition     = contains(["ash", "hil", "fsn1", "nbg1", "hel1"], var.location)
    error_message = "location is ash, hil, fsn1, nbg1 or hel1."
  }
}

variable "server_type" {
  description = "Hetzner server type of the API nodes: the cheapest with at least 2 vCPU and 4 GB. List them with `hcloud server-type list`."
  type        = string
}

variable "db_server_type" {
  description = "Server type of the Postgres node; defaults to server_type."
  type        = string
  default     = null
}

variable "api_node_count" {
  description = "API nodes. 1 in single mode; 2 or more in cluster mode."
  type        = number
  default     = 1

  validation {
    condition     = var.api_node_count >= 1 && var.api_node_count <= 10
    error_message = "api_node_count is between 1 and 10."
  }
}

variable "volume_size_gb" {
  description = "Size of the data volume (SQLite and local objects in single mode, Postgres in cluster mode)."
  type        = number
  default     = 10

  validation {
    condition     = var.volume_size_gb >= 10
    error_message = "Hetzner volumes start at 10 GB."
  }
}

variable "floating_ip" {
  description = "Allocate a floating IPv4 address and assign it to the first API node."
  type        = bool
  default     = false
}

variable "server_backups" {
  description = "Enable Hetzner server backups (adds 20% to the server price; volumes are not included)."
  type        = bool
  default     = false
}

variable "ssh_public_keys" {
  description = "SSH public keys that may log in as root."
  type        = list(string)

  validation {
    condition     = length(var.ssh_public_keys) > 0
    error_message = "At least one SSH public key is needed to run init."
  }
}

variable "ssh_allowed_cidrs" {
  description = "Source ranges allowed to reach SSH. Empty closes port 22."
  type        = list(string)
  default     = []
}

variable "store_kind" {
  description = "s3: a Cloudflare R2 bucket. fs: the data volume."
  type        = string
  default     = "s3"

  validation {
    condition     = contains(["s3", "fs"], var.store_kind)
    error_message = "store_kind is s3 or fs."
  }
}

variable "cloudflare_account_id" {
  description = "Cloudflare account id that owns the R2 bucket."
  type        = string
  default     = ""
}

variable "r2_bucket_name" {
  description = "Name of the R2 bucket."
  type        = string
  default     = "oxabl-cache"
}

variable "r2_jurisdiction" {
  description = "default, eu or fedramp. A bucket's jurisdiction cannot change."
  type        = string
  default     = "default"

  validation {
    condition     = contains(["default", "eu", "fedramp"], var.r2_jurisdiction)
    error_message = "r2_jurisdiction is default, eu or fedramp."
  }
}

variable "r2_location" {
  description = "Optional R2 location hint: apac, eeur, enam, weur or wnam."
  type        = string
  default     = null
}

variable "create_r2_token" {
  description = "Create an account API token limited to the bucket. When false, supply r2_access_key_id and r2_secret_access_key."
  type        = bool
  default     = true
}

variable "r2_access_key_id" {
  description = "S3 access key id of an R2 token created by hand."
  type        = string
  default     = ""
  sensitive   = true
}

variable "r2_secret_access_key" {
  description = "S3 secret access key of an R2 token created by hand."
  type        = string
  default     = ""
  sensitive   = true
}

variable "oidc_issuer_allowlist" {
  description = "OIDC issuers tenants may register."
  type        = list(string)
  default     = ["https://token.actions.githubusercontent.com", "https://gitlab.com"]
}

variable "default_hard_quota" {
  description = "Default hard quota of a new tenant."
  type        = string
  default     = "20GiB"
}

variable "default_retention_days" {
  description = "Default retention of entries."
  type        = number
  default     = 30
}

variable "default_rate" {
  description = "Default requests per second per tenant."
  type        = number
  default     = 50
}

variable "default_burst" {
  description = "Default request burst per tenant."
  type        = number
  default     = 200
}

variable "log_level" {
  description = "debug, info, warn or error."
  type        = string
  default     = "info"
}
