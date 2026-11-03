data "hcloud_server_type" "api" {
  name = var.server_type
}

data "hcloud_server_type" "database" {
  name = coalesce(var.db_server_type, var.server_type)
}

resource "hcloud_server" "api" {
  count = var.api_node_count

  name         = "${var.name}-api-${count.index + 1}"
  server_type  = data.hcloud_server_type.api.name
  image        = "ubuntu-24.04"
  location     = var.location
  ssh_keys     = hcloud_ssh_key.operator[*].id
  firewall_ids = [hcloud_firewall.public.id]
  backups      = var.server_backups
  labels       = merge(local.labels, { role = "api" })

  public_net {
    ipv4_enabled = true
    ipv6_enabled = true
  }

  network {
    network_id = hcloud_network.private.id
    ip         = local.api_private_ips[count.index]
  }

  user_data = templatefile("${path.module}/templates/cloud-init.yaml.tftpl", {
    files = local.api_files[count.index]
  })

  lifecycle {
    ignore_changes = [user_data]
  }

  depends_on = [hcloud_network_subnet.private, terraform_data.guard]
}

resource "hcloud_server" "database" {
  count = local.single ? 0 : 1

  name         = "${var.name}-db"
  server_type  = data.hcloud_server_type.database.name
  image        = "ubuntu-24.04"
  location     = var.location
  ssh_keys     = hcloud_ssh_key.operator[*].id
  firewall_ids = [hcloud_firewall.database[0].id]
  backups      = var.server_backups
  labels       = merge(local.labels, { role = "database" })

  public_net {
    ipv4_enabled = true
    ipv6_enabled = true
  }

  network {
    network_id = hcloud_network.private.id
    ip         = local.db_private_ip
  }

  user_data = templatefile("${path.module}/templates/cloud-init.yaml.tftpl", {
    files = local.database_files
  })

  lifecycle {
    ignore_changes = [user_data]
  }

  depends_on = [hcloud_network_subnet.private, terraform_data.guard]
}

resource "hcloud_volume" "data" {
  name      = "${var.name}-data"
  size      = var.volume_size_gb
  format    = "ext4"
  server_id = local.single ? hcloud_server.api[0].id : hcloud_server.database[0].id
  labels    = local.labels
}
