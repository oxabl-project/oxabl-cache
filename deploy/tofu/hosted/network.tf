resource "hcloud_ssh_key" "operator" {
  count = length(var.ssh_public_keys)

  name       = "${var.name}-${count.index}"
  public_key = var.ssh_public_keys[count.index]
  labels     = local.labels
}

resource "hcloud_network" "private" {
  name     = var.name
  ip_range = local.network_range
  labels   = local.labels
}

resource "hcloud_network_subnet" "private" {
  network_id   = hcloud_network.private.id
  type         = "cloud"
  network_zone = local.network_zone
  ip_range     = local.subnet_range
}

resource "hcloud_firewall" "public" {
  name   = "${var.name}-public"
  labels = local.labels

  rule {
    description = "HTTP, for certificate issuance and redirects"
    direction   = "in"
    protocol    = "tcp"
    port        = "80"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  rule {
    description = "HTTPS, the public listener"
    direction   = "in"
    protocol    = "tcp"
    port        = "443"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  rule {
    description = "ICMP"
    direction   = "in"
    protocol    = "icmp"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  dynamic "rule" {
    for_each = length(var.ssh_allowed_cidrs) > 0 ? [1] : []
    content {
      description = "SSH from the operator ranges"
      direction   = "in"
      protocol    = "tcp"
      port        = "22"
      source_ips  = var.ssh_allowed_cidrs
    }
  }
}

resource "hcloud_floating_ip" "public" {
  count = var.floating_ip ? 1 : 0

  name          = var.name
  type          = "ipv4"
  home_location = var.location
  labels        = local.labels
}

resource "hcloud_floating_ip_assignment" "public" {
  count = var.floating_ip ? 1 : 0

  floating_ip_id = hcloud_floating_ip.public[0].id
  server_id      = hcloud_server.api[0].id
}

resource "hcloud_firewall" "database" {
  count = local.single ? 0 : 1

  name   = "${var.name}-database"
  labels = local.labels

  dynamic "rule" {
    for_each = length(var.ssh_allowed_cidrs) > 0 ? [1] : []
    content {
      description = "SSH from the operator ranges"
      direction   = "in"
      protocol    = "tcp"
      port        = "22"
      source_ips  = var.ssh_allowed_cidrs
    }
  }
}
