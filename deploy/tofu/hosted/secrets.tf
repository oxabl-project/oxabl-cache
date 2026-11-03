resource "random_id" "url_key" {
  byte_length = 32
}

resource "random_id" "claim_key" {
  byte_length = 32
}

resource "random_password" "database" {
  count = local.single ? 0 : 1

  length  = 40
  special = false
}

resource "tls_private_key" "database_ca" {
  count = local.single ? 0 : 1

  algorithm   = "ECDSA"
  ecdsa_curve = "P256"
}

resource "tls_self_signed_cert" "database_ca" {
  count = local.single ? 0 : 1

  private_key_pem       = tls_private_key.database_ca[0].private_key_pem
  is_ca_certificate     = true
  validity_period_hours = 87600
  allowed_uses          = ["cert_signing", "crl_signing"]

  subject {
    common_name = "${var.name} database CA"
  }
}

resource "tls_private_key" "database" {
  count = local.single ? 0 : 1

  algorithm   = "ECDSA"
  ecdsa_curve = "P256"
}

resource "tls_cert_request" "database" {
  count = local.single ? 0 : 1

  private_key_pem = tls_private_key.database[0].private_key_pem
  ip_addresses    = [local.db_private_ip]

  subject {
    common_name = "${var.name}-db"
  }
}

resource "tls_locally_signed_cert" "database" {
  count = local.single ? 0 : 1

  cert_request_pem      = tls_cert_request.database[0].cert_request_pem
  ca_private_key_pem    = tls_private_key.database_ca[0].private_key_pem
  ca_cert_pem           = tls_self_signed_cert.database_ca[0].cert_pem
  validity_period_hours = 87600
  allowed_uses          = ["server_auth", "digital_signature", "key_encipherment"]
}
