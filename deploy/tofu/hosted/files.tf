locals {
  data_mount_single = "/var/lib/oxabl-cache"
  data_mount_db     = "/var/lib/oxabl-cache-pg"

  issuers = join(", ", [for issuer in var.oidc_issuer_allowlist : jsonencode(issuer)])

  secret_files = merge(
    {
      "url-key"   = "${random_id.url_key.hex}\n"
      "claim-key" = "${random_id.claim_key.hex}\n"
    },
    local.s3 ? {
      "s3-access-key-id"     = "${local.s3_access_key_id}\n"
      "s3-secret-access-key" = "${local.s3_secret_access_key}\n"
    } : {},
    local.single ? {} : {
      "db-password" = "${random_password.database[0].result}\n"
    },
    var.tls_mode == "certificate" ? {
      "tls.crt" = var.tls_certificate_pem
      "tls.key" = var.tls_private_key_pem
    } : {},
  )

  api_files = [
    for index in range(var.api_node_count) : concat(
      [
        {
          path        = "/etc/oxabl-cache/server.toml"
          permissions = "0644"
          content = templatefile("${path.module}/templates/server.toml.tftpl", {
            listen          = var.tls_mode == "certificate" ? "0.0.0.0:443" : "127.0.0.1:8443"
            metrics_listen  = local.single ? "127.0.0.1:9090" : "${local.api_private_ips[index]}:9090"
            domain          = var.domain
            tls_certificate = var.tls_mode == "certificate"
            single          = local.single
            database_host   = local.db_private_ip
            s3              = local.s3
            r2_endpoint     = local.r2_endpoint
            r2_bucket       = var.r2_bucket_name
            retention_days  = var.default_retention_days
            hard_quota      = var.default_hard_quota
            rate            = var.default_rate
            burst           = var.default_burst
            issuers         = local.issuers
            log_level       = var.log_level
          })
        },
        {
          path        = "/etc/containers/systemd/oxabl-cache.container"
          permissions = "0644"
          content = templatefile("${path.module}/templates/oxabl-cache.container.tftpl", {
            image      = var.image
            data_mount = local.single ? local.data_mount_single : ""
          })
        },
        {
          path        = "/usr/local/sbin/oxabl-cache-init"
          permissions = "0750"
          content = templatefile("${path.module}/templates/helper.sh.tftpl", {
            description = "Creates the schema and prints the first operator token once."
            image       = var.image
            data_mount  = local.single ? local.data_mount_single : ""
            entrypoint  = ""
          })
        },
        {
          path        = "/usr/local/sbin/oxabl-cache-admin"
          permissions = "0750"
          content = templatefile("${path.module}/templates/helper.sh.tftpl", {
            description = "Runs oxabl-cache-admin against this node; --token-file /run/secrets/operator-token reads /etc/oxabl-cache/operator-token."
            image       = var.image
            data_mount  = ""
            entrypoint  = "/usr/local/bin/oxabl-cache-admin"
          })
        },
        {
          path        = "/usr/local/sbin/oxabl-cache-bootstrap"
          permissions = "0750"
          content = templatefile("${path.module}/templates/bootstrap.sh.tftpl", {
            role           = "api"
            data_mount     = local.single ? local.data_mount_single : ""
            bind_low_ports = var.tls_mode == "certificate"
            floating_ip    = var.floating_ip && index == 0 ? hcloud_floating_ip.public[0].ip_address : ""
            units          = var.tls_mode == "caddy" ? ["oxabl-cache", "caddy"] : ["oxabl-cache"]
          })
        },
      ],
      [for name, content in local.secret_files : {
        path        = "/etc/oxabl-cache/secrets/${name}"
        permissions = "0600"
        content     = content
      }],
      var.tls_mode == "caddy" ? [
        {
          path        = "/etc/caddy/Caddyfile"
          permissions = "0644"
          content     = templatefile("${path.module}/templates/Caddyfile.tftpl", { domain = var.domain })
        },
        {
          path        = "/etc/containers/systemd/caddy.container"
          permissions = "0644"
          content     = templatefile("${path.module}/templates/caddy.container.tftpl", { image = var.caddy_image })
        },
      ] : [],
    )
  ]

  database_files = local.single ? [] : [
    {
      path        = "/etc/oxabl-cache/secrets/db-password"
      permissions = "0600"
      content     = "${random_password.database[0].result}\n"
    },
    {
      path        = "/etc/containers/systemd/postgres.container"
      permissions = "0644"
      content = templatefile("${path.module}/templates/postgres.container.tftpl", {
        image      = var.postgres_image
        data_mount = local.data_mount_db
        private_ip = local.db_private_ip
      })
    },
    {
      path        = "/usr/local/sbin/oxabl-cache-bootstrap"
      permissions = "0750"
      content = templatefile("${path.module}/templates/bootstrap.sh.tftpl", {
        role           = "database"
        data_mount     = local.data_mount_db
        bind_low_ports = false
        floating_ip    = ""
        units          = ["postgres"]
      })
    },
  ]
}
