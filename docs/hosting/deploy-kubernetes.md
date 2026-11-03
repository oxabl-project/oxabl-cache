# Deploy on Kubernetes

The Helm chart in `deploy/helm/oxabl-cache` runs the server in two modes (operations.md, "Deployment shapes").
The chart renders no Secret: it names the Secrets you create, and mounts their files read-only.

| Mode | Replicas | Metadata | Bytes | Workload |
| --- | --- | --- | --- | --- |
| `cluster` (default) | 2 or more, HPA on CPU | Postgres | S3-compatible bucket | Deployment, rolling update, `PodDisruptionBudget` `minAvailable: 1` |
| `single-node` | 1 | SQLite on a PVC | local directory on the same PVC | Deployment with `Recreate`, PVC kept on uninstall |

Cluster mode needs the Postgres and S3 support described in operations.md. Single-node mode works with the
server as built today. There are no CronJobs: garbage collection and compaction run in the pods under a
Postgres advisory-lock leader, and migrations run at startup under the same lock.

## Prerequisites

- Kubernetes 1.27 or later, Helm 3 or 4, a way to reach the public listener (an Ingress controller or a
  `LoadBalancer` Service) with TLS.
- The image `ghcr.io/oxabl-project/oxabl-cache-server` (`image.tag`, or `image.digest` to pin). The chart's
  default tag is its `appVersion`; set `image.tag` explicitly until releases are cut.
- Cluster mode: a Postgres database and user, and a bucket with its credentials.

## Secrets you create

The chart only references them. Keys are 64 lowercase hex characters:

```sh
kubectl create namespace oxabl-cache

kubectl -n oxabl-cache create secret generic oxabl-cache-keys \
  --from-literal=url-key="$(openssl rand -hex 32)" \
  --from-literal=claim-key="$(openssl rand -hex 32)"
```

Cluster mode also needs one Secret with the database password and one with the S3 credentials, named in
`metadata.postgres.passwordSecret` and `store.s3.credentialsSecret` (key names are values too):

```sh
kubectl -n oxabl-cache create secret generic oxabl-cache-s3 \
  --from-file=access-key-id=<(printf '%s' "$ACCESS_KEY_ID") \
  --from-file=secret-access-key=<(printf '%s' "$SECRET_ACCESS_KEY")
```

`tls.mode: secret` takes a `kubernetes.io/tls` Secret (`tls.crt`, `tls.key`). With the default `tls.mode:
upstream` the Ingress terminates TLS and speaks plain HTTP to the pod, and the server runs with
`tls.terminated_upstream = true`; keep the namespace's pods and the `NetworkPolicy` tight in that case, or use
`tls.mode: secret` with the Ingress talking HTTPS to the backend.

Every secret reaches the pod as a file under `/run/secrets/oxabl-cache` through one projected volume. The server
rejects a secret written inline in its config, and the chart has no value that would put one there.

## Install

Single node, SQLite and local objects on a 20 Gi PVC:

```sh
helm install oxabl-cache deploy/helm/oxabl-cache -n oxabl-cache \
  -f deploy/helm/oxabl-cache/examples/single-node.yaml --set image.tag=<tag>
```

Cluster mode, on an in-cluster S3 store (Ceph RGW, Garage) and a CloudNativePG cluster:

```sh
helm install oxabl-cache deploy/helm/oxabl-cache -n oxabl-cache \
  -f deploy/helm/oxabl-cache/examples/cluster-ceph-cnpg.yaml --set image.tag=<tag>
```

`examples/hosted-r2.yaml` is the same chart pointed at Cloudflare R2 (a bucket's S3 endpoint is
`https://<account id>.r2.cloudflarestorage.com`, region `auto`). Copy an example, replace the host names, Secret
names and endpoints, and keep it in your own repository.

Presigned object URLs name the S3 endpoint, so in S3 mode `store.s3.endpoint` must be reachable from CI and
developer machines, not only from the pods. Expose RGW or Garage through its own Ingress and use that URL.

### The first operator token

`init` runs in a pod; it needs no shell:

```sh
kubectl -n oxabl-cache exec deploy/oxabl-cache -- /usr/local/bin/oxabl-cache-server init
```

It prints the token once on stdout and refuses if one exists. Store it in a secret manager. The operator then
reaches the admin listener through the ClusterIP Service, never through the Ingress:

```sh
kubectl -n oxabl-cache port-forward svc/oxabl-cache-admin 9091:9091
oxabl-cache-admin --token-file token tenant create acme --region na --hard-quota 20GiB
```

## What the chart creates

| Object | Notes |
| --- | --- |
| `Deployment` | `securityContext`: non-root (65532), read-only root filesystem, all capabilities dropped, no privilege escalation, seccomp `RuntimeDefault`, no service account token. The only writable path is the PVC in single-node mode. Pods spread over nodes (`ScheduleAnyway`). The pod restarts when the config changes. |
| `ConfigMap` | The server config rendered from values (`server.toml`). |
| `Service` | Public listener (`service.port`, 443 to the pod's 8443). |
| `Service` `-admin` | ClusterIP only, port 9091. |
| `Service` `-metrics` | ClusterIP, port 9090. |
| `Ingress` | Optional (`ingress.enabled`), public listener only. |
| `HorizontalPodAutoscaler` | Cluster mode: CPU 70%, 2 to 6 replicas. Rate limits divide by the live node count, so scaling keeps a tenant's limit stable. |
| `PodDisruptionBudget` | Cluster mode: `minAvailable: 1`. |
| `ServiceMonitor` | Optional (`monitoring.serviceMonitor.enabled`); needs the Prometheus Operator CRD. |
| `NetworkPolicy` | Optional (`networkPolicy.enabled`). The public port accepts `publicFrom` (any source when empty); the admin and metrics ports accept only the sources you list. |
| `PersistentVolumeClaim` | Single-node only; annotated `helm.sh/resource-policy: keep`, so `helm uninstall` leaves the data. |

Probes use the metrics listener (`GET /metrics` on port 9090). The server binds the listeners before it has
reached its database, and starts answering only afterwards, so the probe means "serving", not "listening".
Probes do not use the public listener (it has no unauthenticated route) or the admin health route (it needs the
operator token).

## Operating it

- **Upgrade:** `helm upgrade` with a new `image.tag`. Cluster mode rolls with `maxUnavailable: 0`; migrations
  are additive, so the old and new pods run side by side during the roll-out.
- **Logs:** JSON lines on stderr (`kubectl logs`). Tokens, query strings, ids and claims are never logged.
  A line `readable by other users; chmod 600 it` about the URL key is expected on Kubernetes: a Secret volume with
  `fsGroup` is group-readable (mode 0440) for the pod's own group, which is the only way a non-root container can
  read it.
- **Metrics and alerts:** scrape `-metrics:9090`; the series and suggested alerts are in operations.md,
  "Observability".
- **Backups:** Postgres base backups and WAL archiving (CloudNativePG does this to a bucket), as in operations.md.
  In single-node mode, snapshot the PVC.
- **Uninstall:** `helm uninstall` keeps the PVC. Delete it yourself when the data is not needed.

## Checks the repository runs

`deploy/helm/check.sh` lints and renders the chart for every example, validates the manifests against the
Kubernetes 1.31 schemas with `kubeconform -strict` (the `ServiceMonitor` CRD is skipped), and fails if the chart
renders any `Secret` or a pod that is not hardened. `deploy/helm/kind-smoke.sh` installs the single-node chart
into a throwaway `kind` cluster, runs `init` in the pod and reads the admin health route.
