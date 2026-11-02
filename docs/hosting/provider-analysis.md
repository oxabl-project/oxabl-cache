# Hosting provider analysis

Where to run the paid oxabl-cache service, decided on numbers. Prices were read from public provider pages and price-list APIs on **2026-11-02**; the sources are in the [appendix](#appendix-sources). Anything not read from a source is marked **(est.)** (my estimate) or **(unverified)** and is collected in [Open items](#7-open-items). Amounts are USD per month unless stated. EUR is converted at an assumed 1.10 USD per EUR.

The model is parameterised and independent of the architecture. It assumes only an object store with the plain S3 API, a small stateless API, a metadata database, and a choice between bytes flowing client to bucket (presigned URLs) or client to API to bucket (proxy).

## 1. Workload model

### Parameters (per tenant, per month)

| Parameter | Base | Low | High |
| --- | --- | --- | --- |
| Artifacts per full set | 10,000 | | |
| Compressed full set | 0.9 GB (90 KB per artifact) | | |
| Stored per tenant (generations and churn, before GC) | **5 GB** | 2 GB | 15 GB |
| CI builds | 200 | | |
| Changed artifacts per build | 200 (range 50 to 500) | | |
| Full rebuilds (toolchain or schema change) | 4 | | |
| Developers | 15 | 5 | 30 |
| Cold full fetches per developer | 3 | 1 | 4 |
| CI builds on a cold runner (fetch the full set) | 25% | 10% | 50% |
| Incremental fetches per developer | 40 (two per working day) | | |
| Artifacts per incremental fetch | 200 (18 MB) | | |
| Packed layout | 72 packs of 12.5 MB per full set | | |

### Arithmetic

Uploads (ingress is free at every provider read):
- CI: 200 builds × 200 artifacts × 90 KB = 3.6 GB.
- Full rebuilds: 4 × 0.9 GB = 3.6 GB.
- About 7.2 GB per tenant-month. With first-write-wins, an artifact already present is not uploaded again, so this is an upper bound. About 4 GB of new data a month kept for a 30-day retention window, plus the 0.9 GB base, gives the 5 GB stored figure.

Egress:
- Cold fetches: developers 15 × 3 = 45, CI 200 × 25% = 50; 95 × 0.9 GB = 85.5 GB.
- Incremental fetches: CI 150 and developers 15 × 40 = 600; 750 × 18 MB = 13.5 GB.
- **About 99 GB per tenant-month, 20 times the stored bytes.** Low 29 GB, high 221 GB.

Requests, **unpacked** (one object per artifact):
- Class A (PUT): 200 × 200 + 4 × 10,000 = 80,000.
- Class B (GET): 95 × 10,000 + 750 × 200 = 1,100,000.
- Existence checks are answered by the metadata database, not by the bucket. DELETE is free at every provider read; GC listing is negligible.

Requests, **packed** (a build uploads its new artifacts as one pack; reads are coalesced range GETs):
- Class A: 200 + 4 × 72 = about 490.
- Class B: 95 × 72 + 750 × 20 = about 21,800.
- Bytes transferred are the same in both layouts.

Scale points: 1, 10, 100, 500 tenants. Egress is 99 GB, 0.99 TB, 9.9 TB, 49.5 TB a month; stored data is 5 GB, 50 GB, 0.5 TB, 2.5 TB.

### Compute and database (same plan for every provider)

S is 2 vCPU with 4 to 8 GB; M is 4 vCPU with 8 to 16 GB. From 100 tenants up, Postgres runs on its own node of the same size, plus a block volume of 50 GB (100 tenants) or 100 GB (500 tenants); below that the database shares the API node with a 20 GB volume. A load balancer is added from 100 tenants where the provider charges for one.

| Tenants | Presigned URLs (bytes bypass the API) | Proxy (bytes through the API) |
| --- | --- | --- |
| 1, 10 | 1 × S (API and DB together) | 1 × S |
| 100 | 2 × S API + 1 × S DB | 3 × S API + 1 × S DB |
| 500 | 2 × M API + 1 × M DB | 3 × M API + 1 × M DB |

Proxy sizing: 50 TB a month is 154 Mbit/s on average and about 1.5 Gbit/s at a 10× peak, which three M nodes with 5 Gbit/s or more carry.

Not modelled: CDN or cache layers, cross-region replication, database backups (small, to the bucket), monitoring, support, VAT, and operator time. The last favours fewer moving parts.

## 2. Price sheet

Hot tier, North American or EU region. Request prices are per million.

### Object storage

| Provider | Storage $/GB-mo | Egress $/GB | Class A / Class B per M | Minimum duration or size | Free allowance |
| --- | --- | --- | --- | --- | --- |
| Cloudflare R2 Standard | 0.015 | **0** | 4.50 / 0.36; deletes free | none (30 days on Infrequent Access only) | 10 GB, 1M A, 10M B |
| AWS S3 Standard (us-east-1) | 0.023 | 0.09 to 10 TB, 0.085 to 50 TB, 0.07 to 150 TB, then 0.05; first 100 GB free | 5.00 / 0.40 | none | |
| Google Cloud Storage Standard (US regions) | 0.020 | 0.12 to 10 TiB, then 0.11 | 5.00 / 0.40 | none | 5 GB |
| Azure Blob Hot LRS (eastus) | 0.0208 | 0.087 after 100 GB, 0.083, 0.07 | 5.00 write / 0.40 read | none | |
| Backblaze B2 | 0.00695 | free up to 3× average stored, then 0.01; free to CDN and compute partners (Cloudflare, Fastly, bunny.net, Vultr and others) | free (A, B, C) | none | 10 GB |
| Wasabi Pay-Go | 0.00799 | 0, subject to fair use | free, subject to fair use | **90 days**; minimum charge on 1 TB active | |
| Hetzner Object Storage (EU only) | base EUR 6.49 per month includes 1 TB stored and 1 TB egress; extra about EUR 6.35 per TB-month (EUR 0.0087 per TB-hour) | extra EUR 1.00 per TB (page text says "per hour"; read as per TB, **unverified**); EU network zone and ingress free | free | 64 KB minimum billable size | quota in base price |
| DigitalOcean Spaces | $5 per month includes 250 GiB, then 0.02 | 1,024 GiB included, then 0.01; free to Droplets in the same region | none listed | Cold tier only | |
| Scaleway Standard Multi-AZ (fr-par) | 0.016 (EUR 0.000022 per GB-hour) | EUR 0.01 | none in the catalog | none on Standard | |
| OVHcloud Standard multi-zone | 0.0157 per GiB (0.0203 High Performance) | **0** | **free** | none on Standard | |
| Vultr Object Storage | **(unverified)** 6 per month with 1 TB stored and 1 TB egress, then 0.01 | **(unverified)** 0.01 | **(unverified)** free | | |
| Tigris (Fly.io) | 0.02 | **0** | 5.00 / 0.50; deletes free | none on Standard | 5 GB, 10k A, 100k B |

### Compute and bandwidth

| Provider | S per month | M per month | Egress from compute |
| --- | --- | --- | --- |
| AWS | t4g.medium (2 vCPU, 4 GiB) 24.5 | c7g.xlarge (4 vCPU, 8 GiB) 105.9 | 0.09 per GB, tiers as S3; EBS gp3 0.08 per GB; ALB 16.4 |
| GCP | **(est.)** 25 | **(est.)** 100 | 0.12 per GB, 0.11 above 10 TiB |
| Azure | D2ps_v5 (2 vCPU, 8 GiB) 56.2 | D4ps_v5 112.4 | 0.087 per GB after 100 GB |
| Hetzner Cloud | **(est.)** EUR 9 (plans start at EUR 5.99 and 6.49) | **(est.)** EUR 17 | 20 TB included per EU server; over-use in 100 MB blocks; rate not retrieved, assumed EUR 1 per TB |
| DigitalOcean | **(est.)** 24 | **(est.)** 48 | **(est.)** 4 TB pooled per Droplet, 0.01 per GiB over |
| Vultr | vc2-2c-4gb 20 (3 TB included) | vc2-4c-8gb 40 (4 TB included) | **(unverified)** 0.01 per GB over |
| Fly.io | 2 × shared-cpu-1x 1 GB 13.4 | 2 × performance-1x 2 GB 66 | 0.02 per GB in NA and EU; volumes 0.15 per GB-month |
| Scaleway | POP2-2C-8G EUR 0.0735 per hour, 59 | POP2-4C-16G EUR 0.147 per hour, 118 | **(est.)** EUR 0.01 per GB |
| OVHcloud | b3-8 (2 vCPU, 8 GB) 44.2, 500 Mbit/s public | b3-16 (4 vCPU, 16 GB) 88.2, 1 Gbit/s public | included |
| Cloudflare | Workers Paid 5 per month plus 0.30 per M requests above 10M | | free |

## 3. Cost tables

All tables use section 1 at 1, 10, 100 and 500 tenants; the base case is 5 GB stored and 99 GB egress per tenant. Figures are rounded to whole dollars.

### 3.1 Object storage only, presigned URLs (storage + egress + requests)

| Provider | Layout | 1 | 10 | 100 | 500 | Main driver |
| --- | --- | --- | --- | --- | --- | --- |
| AWS S3 | unpacked | 1 | 90 | 978 | 4,727 | egress, 0.09 per GB |
| | packed | 0 | 81 | 895 | 4,313 | |
| GCS | unpacked | 13 | 128 | 1,282 | 6,017 | egress, 0.12 per GB |
| | packed | 12 | 120 | 1,199 | 5,603 | |
| Azure Blob | unpacked | 1 | 87 | 947 | 4,613 | egress |
| | packed | 0 | 79 | 864 | 4,199 | |
| Cloudflare R2 | unpacked | 0 | 1 | 75 | 407 | class A and B requests (about 90% of the bill) |
| | packed | 0 | 1 | 7 | 38 | storage |
| Tigris | unpacked | 1 | 10 | 105 | 525 | requests |
| | packed | 0 | 1 | 11 | 56 | storage |
| Backblaze B2 | either | 1 | 9 | 87 | 437 | egress above 3× stored, 0.01 per GB |
| Wasabi | either | 8 | 8 | 11 | 53 | 1 TB minimum charge, then 90-day minimum on churn (see 3.5) |
| Hetzner Object Storage | either | 7 | 7 | 17 | 71 | egress above 1 TB, EUR 1 per TB |
| DigitalOcean Spaces | either | 5 | 5 | 99 | 535 | egress above 1,024 GiB |
| Scaleway | either | 1 | 12 | 118 | 589 | egress, EUR 0.01 per GB |
| OVHcloud | either | 0 | 1 | 8 | 39 | storage |
| Vultr (unverified) | either | 6 | 6 | 95 | 506 | egress |

How R2 is priced at 100 tenants, unpacked: storage (500 GB − 10 GB free) × 0.015 = 7.35; class A (8.0M − 1M free) × 4.50 per M = 31.50; class B (110M − 10M free) × 0.36 per M = 36.00; total about 75. Packed: 7.35 plus about 0.2 of requests.

### 3.2 Compute and database only (see section 1 for the node plan)

| Provider | Presigned: 1 | 10 | 100 | 500 | Proxy: 1 | 10 | 100 | 500 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AWS | 26 | 26 | 94 | 342 | 26 | 106 | 1,000 | 4,698 |
| GCP (est.) | 27 | 27 | 98 | 328 | 39 | 146 | 1,311 | 5,975 |
| Azure | 58 | 58 | 191 | 363 | 58 | 135 | 1,099 | 4,617 |
| Hetzner (est.) | 11 | 11 | 32 | 61 | 11 | 11 | 42 | 80 |
| DigitalOcean (est.) | 26 | 26 | 89 | 166 | 26 | 26 | 113 | 549 |
| Vultr | 22 | 22 | 75 | 140 | 22 | 22 | 95 | 535 |
| Fly.io | 16 | 16 | 48 | 213 | 18 | 36 | 259 | 1,269 |
| Scaleway (est.) | 61 | 61 | 182 | 364 | 62 | 72 | 350 | 1,027 |
| OVHcloud | 44 | 44 | 133 | 266 | 44 | 44 | 177 | 354 |

Proxy mode on a hyperscaler pays the egress rate on every byte (AWS at 500 tenants: about 4,250 of the 4,698). Providers with pooled or free compute bandwidth (Hetzner, OVHcloud, Vultr, DigitalOcean) barely notice it.

### 3.3 Whole stack: object storage + compute + database, packed layout

| Stack | Presigned: 1 | 10 | 100 | 500 | Proxy: 1 | 10 | 100 | 500 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **R2 + Hetzner VMs** | **11** | **12** | **40** | **99** | 11 | 12 | 49 | 117 |
| Tigris + Fly.io | 16 | 17 | 59 | 269 | 18 | 37 | 270 | 1,325 |
| Wasabi + Hetzner VMs | 19 | 19 | 43 | 114 | 19 | 19 | 53 | 133 |
| Hetzner OS + Hetzner VMs | 18 | 18 | 49 | 132 | 18 | 18 | 49 | 97 |
| B2 + Hetzner VMs | 12 | 20 | 120 | 498 | 12 | 20 | 130 | 517 |
| OVHcloud OS + b3 instances | 45 | 45 | 141 | 305 | 45 | 45 | 185 | 393 |
| DigitalOcean Spaces + Droplets (est.) | 31 | 31 | 188 | 701 | 31 | 31 | 123 | 599 |
| Vultr (unverified) | 28 | 28 | 170 | 646 | 28 | 28 | 101 | 556 |
| Scaleway (est.) | 62 | 73 | 300 | 953 | 62 | 73 | 359 | 1,071 |
| AWS S3 + EC2 | 26 | 107 | 989 | 4,655 | 26 | 107 | 1,013 | 4,761 |
| Azure Blob + VMs | 58 | 136 | 1,055 | 4,562 | 58 | 136 | 1,111 | 4,674 |
| GCS + GCE (est.) | 39 | 147 | 1,297 | 5,931 | 39 | 147 | 1,322 | 6,031 |

Same table, unpacked layout, presigned URLs, for the stacks where it changes the ranking:

| Stack | 1 | 10 | 100 | 500 |
| --- | --- | --- | --- | --- |
| R2 + Hetzner VMs | 11 | 12 | 107 | 468 |
| Tigris + Fly.io | 17 | 27 | 152 | 738 |
| Hetzner OS + Hetzner VMs | 18 | 18 | 49 | 132 |
| Wasabi + Hetzner VMs | 19 | 19 | 43 | 114 |
| AWS S3 + EC2 | 27 | 116 | 1,071 | 5,069 |

Per tenant at 500 tenants, packed, presigned: R2 + Hetzner 0.20; Hetzner Object Storage + Hetzner 0.26; B2 + Hetzner 1.00; AWS 9.31.

### 3.4 Sensitivity at 100 tenants, object storage only, packed

Stored per tenant (egress fixed at 99 GB):

| Stored | AWS S3 | R2 | B2 | Hetzner OS | Wasabi | OVHcloud | Tigris | Scaleway | DO Spaces |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2 GB | 888 | 3 | 94 | 17 | 8 | 3 | 5 | 112 | 94 |
| 5 GB | 895 | 7 | 87 | 17 | 11 | 8 | 11 | 118 | 99 |
| 15 GB | 918 | 22 | 64 | 20 | 32 | 24 | 31 | 135 | 119 |

Egress per tenant (stored fixed at 5 GB):

| Egress | AWS S3 | R2 | B2 | Hetzner OS | Wasabi | OVHcloud | Tigris | Scaleway | DO Spaces |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 29 GB (low) | 267 | 7 | 18 | 9 | 11 | 8 | 11 | 41 | 29 |
| 99 GB (base) | 895 | 7 | 87 | 17 | 11 | 8 | 11 | 118 | 99 |
| 221 GB (high) | 1,938 | 7 | 210 | 30 | 11 | 8 | 12 | 252 | 221 |

Zero-egress providers (R2, Tigris, OVHcloud, Wasabi) are flat in egress. B2 and Hetzner scale with it at cents per GB. Hyperscalers scale at 9 to 12 cents per GB.

### 3.5 Wasabi and churn

Wasabi bills every object for at least 90 days. With a 30-day retention window, about 4.1 GB of the 5 GB per tenant is churn that lives 30 days and is billed for 90, so billed storage is about 0.9 + 4.1 × 3 = 13.2 GB, or 2.6× the live bytes. It is still cheap (11 at 100 tenants in table 3.1), but the cost grows with churn and shortens only by lengthening retention. More important is fair use: Wasabi's own page says it dynamically limits concurrent API requests and upload and download resources per account by the storage volume of the account. At 500 tenants the account holds 2.5 TB and serves 50 TB a month, a 20:1 egress-to-storage ratio. The policy page that states the exact ratio was not readable, so treat Wasabi as **unsuitable for primary** until the fair-use limit is confirmed in writing.

## 4. Performance notes

No provider publishes latency or per-object throughput figures that I could read; the numbers to decide on come from the prototype (section 8). What the documents do state:

| Provider | Request and throughput limits read | Notes for this workload |
| --- | --- | --- |
| R2 | one write per second to the same key (429 above); 5 GiB single part, 4.995 TiB multipart; management API 1,200 requests per 5 minutes; r2.dev subdomain is rate limited and not for production | Content-addressed keys are written once, so the same-key limit does not bind. The S3 endpoint, not a Worker, should carry presigned traffic. Region is automatic; EU jurisdiction buckets exist. A "Local uploads" beta is listed in the docs and is not evaluated here. |
| Hetzner Object Storage | 750 requests per second per bucket and per source IP, 10 Gbit/s per bucket, 256 parallel TCP sessions per source IP, 100 buckets, 100 TB and 50M objects per bucket, 5 GB single PUT | EU only (Falkenstein, Nuremberg, Helsinki). North American clients cross the Atlantic. 64 KB billing floor: artifacts under 64 KB are billed as 64 KB. Objects are described as write-once. |
| DigitalOcean Spaces | 800 operations per second per new bucket; older buckets 500, 300 for writes and lists | Low request ceiling; packing needed. |
| AWS S3 | **(from memory, unverified)** 3,500 writes and 5,500 reads per second per prefix | Highest per-prefix rates, lowest latency within AWS. |
| Wasabi | dynamic by account volume | See 3.5. |
| B2, GCS, Azure, Scaleway, OVHcloud, Vultr, Tigris | not retrieved | Measure in the prototype. |

Latency and throughput reasoning (arithmetic, not provider data):

- Round trip is about 80 to 100 ms between the US east coast and Western Europe, and 140 to 160 ms between the US west coast and Western Europe **(est.)**. A client that fetches 10,000 small objects at concurrency 64 needs 10,000 / 64 = about 157 sequential rounds: about 16 s at 100 ms plus the transfer time, and about 1.6 s at 10 ms. Packed, the same cold fetch is 72 range requests, about 2 rounds, so latency stops mattering and the transfer rate decides.
- A 0.9 GB cold fetch at an effective 200 Mbit/s takes 36 s, at 1 Gbit/s 7 s. Both are shorter than a typical compile of a changed set, so the throughput target is a single-digit-second cold fetch from a nearby region.
- Small objects are slow and expensive in two ways: request fees (R2 and Tigris charge 0.36 to 0.50 per million reads and 4.50 to 5.00 per million writes, so 1.1M reads and 80,000 writes a tenant-month cost 0.75 to 1.05 against 0.07 to 0.11 packed) and per-request latency. Packing wins on both. Ranged reads of 5 to 20 MB packs keep requests at about 22,000 a tenant-month.
- Range GET support is required for the packed layout; only R2 was confirmed from its compatibility page, the others are covered by the prototype probe (section 5).
- North American customers mostly: a bucket that is global or in a North American region is needed for NA clients. R2 and Tigris place data automatically; Hetzner Object Storage cannot (EU only). With presigned URLs the API node location matters little, because bytes bypass it.
- Proxy mode adds a hop and the API node's bandwidth cap: OVHcloud b3-8 offers 500 Mbit/s public, Hetzner and Vultr more. Presigned URLs remove that cap.

## 5. S3 compatibility, lock-in and portability

What the core needs from a store (the portable set): `PutObject` (optionally `If-None-Match: *`), `GetObject` with `Range`, `HeadObject`, `DeleteObject` and `DeleteObjects`, `ListObjectsV2`, multipart upload, presigned PUT and GET. Client-side integrity (AEAD tags and content hashes verified by the client) means checksum headers such as `x-amz-checksum-sha256` are a transport optimisation, never the trust anchor, so a backend without them is acceptable.

| Provider | Presigned URLs | Range GET | Multipart | `If-None-Match` on PUT | `x-amz-checksum-sha256` | Status |
| --- | --- | --- | --- | --- | --- | --- |
| R2 | yes (documented page) | yes | yes | **yes**, also `If-Match`, `If-Unmodified-Since` | SHA-256 listed as a composite checksum type; per-operation support on PutObject not shown, **test** | read from the compatibility page |
| Hetzner OS (Ceph RGW) | assumed (S3 standard) | assumed | yes, up to 10,000 parts | listed under restricted features for versioned buckets; **test** | **unverified** | limits read; operation list partial |
| B2, Wasabi, DO Spaces, Scaleway, OVHcloud, Vultr, Tigris | **unverified** for each | **unverified** | **unverified** | **unverified** | **unverified** | compatibility pages were blocked or not machine-readable; the prototype probe covers them |
| AWS S3 | yes | yes | yes | yes (since 2024) **(from memory)** | yes | de facto reference |
| GCS | via XML API with HMAC keys **(from memory)** | yes | via XML API | own generation preconditions **(from memory)** | not supported in the S3 interop layer **(from memory)** | needs a thin adapter for strict parity |
| Azure Blob | **no S3 API** | native | native (block upload) | native conditional headers | own MD5 and CRC64 | requires a provider adapter |

Portability rules for the design:

1. The core talks only to the portable set above, behind one `ObjectStore` trait with a local-disk implementation and an S3 implementation. Anything beyond it (Azure Blob, CDN signing, R2 jurisdictions, Workers) lives in a separate adapter crate.
2. At startup the S3 backend runs a conformance probe against the configured bucket (write-once on a second PUT with `If-None-Match: *`, ranged read, multipart, presigned round trip, batch delete) and refuses to start on a failed required check. An optional check (checksum headers) downgrades to a logged capability flag. Fail closed.
3. Where `If-None-Match` is absent, first-write-wins is arbitrated by the metadata database and objects get unique keys; the bucket then never needs conditional writes. This keeps Ceph RGW, MinIO, Garage and any older store usable.
4. Cloudflare-specific features (R2 jurisdictions and locations, Workers, Cache API) and B2's free egress through CDN partners are optional deployment topology, never core dependencies. Moving between R2, Hetzner Object Storage, B2 and Tigris is a bucket sync plus a config change.

## 6. Recommendation

The cost ranking is clear. Egress is 20 times the stored bytes, so providers charging 9 to 12 cents a GB (AWS, GCS, Azure) cost 9 to 12 dollars per tenant-month, against 0.07 to 1.2 for the rest. Once egress is free or near free, bucket cost is comparable to a node: at 500 tenants the best stack is about 99 a month, 0.20 per tenant. After egress, the deciding factors are North American performance, S3 conformance, request fees (which packing removes) and operational simplicity.

**Primary hosted setup**

- **Object storage: Cloudflare R2**, buckets per data region (North America default, EU jurisdiction for EU tenants), accessed through the S3 endpoint with presigned URLs.
- **Compute: Hetzner Cloud**, 2 API nodes behind a floating address or DNS failover at 100 tenants and up, in Ashburn or Hillsboro for North America and Falkenstein for EU tenants. The API is stateless Rust; bytes bypass it.
- **Metadata database: Postgres** on a dedicated Hetzner node with a volume and a replica or base backup to a bucket. SQLite on a volume is enough for the first tenants; Postgres is needed once two API nodes share state.
- **Object layout: packed** (5 to 20 MB packs, range reads). Unpacked on R2 costs 2.7 times as much in total at 100 tenants (107 against 40) and 10 times as much for the bucket alone (75 against 7), because of class A and B fees.
- **Mode: presigned URLs.** It removes API bandwidth as a scaling concern and costs nothing extra on R2.
- Expected cost: 11 (1 tenant), 12 (10), 40 (100), 99 (500) per month.

Why not the hyperscalers: egress. AWS at 500 tenants is 4,655 against 99, with no performance gain for this workload. Azure also lacks an S3 API. A CloudFront layer would cut the AWS egress rate for cached reads, but the data is client-encrypted and mostly unique per tenant, so cache hit rates are low; that path was not modelled.

**Runner-up: Hetzner Object Storage + Hetzner Cloud**

One vendor, one invoice, EU data residency, 97 to 132 a month at 500 tenants, and the lowest and most predictable egress after R2. It loses on North American latency (EU only), a request ceiling of 750 per second per bucket and per source IP, the 64 KB billing floor, an unverified conditional-write story and an unclear EUR 1 per TB egress line. It is the right fallback if R2 conformance or terms fail the prototype, and the natural home for EU tenants who need data residency inside the EU. B2 (cheapest per stored GB, 0.01 per GB egress beyond 3× stored, free through partner CDNs) and Tigris (zero egress, 0.02 per GB stored, global replication) are conformance-test candidates in the same prototype, not recommendations.

**Rejected as primary:** Wasabi (90-day minimum duration against churn, fair-use ceiling against a 20:1 egress ratio), DigitalOcean Spaces, Scaleway and Vultr (egress at 1 cent per GB beyond a small allowance; 95 to 118 at 100 tenants and 506 to 589 at 500 for the bucket alone, against 7 to 17 and 38 to 71 for the leaders), OVHcloud (the cheapest bucket at 39 for 500 tenants, but its instances cost 44 to 88 each; a candidate if it offers a North American region and passes S3 conformance), and any Cloudflare Worker or D1 core (a provider-specific runtime for the core is the lock-in the product rules out).

**Self-hosted default for a customer's own cluster**

- **Default: local disk.** One binary, a directory for blobs and a SQLite metadata file, no dependencies. Content-addressed files are written to a temporary name and renamed into place with exclusive-create semantics.
- **Cluster mode: any S3-compatible store**, the owner's Ceph RGW first, then MinIO or Garage. They must pass the startup conformance probe of section 5; where `If-None-Match` is missing, the metadata database arbitrates first-write-wins. Metadata moves from SQLite to Postgres once two API nodes run.
- No cloud SDK is linked into the default build.

## 7. Open items

Marked **(est.)** or **(unverified)** above; none of them changes the ranking by more than the stated margins.

1. Hetzner Cloud S and M prices and the VM overage rate. Entry plans start at EUR 5.99 and 6.49; the 2 vCPU class price is my estimate. Doubling the Hetzner compute estimate adds 61 at 500 tenants and does not change the ranking.
2. Hetzner Object Storage egress: the page says EUR 1.00 "per hour, per TB"; I read it as per TB. A tenfold higher rate would put the 500-tenant stack at about 600, which is dearer than R2 and B2-free-egress alternatives but still far below the hyperscalers.
3. Vultr Object Storage pricing (the page returned a bot challenge), Scaleway compute egress, GCP and DigitalOcean compute prices.
4. S3 conformance for every provider except R2 and (partly) Hetzner. The prototype probe closes it.
5. CloudFront request prices were not in the price list I queried; CloudFront was not priced.
6. Whether R2 requires a payment method on file for the free tier, and its checksum header behaviour on PutObject.
7. Wasabi's exact fair-use ratio.
8. All price pages move; re-read before committing to a contract.

## 8. Prototype plan (under 25 USD, a few weeks)

Goal: choose between R2 and Hetzner Object Storage with measurements, and confirm S3 conformance of the other candidates. Three weeks (21 days).

### Resources and expected bill

| Resource | Size | Basis | Expected |
| --- | --- | --- | --- |
| Cloudflare R2 | 2 buckets (North America default, EU jurisdiction); keep under 10 GB, 1M class A, 10M class B | free tier | 0 |
| Hetzner Cloud VM, Falkenstein | cheapest plan with at least 2 vCPU and 4 GB, hourly billing | EUR 6.49 per month × 21/30 | about EUR 4.5 |
| Hetzner Cloud VM, Ashburn or Hillsboro | same plan | same | about EUR 4.5 |
| Hetzner Object Storage | 1 bucket in Falkenstein | base EUR 0.0104 per hour × 504 hours, includes 1 TB stored and 1 TB egress | about EUR 5.3 |
| Backblaze B2 | 1 bucket, under 10 GB, egress under 30 GB | free tier and 3× stored egress | 0 |
| Tigris | 1 bucket, under 5 GB, under 10,000 class A and 100,000 class B | free tier | 0, overage cents |
| Total | | | about EUR 14.3, about 16 USD |
| VAT and slack | up to 19% VAT on EUR items, plus 3 USD for overage | | about 20 USD |

The unpacked test uploads 10,000 objects, which equals the Tigris class A free tier, so run it once there or accept a few cents. A 3 USD cushion covers it; the plan ceiling is 25 USD.

### Steps the owner must do by hand

- [ ] Cloudflare account; enable R2 (a payment method may be required); create buckets `cache-proto-na` and `cache-proto-eu` (EU jurisdiction); create an R2 API token with Object Read and Write scoped to these two buckets; note the account S3 endpoint; set a billing notification.
- [ ] Hetzner account and a Cloud project; add an SSH public key; create the two VMs (Falkenstein, Ashburn or Hillsboro) with the standard Linux image; create an Object Storage bucket in Falkenstein and a credential pair for it; set a budget reminder (Hetzner has no hard spend cap).
- [ ] Backblaze account; create one private bucket and an application key restricted to that bucket.
- [ ] Tigris account (through Fly.io); create one bucket and an access key.
- [ ] DNS (optional): one A record, for example `cache-proto.<your-domain>`, to the Ashburn or Hillsboro VM so the API can obtain a certificate by ACME; without it, test over the VM IP address.
- [ ] Put every key in a secret manager or the agent secret prompt. Never in the repository or in chat.
- [ ] At the end of week 3: delete both VMs, the Hetzner bucket, and the test buckets; confirm the next invoices are zero.

### Test script (run from both VMs)

1. **Conformance probe** with the AWS CLI against each endpoint: second `put-object --if-none-match "*"` must return 412; `put-object --checksum-algorithm SHA256` with a wrong digest must fail and with a right digest must succeed; `get-object --range bytes=100-199` returns 206 with the exact bytes; a three-part multipart upload (5 MiB parts) completes; a presigned PUT and GET work through `curl`, including with `If-None-Match` signed; `delete-objects` of 1,000 keys; `list-objects-v2` over 10,000 keys paginates.
2. **Latency and throughput.** Upload 10,000 × 90 KB objects, and 72 × 12.5 MB packs of the same bytes. From each VM: full cold fetch at concurrency 16 and 64 (unpacked GETs against packed range GETs), 750 incremental fetches of 200 artifacts, and record time to first byte (p50, p99), total time and effective Mbit/s per provider.
3. **Billing check.** Compare each dashboard's request and egress counters with the bytes and requests the script sent; confirm class A and B classification and the free-tier arithmetic.
4. **Churn.** Write 4 GB, delete it after one day, and verify the invoice line (matters for Wasabi-style minimum durations; here it confirms R2 and Hetzner bill nothing extra).

Output of the prototype: a table of conformance results and measured throughput per provider and per VM location, which replaces every **(unverified)** cell above and settles R2 against Hetzner Object Storage.

## Appendix: sources

All read 2026-11-02.

- Cloudflare R2: https://developers.cloudflare.com/r2/pricing/, https://developers.cloudflare.com/r2/platform/limits/, https://developers.cloudflare.com/r2/api/s3/api/, Workers: https://developers.cloudflare.com/workers/platform/pricing/
- AWS: price list files https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonS3/current/index.json, https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AWSDataTransfer/current/index.json, https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonEC2/current/us-east-1/index.json, https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonCloudFront/current/index.json; page https://aws.amazon.com/s3/pricing/
- Google Cloud Storage: https://cloud.google.com/storage/pricing
- Azure: retail prices API https://prices.azure.com/api/retail/prices (Storage, Bandwidth and Virtual Machines in eastus)
- Backblaze B2: https://www.backblaze.com/cloud-storage/pricing
- Wasabi: https://wasabi.com/pricing, https://docs.wasabi.com/docs/may-2026-wasabi-pricing-faqs, https://docs.wasabi.com/docs/how-do-wasabis-fair-use-policies-work
- Hetzner: https://www.hetzner.com/storage/object-storage/, https://docs.hetzner.com/storage/object-storage/overview/, https://docs.hetzner.com/storage/object-storage/supported-actions/, https://www.hetzner.com/cloud/, https://docs.hetzner.com/cloud/billing/faq/
- DigitalOcean: https://docs.digitalocean.com/products/spaces/details/pricing/, https://docs.digitalocean.com/products/spaces/details/limits/
- Scaleway: product catalog https://api.scaleway.com/product-catalog/v2alpha1/public-catalog/products (object_storage and instance)
- OVHcloud: https://www.ovhcloud.com/en/public-cloud/prices/
- Vultr: https://api.vultr.com/v2/plans?type=vc2 (compute); object storage page not readable
- Tigris: https://www.tigrisdata.com/pricing/; Fly.io: https://fly.io/docs/about/pricing/
