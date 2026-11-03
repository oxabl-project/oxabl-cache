# shellcheck shell=bash
# Starts a TLS Postgres and a Garage S3 store for the smoke scripts. Source it, then call
# `start_backends <work> <bind> <host>`: work is an empty directory, bind the address both
# listen on, host the address clients use (the Postgres certificate names it). The ports land
# in "$work/pg-port" and "$work/s3-port"; "$work/secrets" gets db-password, db-ca.pem,
# s3-access-key-id, s3-secret-access-key, url-key and claim-key. `stop_backends` kills both.
# Needs garage, postgres (initdb, createdb, pg_isready) and openssl.

free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'; }

pids=()
stop_backends() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
}

start_backends() {
  local work="$1" bind="$2" host="$3" pg_port s3_port rpc_port
  mkdir -p "$work"/{pg,garage/meta,garage/data,secrets}
  pg_port="$(free_port)"
  s3_port="$(free_port)"
  rpc_port="$(free_port)"
  printf '%s' "$pg_port" > "$work/pg-port"
  printf '%s' "$s3_port" > "$work/s3-port"

  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 -subj /CN=smoke-ca \
    -keyout "$work/ca.key" -out "$work/secrets/db-ca.pem" 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -subj /CN=smoke-db \
    -keyout "$work/pg/server.key" -out "$work/server.csr" 2>/dev/null
  printf 'subjectAltName=IP:%s\n' "$host" > "$work/san.ext"
  openssl x509 -req -in "$work/server.csr" -CA "$work/secrets/db-ca.pem" -CAkey "$work/ca.key" -CAcreateserial \
    -days 2 -extfile "$work/san.ext" -out "$work/pg/server.crt" 2>/dev/null
  chmod 0600 "$work/pg/server.key"
  printf 'smoke-password' > "$work/secrets/db-password"
  initdb -D "$work/pg/data" -U oxabl_cache --auth-local=scram-sha-256 --auth-host=scram-sha-256 \
    --pwfile="$work/secrets/db-password" >/dev/null
  printf 'hostssl all all all scram-sha-256\n' >> "$work/pg/data/pg_hba.conf"
  postgres -D "$work/pg/data" -p "$pg_port" -k "$work/pg" -c listen_addresses="$bind" \
    -c ssl=on -c ssl_cert_file="$work/pg/server.crt" -c ssl_key_file="$work/pg/server.key" \
    >"$work/pg/log" 2>&1 &
  pids+=($!)
  for _ in $(seq 1 100); do pg_isready -q -h "$host" -p "$pg_port" && break; sleep 0.1; done
  PGPASSWORD="$(cat "$work/secrets/db-password")" createdb -h "$host" -p "$pg_port" -U oxabl_cache oxabl_cache

  cat > "$work/garage/garage.toml" <<EOF
metadata_dir = "$work/garage/meta"
data_dir = "$work/garage/data"
db_engine = "sqlite"
replication_factor = 1
rpc_bind_addr = "127.0.0.1:$rpc_port"
rpc_public_addr = "127.0.0.1:$rpc_port"
rpc_secret = "$(openssl rand -hex 32)"
[s3_api]
s3_region = "garage"
api_bind_addr = "$bind:$s3_port"
EOF
  garage -c "$work/garage/garage.toml" server >"$work/garage/log" 2>&1 &
  pids+=($!)
  for _ in $(seq 1 100); do garage -c "$work/garage/garage.toml" status >/dev/null 2>&1 && break; sleep 0.2; done
  local node key
  node="$(garage -c "$work/garage/garage.toml" node id -q | cut -d@ -f1)"
  garage -c "$work/garage/garage.toml" layout assign -z dc1 -c 1G "$node" >/dev/null
  garage -c "$work/garage/garage.toml" layout apply --version 1 >/dev/null
  garage -c "$work/garage/garage.toml" bucket create oxabl-cache >/dev/null
  key="$(garage -c "$work/garage/garage.toml" key create smoke)"
  printf '%s' "$(sed -n 's/^Key ID: *//p' <<< "$key")" > "$work/secrets/s3-access-key-id"
  printf '%s' "$(sed -n 's/^Secret key: *//p' <<< "$key")" > "$work/secrets/s3-secret-access-key"
  garage -c "$work/garage/garage.toml" bucket allow --read --write --owner oxabl-cache --key smoke >/dev/null

  openssl rand -hex 32 > "$work/secrets/url-key"
  openssl rand -hex 32 > "$work/secrets/claim-key"
  chmod 0600 "$work"/secrets/*
}
