# Deploy the hosted service

The operator's step list for one region of the hosted service, in the shape provider-analysis.md recommends
(section 6): a Cloudflare R2 bucket for the bytes and Hetzner Cloud for compute, with OpenTofu in
`deploy/tofu/hosted`. This guide builds the **prototype** (one small node, SQLite on a volume, one R2 bucket,
well under 25 USD a month) and says what changes for a production region.

Nothing here is applied for you: `tofu plan` shows what would be created, and the operator runs `apply`.

## What gets created

| Resource | Prototype | Region (cluster mode) |
| --- | --- | --- |
| R2 bucket (and an access token scoped to it) | 1 | 1 per data region (EU jurisdiction for EU tenants) |
| Hetzner server, Ubuntu 24.04 | 1 (API, SQLite, Caddy) | `api_node_count` API nodes (2 or more) and 1 Postgres node |
| Hetzner volume (ext4) | 10 GB, mounted at `/var/lib/oxabl-cache` | `volume_size_gb` on the Postgres node |
| Hetzner firewall | 80 and 443 public, 22 from `ssh_allowed_cidrs`, ICMP | the same on API nodes; SSH only on the Postgres node |
| Private network | yes | the API and Postgres nodes talk over it |
| Floating IPv4 | optional | optional, assigned to the first API node |
| DNS record | left to the operator (the address is an output) | same |

Admin (9091), metrics (9090) and Postgres (5432) are never in a public firewall rule and never bind a public
address. A test (`tofu test`) enforces the firewall rules and that every secret is written as a mode 0600 file.

Each server's cloud-init installs Podman, writes the config, the secret files and Quadlet units, mounts the
volume, and starts the containers (the server, and Caddy or Postgres). Two helper commands land in
`/usr/local/sbin`: `oxabl-cache-init` and `oxabl-cache-admin`. Config changes after creation are **not** applied
by `tofu apply` (the servers ignore `user_data` changes, so an edit never replaces a server that holds data);
change `/etc/oxabl-cache/server.toml` on the node and restart `oxabl-cache`, or taint and rebuild a stateless
API node.

## Accounts and tokens to create by hand

Do these once, and put every value in a secret manager. None of them belongs in the repository, a `.tfvars`
file or a chat.

### 1. Cloudflare

1. Create or use a Cloudflare account and enable **R2** (R2 may ask for a payment method on file even for the
   free tier). Note the **Account ID** (R2 overview page).
2. Create the token OpenTofu uses: **Manage Account, Account API Tokens, Create Token**, with these permissions
   and nothing else:
   - Account, **Workers R2 Storage**, **Edit** (creates the bucket);
   - Account, **Account API Tokens**, **Edit** (creates the bucket-scoped object token; skip it if you create
     that token by hand below).

   Export it as `CLOUDFLARE_API_TOKEN` in the shell that runs `tofu`.
3. Set a billing notification on the account.

The bucket-scoped token the servers use is created by OpenTofu with one policy: the permission group
**Workers R2 Storage Bucket Item Write** ("Object Read & Write") on exactly the resource
`com.cloudflare.edge.r2.bucket.<account id>_<jurisdiction>_<bucket>`. The server needs only put, get, head, delete
and list on that one bucket. Its S3 access key id is the token id and its secret is the SHA-256 of the token value.
This path has not been applied against the live API; if the first plan or apply fails on the token, do the manual
alternative: create the token in **R2, Manage API tokens, Create Account API token** with **Object Read & Write**
restricted to the one bucket, and pass `create_r2_token = false` with `r2_access_key_id` and
`r2_secret_access_key` (as `TF_VAR_` environment variables, never in a file).

### 2. Hetzner

1. Create a Hetzner account and a Cloud **project** for the service.
2. **Security, API tokens, Generate API token** with **Read & Write**. Export it as `HCLOUD_TOKEN`.
3. Have an SSH key pair; its public key goes in `ssh_public_keys`. Hetzner has no hard spending cap, so set a
   budget reminder.

### 3. State

Local state is the default and holds the generated keys and the R2 secret in plain text. Encrypt it. OpenTofu
encrypts state and plans natively; set a passphrase method in the environment (not a file):

```sh
export TF_ENCRYPTION='key_provider "pbkdf2" "main" { passphrase = "…" }
method "aes_gcm" "main" { keys = key_provider.pbkdf2.main }
state { method = method.aes_gcm.main }
plan { method = method.aes_gcm.main }'
```

For a remote state, copy `backend.tf.example` to `backend.tf` and `backend.hcl.example` to `backend.hcl` in the
example directory (both are ignored by git) and run `tofu init -backend-config=backend.hcl`. Anyone with Hetzner
project access can also read a server's `user_data`, which carries the same secrets: treat project access as
secret-holding and rotate the keys if it leaks.

## Steps

```sh
cd deploy/tofu/hosted/examples/prototype
cp terraform.tfvars.example terraform.tfvars      # ignored by git; edit: domain, account id, SSH key, your IP range
hcloud server-type list                           # pick the cheapest type with at least 2 vCPU and 4 GB; set server_type

tofu init
tofu plan -out plan.tfplan                        # read it: one server, one volume, one firewall, one bucket, one token
tofu apply plan.tfplan
```

