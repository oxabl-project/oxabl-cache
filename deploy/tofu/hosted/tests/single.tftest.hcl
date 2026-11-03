mock_provider "hcloud" {
  mock_resource "hcloud_network" {
    defaults = {
      id = "101"
    }
  }

  mock_resource "hcloud_ssh_key" {
    defaults = {
      id = "102"
    }
  }

  mock_resource "hcloud_firewall" {
    defaults = {
      id = "103"
    }
  }

  mock_resource "hcloud_server" {
    defaults = {
      id = "104"
    }
  }

  mock_resource "hcloud_volume" {
    defaults = {
      id = "105"
    }
  }

  mock_resource "hcloud_floating_ip" {
    defaults = {
      id = "106"
    }
  }
}

mock_provider "cloudflare" {
  mock_data "cloudflare_account_api_token_permission_groups_list" {
    defaults = {
      result = [{
        id            = "00000000000000000000000000000001"
        name          = "Workers R2 Storage Bucket Item Write"
        category      = "r2"
        is_selectable = true
        scopes        = ["com.cloudflare.api.account"]
      }]
    }
  }
}

variables {
  domain                = "cache.example.com"
  server_type           = "cx23"
  ssh_public_keys       = ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleExampleExampleExampleExampleExample operator@example.com"]
  ssh_allowed_cidrs     = ["203.0.113.7/32"]
  cloudflare_account_id = "0123456789abcdef0123456789abcdef"
}

run "prototype_renders_a_locked_down_single_node" {
  command = apply

  assert {
    condition     = length(hcloud_server.api) == 1 && length(hcloud_server.database) == 0
    error_message = "single mode is one API node and no database node"
  }

  assert {
    condition = alltrue([
      for rule in hcloud_firewall.public.rule : contains(["80", "443", "22", "icmp"], rule.port == null ? "icmp" : rule.port)
    ])
    error_message = "the public firewall opens only 80, 443 and 22"
  }

  assert {
    condition     = !anytrue([for rule in hcloud_firewall.public.rule : contains(["9090", "9091", "5432"], rule.port == null ? "icmp" : rule.port)])
    error_message = "admin, metrics and Postgres ports are never public"
  }

  assert {
    condition     = alltrue([for rule in hcloud_firewall.public.rule : rule.port != "22" || rule.source_ips == toset(["203.0.113.7/32"])])
    error_message = "SSH is open to the operator ranges only"
  }

  assert {
    condition = alltrue([
      for name in ["url-key", "claim-key", "s3-access-key-id", "s3-secret-access-key"] :
      strcontains(hcloud_server.api[0].user_data, "path: /etc/oxabl-cache/secrets/${name}\n    permissions: \"0600\"")
    ])
    error_message = "every secret is written as a file with mode 0600"
  }

  assert {
    condition = (
      strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "kind = \"s3\"")
      && strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "url = \"sqlite:///var/lib/oxabl-cache/meta.db\"")
      && strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "listen = \"127.0.0.1:8443\"")
    )
    error_message = "the config names R2, SQLite and a loopback listener behind Caddy"
  }

  assert {
    condition     = !strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "access_key_id = ")
    error_message = "the config holds no inline secret"
  }

  assert {
    condition     = length(cloudflare_account_token.r2) == 1 && strcontains(cloudflare_account_token.r2[0].policies[0].resources, "com.cloudflare.edge.r2.bucket.0123456789abcdef0123456789abcdef_default_oxabl-cache")
    error_message = "the R2 token is scoped to the one bucket"
  }
}
