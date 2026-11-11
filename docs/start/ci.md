# Getting started: CI maintainer

A CI system uses the cache in two ways. Jobs on the protected branch compile and write the trusted
namespace. Pull request jobs only read. This path ends with both: a CI system whose branch jobs fill
the cache and whose pull request jobs fetch from it, and a check that proves each.

You need: the cache's `.oxabl/cache.toml` in the repository (owner.md), the `oxabl-cache` binary on
the computer where you set up, and two helpers. The cache owner runs `member add` (step 2). The
tenant administrator makes the tokens (step 3). If you hold both roles, you run every step.

> **Planned:** one command will replace steps 1 to 4 below (identity, roster entry, token and secrets
> in one run). Until it ships, follow the steps.
<!-- onboard-ci: the single command replaces steps 1 to 4 -->

## What a CI system gets

Each CI system gets two members, because the two jobs need different rights (owner.md, "Roles and
scopes"):

| Member | Role | Token scopes | Mode | Secrets go to |
| --- | --- | --- | --- | --- |
| `ci-main` | `ci` | `read`, `write:trusted` | `ci` | protected-branch jobs only |
| `ci-pr` | `reader` | `read` | `read-only` | pull request jobs |

Each member has two secrets: the identity string (`OXABL_CACHE_IDENTITY`) and the token
(`OXABL_CACHE_TOKEN`). The identity decrypts and signs. The token opens the server's door. Neither
works without the other.

A pull request that comes from a fork gets no secrets on the supported CI systems. It builds without
the cache. That is the intended result.

## 1. Make the identities

Run this once for each member. `--secret-out` writes the identity string to a file of mode 0600, for a
CI secret store. It makes no passphrase file, and it never prints the string:

```sh
oxabl-cache identity new --secret-out ci-main.identity
secret file        ci-main.identity
descriptor         oxabl-cache-member-1q…
signing key        ed25519:DYVY+uZe/7PsdF6AZHN7oIFq4B83UCwGadaC8QQO/rk
signing key (hex)  0d8558fae65effb3ec745e8064737ba0816ae01f37502c0669d682f1040efeb9

oxabl-cache identity new --secret-out ci-pr.identity
```

The descriptor is public. Copy all of it, about 130 characters, and send it to the owner. A descriptor
cut off in the middle fails with "not a member descriptor".

## 2. The owner adds the members

The owner runs, with the descriptors from step 1:

```sh
oxabl-cache --token-file owner.token member add <ci-main descriptor> --label ci-main --role ci
published roster v3 at epoch 1
oxabl-cache --token-file owner.token member add <ci-pr descriptor> --label ci-pr --role reader
published roster v4 at epoch 1
```

The owner's identity file has its own passphrase. If the command says "cannot unlock the identity
file", you used the passphrase of another identity file.

A new member joins the current epoch. The cache stays warm.

## 3. The tenant administrator makes the tokens

Tokens with `read`, `write:trusted` or `write:personal` have no command today. The tenant
administrator calls `POST /v1/tenant/tokens` (protocol.md, "Tenant administration"). The call reads
the administrator's token from a file. The secret of the new token comes back once, in the field
`token`, so write it to a file:

```sh
( umask 077; printf 'Authorization: Bearer %s\n' "$(cat acme-admin.token)" > acme-admin.hdr )

( umask 077
  curl -sS -H @acme-admin.hdr -H 'content-type: application/json' \
    https://cache.example.com/v1/tenant/tokens \
    -d '{"name":"ci-main","scopes":["read","write:trusted"],"caches":["c_6v2k9q3m8x1r4t7w0y5z2b8n4d"],"expires_in_days":90}' \
    > ci-main.json
  jq -r .token ci-main.json > ci-main.token
  jq 'del(.token)' ci-main.json; rm ci-main.json )
{"expires_at":1802179891,"id":"9edjcs70bby4q8p8","principal":"p_…"}
```

Make the second token the same way, with `"name":"ci-pr"` and `"scopes":["read"]`.

The role and the scopes must agree. A `reader` cannot sign. A pull request job therefore uses the read
token and the read-only mode. Give the `ci` member's token `write:trusted` and no other write scope.

A token lives 90 days by default and one year at most. Put the renewal date in your calendar
(operations.md, "Renewing tokens").

## 4. Put the secrets in the CI system

Create four secrets in the CI system's secret store. Paste the contents of each file:

| Secret | File | Visible to |
| --- | --- | --- |
| `OXABL_CACHE_IDENTITY` | `ci-main.identity` | protected-branch jobs only |
| `OXABL_CACHE_TOKEN` | `ci-main.token` | protected-branch jobs only |
| `OXABL_CACHE_IDENTITY` | `ci-pr.identity` | pull request jobs |
| `OXABL_CACHE_TOKEN` | `ci-pr.token` | pull request jobs |

Use the CI system's environment or protected-variable scoping, so a pull request job never sees the
`ci-main` pair. Both pairs use the same variable names. The scoping picks the pair. Delete the local
files when you finish.

A `ci` identity in a pull request job lets that job write the trusted namespace. Anyone who can open a
pull request could then plant artifacts. That is why the scoping matters.

## 5. Set the mode in each job

The client reads the mode from `OXABL_CACHE_MODE`. Without it, a job with the `CI` variable set runs
read-only, and any other job runs as a developer. Writing to the trusted namespace always needs an
explicit `ci`:

| Job | Variables |
| --- | --- |
| Protected branch (fills the cache) | `OXABL_CACHE_MODE=ci`, plus the `ci-main` pair |
| Pull request | `OXABL_CACHE_MODE=read-only`, plus the `ci-pr` pair |

CI systems that do not set `CI` (Jenkins, TeamCity, Bamboo agents, for example) need the mode in every
job. The job also needs the repository checked out, because the client reads `.oxabl/cache.toml` from
it.

Run a fill job on every push to the main branch. A change of the cache schema, of the toolchain or of
the compile key makes every key miss until a job refills the cache. A fill job on every push to the
main branch is the pattern that keeps the cache warm.

GitHub Actions and GitLab CI can also sign in with their own job tokens, without `OXABL_CACHE_TOKEN`.
The tenant then registers the issuer and a trust policy (security.md, "Transport and authentication").
The identity string stays a secret in both cases.

## 6. Check it works

In a protected-branch job (or on a computer with the same variables), check the member first:

```sh
OXABL_CACHE_MODE=ci oxabl-cache status
cache               on
mode                ci
principal           ci-main (ci)
roster              v4, signed by ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ
epoch               1
min_roster_version  2
dictionary          v0
quota               0 live of 21474836480 hard, 0 pending, 0 stored (0 reclaimable)
members
  owner                    owner
  offline                  owner
  ci-main                  ci
  ci-pr                    reader
```

`mode ci` and a principal with the role `ci` mean the member can write. If the cache is `off`, the
`reason` line names the cause: operations.md, "When something fails".

Then run the build that fills the cache. The first run stores every unit. To test the member without
a build, store one file by hand. The key is 64 hex digits:

```sh
OXABL_CACHE_MODE=ci oxabl-cache put --key aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --file r/hello.r=out/r/hello.r
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  committed  (248 bytes uploaded)
```

In a pull request job, the read-only member fetches the same key. `fetch` installs the files and
prints the signer:

```sh
OXABL_CACHE_MODE=read-only oxabl-cache fetch --key aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --stage stage
key     aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
tier    trusted
signer  0d8558fae65effb3ec745e8064737ba0816ae01f37502c0669d682f1040efeb9
epoch   1
  r/hello.r (16 bytes)
```

`tier trusted` and the `ci-main` signing key show that the pull request job read what the branch job
wrote, and verified it. For a larger proof, fetch a random sample of the real entries. It exits 1 when
nothing verified, and an empty cache verifies nothing:

```sh
OXABL_CACHE_MODE=read-only oxabl-cache verify --sample 20
sampled      20
verified     20
not trusted  0
```

When your build links the client, it fetches the same way: the pull request job compiles only what the
cache lacks.

## Common mistakes

| You see | Cause | Fix |
| --- | --- | --- |
| `not a member descriptor` | The descriptor was copied cut off. | Copy all of it (about 130 characters). |
| `the identity is not a ci member of the roster` | A job in `ci` mode holds a `reader` identity. | Use the `ci-main` identity, or set the mode to `read-only`. |
| `not stored: failed: rejected` | The token lacks `write:trusted`. | Make a token with `read` and `write:trusted`. |
| `cannot unlock the identity file` | The passphrase belongs to another identity file. | Use the passphrase of the file you pass. |
| `the cache server refused the credentials` | The token expired, was revoked, or is wrong. | Make a new token (operations.md, "Renewing tokens"). |
| Every fetch misses after a change | A schema, toolchain or key change, or a rotation. | Run the fill job. |

More: operations.md, "When something fails".
