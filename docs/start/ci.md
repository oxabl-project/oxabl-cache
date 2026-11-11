# Getting started: CI maintainer

A CI system uses the cache in two ways. Jobs on the protected branch compile and write the trusted
namespace. Pull request jobs only read. This path ends with both: a CI system whose branch jobs fill
the cache and whose pull request jobs fetch from it, and a check that proves each.

You need: the cache's `.oxabl/cache.toml` in the repository (owner.md), the `oxabl-cache` binary, and
a checkout of the repository to run the commands in.

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

## 1. Make the members with one command

`onboard ci` is for a person who is both the cache owner and the tenant administrator. You need:

- the owner identity, and its passphrase, on this computer (owner.md, step 1);
- the owner's token, which holds `cache:admin` on the cache (owner.md, step 2), passed with
  `--token-file`;
- a `tenant:admin` token of the tenant, passed with `--admin-token-file` (operator.md, step 3).

Run it once for each member, in the repository. First the member that writes:

```sh
oxabl-cache --token-file owner.token onboard ci --name ci-main \
  --admin-token-file acme-admin.token --out ci-main
```

The command asks for the owner's passphrase. It then:

1. checks every input and changes nothing: the passphrase, the owner role, `cache:admin` on the
   owner's token, `tenant:admin` on the administrator token, a new label, and that the two output
   files do not exist;
2. makes a token for this cache only, with the scopes of the role (`read` and `write:trusted` for
   `ci`, `read` for `reader`), and asks the server to confirm them;
3. writes two files of mode 0600 in `ci-main/`, named after the variables: `OXABL_CACHE_TOKEN` and
   `OXABL_CACHE_IDENTITY`. It makes the directory with mode 0700 when it is missing;
4. adds the new identity to the roster with the role, signed by the owner. The roster changes last.

It prints one check line and never prints a secret:

```
member      ci-main (ci), roster v3, epoch 1
token       9edjcs70bby4q8p8, read, write:trusted
expires at  unix time 1802179891

Store these files as secrets of the CI system, under these names, then delete them:
  OXABL_CACHE_IDENTITY   ci-main/OXABL_CACHE_IDENTITY
  OXABL_CACHE_TOKEN      ci-main/OXABL_CACHE_TOKEN

Check, in a checkout of the repository with both secrets in the environment:
  oxabl-cache --mode ci status
```

Then the member for pull requests, with `--role reader` and its own directory:

```sh
oxabl-cache --token-file owner.token onboard ci --name ci-pr --role reader \
  --admin-token-file acme-admin.token --out ci-pr
```

Its check line is `oxabl-cache --mode read-only status`. The token lives 90 days by default, and one
year at most. Set another life with `--expires-in-days`, and put the renewal date in your calendar
(operations.md, "Renewing tokens").

If a step before the roster change fails, the command revokes the new token, removes the files and
says so. The roster stays as it was, and no new epoch is needed. If the command cannot tell whether
the roster changed, it says what to check (`oxabl-cache status`) and how to undo it
(`oxabl-cache token revoke <id>`, and delete the files). If the process stops between steps 2 and 4,
a token named like the member stays in `token list`: revoke it.

## 2. Put the secrets in the CI system

Create four secrets in the CI system's secret store. Paste the contents of each file, then delete the
files:

| Secret | File | Visible to |
| --- | --- | --- |
| `OXABL_CACHE_IDENTITY` | `ci-main/OXABL_CACHE_IDENTITY` | protected-branch jobs only |
| `OXABL_CACHE_TOKEN` | `ci-main/OXABL_CACHE_TOKEN` | protected-branch jobs only |
| `OXABL_CACHE_IDENTITY` | `ci-pr/OXABL_CACHE_IDENTITY` | pull request jobs |
| `OXABL_CACHE_TOKEN` | `ci-pr/OXABL_CACHE_TOKEN` | pull request jobs |

Use the CI system's environment or protected-variable scoping, so a pull request job never sees the
`ci-main` pair. Both pairs use the same variable names. The scoping picks the pair.

A `ci` identity in a pull request job lets that job write the trusted namespace. Anyone who can open a
pull request could then plant artifacts. That is why the scoping matters.

## 3. Set the mode in each job

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

## 4. Check it works

In a protected-branch job (or in a checkout on a computer with the same variables), run the check line
the command printed:

```sh
oxabl-cache --mode ci status
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
oxabl-cache --mode ci put --key aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --file r/hello.r=out/r/hello.r
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  committed  (248 bytes uploaded)
```