Outputs give `public_ipv4` and the next steps.

1. **DNS.** Create `A cache-proto.example.com -> <public_ipv4>` (and `AAAA` if you want IPv6; the module outputs
   `api_ipv6_addresses`). DNS stays with the operator so any provider works. Use a short TTL for the first day.
2. **Wait for the node.** `ssh root@<public_ipv4>` (from an allowed range), then `cloud-init status --wait`.
   The bootstrap log is `/var/log/oxabl-cache-bootstrap.log`.
3. **TLS.** In the default `tls_mode = "caddy"`, Caddy obtains a Let's Encrypt certificate for the domain as soon
   as the DNS record resolves (HTTP-01 on port 80, which the firewall allows). Check
   `journalctl -u caddy` and `curl -sI https://cache-proto.example.com/`. For **built-in rustls** with your own
   certificate, set `tls_mode = "certificate"` with `tls_certificate_pem` and `tls_private_key_pem` (as
   `TF_VAR_` variables); the server then listens on 443 itself, TLS 1.3 only. Several API nodes need this mode,
   because each Caddy would request its own certificate.
4. **First operator credential.** On the node, once:

   ```sh
   oxabl-cache-init
   ```

   It prints the operator token on stdout, once, and refuses a second time. Store it in a secret manager. To use
   the admin helper on the node, save it root-only: `install -m 0600 /dev/null /etc/oxabl-cache/operator-token`,
   then paste it into that file.
5. **Check the service.** On the node, with the token in `$TOKEN`:
   `curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9091/admin/v1/health` answers
   `{"gc_leader":true,"metadata":"ok","store":"ok"}`. With the built-in TLS mode the admin listener is TLS too:
   `curl --resolve cache-proto.example.com:9091:127.0.0.1 https://cache-proto.example.com:9091/admin/v1/health`.
   A request with no token must answer 401, and from outside the node ports 9090 and 9091 must be closed.
6. **First tenant.** From operations.md, "Tenant administration", on the node:

   ```sh
   oxabl-cache-admin tenant create acme --hard-quota 20GiB
   oxabl-cache-admin token create --tenant <t> --scopes tenant:admin --expires 30d
   ```

   Hand the tenant admin token to the customer through a secure channel. They create caches, tokens and OIDC
   policies themselves (protocol.md). A region is its own deployment, so there is no region flag: the helper
   points at this node's admin listener (`OXABL_CACHE_ADMIN_SERVER`) and reads the token from
   `/etc/oxabl-cache/operator-token`.
7. **Backups.** The metadata is what matters; packs are reproducible. On the single-node prototype, run
   `oxabl-cache-server backup --to <file>` on a timer and copy the file off the server; `server_backups = true`
   (adds 20% to the server) or a volume snapshot from the Hetzner console before risky changes also work. Production uses Postgres base backups and WAL
   archiving (operations.md, "Backup and restore").

## A production region

`examples/region` is the same module in `cluster` mode: two API nodes behind a floating IP, a Postgres node with
a volume, an R2 bucket per data region (`r2_jurisdiction = "eu"` and `location = "fsn1"` for the EU), and
`tls_mode = "certificate"`. The module generates a private CA and a server certificate for Postgres, which is
served over TLS 1.3 on the private network only. The module does not provision a Postgres replica, WAL archiving or failover of the
floating IP between API nodes; plan those as follow-up work. Run `tofu plan` for each region in its own state.

## Cost

From provider-analysis.md (prices read 2026-11-02; re-read before committing):

| Item | Per month |
| --- | --- |
| Prototype: one Hetzner Cloud VM, 2 vCPU and 4 GB | about EUR 6.49 |
| Prototype: R2 bucket under the free tier (10 GB, 1M class A, 10M class B) | 0 |
| Prototype: a 10 GB volume and the server's IPv4 address | cents to about EUR 1 (not in provider-analysis; check the Hetzner price list) |
| Prototype total | about 8 to 9 USD, against a 25 USD ceiling |
| Hosted stack with 1 / 10 / 100 / 500 tenants (R2 plus Hetzner, packed, presigned URLs) | 11 / 12 / 40 / 99 USD |

The 11 USD at one tenant includes a larger node and volume than the prototype. Egress from R2 is free; object
requests stay near 22,000 a tenant-month because packs are read by range. A floating IP and server backups cost
extra.

## Teardown

1. Export or hand over any tenant data that matters. Packs are reproducible; metadata is not.
2. Empty the bucket (the R2 dashboard, or `aws s3 rm s3://<bucket> --recursive --endpoint-url https://<account
   id>.r2.cloudflarestorage.com`). `tofu destroy` cannot delete a non-empty bucket.
3. `tofu destroy` from the example directory. It removes the servers, the volume, the firewall, the network, the
   floating IP, the bucket and the R2 token. The volume holds the database: destroying it is the point of
   teardown, so confirm the plan lists what you expect.
4. Delete the DNS record. Revoke the Cloudflare and Hetzner API tokens you created, and the SSH key if it was only
   for this.
5. Check the next invoices from both providers are zero.
