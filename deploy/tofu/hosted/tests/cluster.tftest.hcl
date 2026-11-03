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

run "cluster_has_two_api_nodes_and_a_private_database" {
  command = apply

  variables {
    mode                = "cluster"
    api_node_count      = 2
    tls_mode            = "certificate"
    tls_certificate_pem = "-----BEGIN CERTIFICATE-----\nexample\n-----END CERTIFICATE-----\n"
    tls_private_key_pem = "-----BEGIN PRIVATE KEY-----\nexample\n-----END PRIVATE KEY-----\n"
    floating_ip         = true
  }

  assert {
    condition     = length(hcloud_server.api) == 2 && length(hcloud_server.database) == 1
    error_message = "cluster mode is two API nodes and a database node"
  }

  assert {
    condition     = length(hcloud_firewall.database[0].rule) == 1 && alltrue([for rule in hcloud_firewall.database[0].rule : rule.port == "22"])
    error_message = "the database node accepts SSH only, from the operator ranges"
  }

  assert {
    condition     = strcontains(base64decode(regex("path: /etc/containers/systemd/postgres.container\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.database[0].user_data)[0]), "listen_addresses=10.0.0.5")
    error_message = "Postgres listens on the private address only"
  }

  assert {
    condition = alltrue([
      for index in [0, 1] : strcontains(hcloud_server.api[index].user_data, "path: /etc/oxabl-cache/secrets/db-password\n    permissions: \"0600\"")
    ])
    error_message = "the database password is a mode 0600 file on every API node"
  }
  assert {
    condition     = strcontains(base64decode(regex("path: /etc/containers/systemd/postgres.container\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.database[0].user_data)[0]), "ssl=on") && strcontains(hcloud_server.database[0].user_data, "path: /etc/oxabl-cache/secrets/db-server.key\n    permissions: \"0600\"")
    error_message = "Postgres serves TLS and its key is a mode 0600 file"
  }

  assert {
    condition     = strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "ca_file = \"/run/secrets/oxabl-cache/db-ca.pem\"")
    error_message = "API nodes verify the database with the generated CA"
  }
}