In a pull request job, the read-only member fetches the same key. `fetch` installs the files and
prints the signer:

```sh
oxabl-cache --mode read-only fetch --key aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --stage stage
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
oxabl-cache --mode read-only verify --sample 20
sampled      20
verified     20
not trusted  0
```

When your build links the client, it fetches the same way: the pull request job compiles only what the
cache lacks.

## When the owner and the tenant administrator are different people

`onboard ci` needs both tokens in one place. When two people hold them, split the work. The
descriptor and the tokens travel between them. The identity string never does.

1. **The CI maintainer makes the identities.** Run it once for each member. `--secret-out` writes the
   identity string to a file of mode 0600, for a CI secret store. It makes no passphrase file, and it
   never prints the string:

   ```sh
   oxabl-cache identity new --secret-out ci-main.identity
   secret file        ci-main.identity
   descriptor         oxabl-cache-member-1q…
   signing key        ed25519:DYVY+uZe/7PsdF6AZHN7oIFq4B83UCwGadaC8QQO/rk
   signing key (hex)  0d8558fae65effb3ec745e8064737ba0816ae01f37502c0669d682f1040efeb9

   oxabl-cache identity new --secret-out ci-pr.identity
   ```

   Send each descriptor to the owner. Copy all of it, about 130 characters. A descriptor cut off in
   the middle fails with "not a member descriptor".

2. **The owner adds the members**, with the descriptors from step 1:

   ```sh
   oxabl-cache --token-file owner.token member add <ci-main descriptor> --label ci-main --role ci
   published roster v3 at epoch 1
   oxabl-cache --token-file owner.token member add <ci-pr descriptor> --label ci-pr --role reader
   published roster v4 at epoch 1
   ```

   The owner's identity file has its own passphrase. If the command says "cannot unlock the identity
   file", you used the passphrase of another identity file. A new member joins the current epoch, so
   the cache stays warm.

3. **The tenant administrator makes the tokens**, in a checkout of the repository. The role and the
   scopes must agree: a `reader` cannot sign, so the pull request member gets only `read`. The secret
   of each token goes to a new file of mode 0600 and is never printed:

   ```sh
   oxabl-cache token create --admin-token-file acme-admin.token \
     --scopes read,write:trusted --name ci-main --out ci-main.token
   id          9edjcs70bby4q8p8
   name        ci-main
   scopes      read, write:trusted
   expires at  unix time 1802179891
   token file  ci-main.token

   oxabl-cache token create --admin-token-file acme-admin.token \
     --scopes read --name ci-pr --out ci-pr.token
   ```

   A token lives 90 days by default and one year at most. `--expires-in-days` sets another life.

4. **The CI maintainer stores the secrets.** Put each file in the secret store as in step 2 above:
   `ci-main.identity` and `ci-main.token` become `OXABL_CACHE_IDENTITY` and `OXABL_CACHE_TOKEN` for
   protected-branch jobs, and the `ci-pr` files become the same two names for pull request jobs.
   Then continue with step 3 above.

## Common mistakes

| You see | Cause | Fix |
| --- | --- | --- |
| `not a member descriptor` | The descriptor was copied cut off. | Copy all of it (about 130 characters). |
| `the identity is not a ci member of the roster` | A job in `ci` mode holds a `reader` identity. | Use the `ci-main` identity, or set the mode to `read-only`. |
| `not stored: failed: rejected` | The token lacks `write:trusted`. | Make a token with `read` and `write:trusted`. |
| `cannot unlock the identity file` | The passphrase belongs to another identity file. | Use the passphrase of the file you pass. |
| `the passphrase does not unlock the owner identity` (from `onboard ci`) | The same, for the owner identity. | Use the owner's passphrase. |
| `the owner's token has no cache:admin on <cache>` | `--token-file` names a token without `cache:admin`. | Pass the owner's token (owner.md, step 2). |
| `admin token <file>: the token does not carry tenant:admin` | `--admin-token-file` names another token. | Pass the tenant administrator's token. |
| `<file> exists; a secret file is never overwritten` | The output directory already holds the files of an earlier run. | Choose a new `--out`. Delete the old files only when no CI system uses them. |
| `the cache server refused the credentials` | The token expired, was revoked, or is wrong. | Make a new token (operations.md, "Renewing tokens"). |
| Every fetch misses after a change | A schema, toolchain or key change, or a rotation. | Run the fill job. |

More: operations.md, "When something fails".
