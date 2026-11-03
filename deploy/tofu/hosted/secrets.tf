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
