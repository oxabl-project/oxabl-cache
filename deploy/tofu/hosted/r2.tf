resource "cloudflare_r2_bucket" "cache" {
  count = local.s3 ? 1 : 0

  account_id   = var.cloudflare_account_id
  name         = var.r2_bucket_name
  jurisdiction = var.r2_jurisdiction == "default" ? null : var.r2_jurisdiction
  location     = var.r2_location
}

data "cloudflare_account_api_token_permission_groups_list" "r2_write" {
  count = local.s3 && var.create_r2_token ? 1 : 0

  account_id = var.cloudflare_account_id
  name       = urlencode("Workers R2 Storage Bucket Item Write")
}

locals {
  r2_write_group = local.s3 && var.create_r2_token ? one([
    for group in data.cloudflare_account_api_token_permission_groups_list.r2_write[0].result : group.id
    if group.name == "Workers R2 Storage Bucket Item Write"
  ]) : null
}

resource "cloudflare_account_token" "r2" {
  count = local.s3 && var.create_r2_token ? 1 : 0

  account_id = var.cloudflare_account_id
  name       = "${var.name}-${var.r2_bucket_name}-objects"

  policies = [{
    effect = "allow"
    permission_groups = [{
      id = local.r2_write_group
    }]
    resources = jsonencode({
      "com.cloudflare.edge.r2.bucket.${var.cloudflare_account_id}_${var.r2_jurisdiction}_${var.r2_bucket_name}" = "*"
    })
  }]

  lifecycle {
    precondition {
      condition     = local.r2_write_group != null
      error_message = "The permission group Workers R2 Storage Bucket Item Write was not found for this account."
    }
  }

  depends_on = [cloudflare_r2_bucket.cache]
}
