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

run "a_local_object_store_needs_no_cloudflare_resources" {
  command = apply

  variables {
    store_kind            = "fs"
    cloudflare_account_id = ""
  }

  assert {
    condition     = length(cloudflare_r2_bucket.cache) == 0 && length(cloudflare_account_token.r2) == 0
    error_message = "no R2 bucket or token for a local store"
  }

  assert {
    condition     = strcontains(base64decode(regex("path: /etc/oxabl-cache/server.toml\n    permissions: \"0644\"\n    owner: root:root\n    encoding: b64\n    content: (\\S+)", hcloud_server.api[0].user_data)[0]), "kind = \"fs\"")
    error_message = "the config names the local store"
  }

  assert {
    condition     = !strcontains(hcloud_server.api[0].user_data, "path: /etc/oxabl-cache/secrets/s3-access-key-id")
    error_message = "no S3 credential file for a local store"
  }
}
