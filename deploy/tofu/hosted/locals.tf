locals {
  single = var.mode == "single"
  s3     = var.store_kind == "s3"

  network_zone = {
    ash  = "us-east"
    hil  = "us-west"
    fsn1 = "eu-central"
    nbg1 = "eu-central"
    hel1 = "eu-central"
  }[var.location]

  network_range   = "10.0.0.0/16"
  subnet_range    = "10.0.0.0/24"
  api_private_ips = [for i in range(var.api_node_count) : cidrhost(local.subnet_range, 10 + i)]
  db_private_ip   = cidrhost(local.subnet_range, 5)

  r2_host = {
    default = "r2.cloudflarestorage.com"
    eu      = "eu.r2.cloudflarestorage.com"
    fedramp = "fedramp.r2.cloudflarestorage.com"
  }[var.r2_jurisdiction]
  r2_endpoint = "https://${var.cloudflare_account_id}.${local.r2_host}"

  s3_access_key_id     = !local.s3 ? "" : var.create_r2_token ? cloudflare_account_token.r2[0].id : var.r2_access_key_id
  s3_secret_access_key = !local.s3 ? "" : var.create_r2_token ? sha256(cloudflare_account_token.r2[0].value) : var.r2_secret_access_key

  labels = {
    app = var.name
  }
}

resource "terraform_data" "guard" {
  lifecycle {
    precondition {
      condition     = local.single ? var.api_node_count == 1 : var.api_node_count >= 2
      error_message = "single mode runs one API node; cluster mode runs two or more."
    }
    precondition {
      condition     = var.tls_mode != "caddy" || var.api_node_count == 1
      error_message = "Caddy obtains a certificate per node, which only works for one API node. Use tls_mode = certificate for more."
    }
    precondition {
      condition     = var.tls_mode != "certificate" || (var.tls_certificate_pem != "" && var.tls_private_key_pem != "")
      error_message = "tls_mode = certificate needs tls_certificate_pem and tls_private_key_pem."
    }
    precondition {
      condition     = !local.s3 || var.cloudflare_account_id != ""
      error_message = "store_kind = s3 needs cloudflare_account_id."
    }
    precondition {
      condition     = !local.s3 || var.create_r2_token || (var.r2_access_key_id != "" && var.r2_secret_access_key != "")
      error_message = "With create_r2_token = false, supply r2_access_key_id and r2_secret_access_key."
    }
    precondition {
      condition     = local.single || local.s3
      error_message = "cluster mode needs an object store every node reaches: store_kind = s3."
    }
    precondition {
      condition     = !var.floating_ip || var.tls_mode == "certificate" || var.api_node_count == 1
      error_message = "A floating IP serves one node."
    }
  }
}
