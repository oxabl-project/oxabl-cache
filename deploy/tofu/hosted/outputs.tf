output "public_ipv4" {
  description = "Address the domain's A record points at: the floating IP when allocated, else the first API node."
  value       = var.floating_ip ? hcloud_floating_ip.public[0].ip_address : hcloud_server.api[0].ipv4_address
}

output "api_ipv4_addresses" {
  description = "Public IPv4 address of every API node."
  value       = hcloud_server.api[*].ipv4_address
}

output "api_ipv6_addresses" {
  description = "Public IPv6 address of every API node."
  value       = hcloud_server.api[*].ipv6_address
}

output "database_private_ip" {
  description = "Private address of the Postgres node (cluster mode)."
  value       = local.single ? null : local.db_private_ip
}

output "public_url" {
  description = "URL clients use."
  value       = "https://${var.domain}"
}

output "r2_endpoint" {
  description = "S3 endpoint of the bucket."
  value       = local.s3 ? local.r2_endpoint : null
}

output "r2_bucket" {
  description = "Name of the bucket."
  value       = local.s3 ? var.r2_bucket_name : null
}

output "next_steps" {
  description = "What to do after apply."
  value       = <<-EOT
    1. Create the DNS record: ${var.domain} A ${var.floating_ip ? hcloud_floating_ip.public[0].ip_address : hcloud_server.api[0].ipv4_address}
    2. ssh root@${hcloud_server.api[0].ipv4_address} (from an allowed range) and wait for cloud-init: cloud-init status --wait
    3. Run oxabl-cache-init there once and store the printed operator token.
  EOT
}
