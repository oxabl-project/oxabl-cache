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

run "caddy_with_several_nodes_is_refused" {
  command = plan

  variables {
    mode           = "cluster"
    api_node_count = 2
  }

  expect_failures = [terraform_data.guard]
}

run "certificate_mode_without_a_certificate_is_refused" {
  command = plan

  variables {
    tls_mode = "certificate"
  }

  expect_failures = [terraform_data.guard]
}

run "an_object_store_is_required_in_cluster_mode" {
  command = plan

  variables {
    mode                = "cluster"
    api_node_count      = 2
    tls_mode            = "certificate"
    tls_certificate_pem = "x"
    tls_private_key_pem = "x"
    store_kind          = "fs"
  }

  expect_failures = [terraform_data.guard]
}
