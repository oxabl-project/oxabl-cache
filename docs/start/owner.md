# Getting started: cache owner

The owner makes a cache and controls who belongs to it. This path ends with a cache that has a signed
roster, a `.oxabl/cache.toml` in your repository, and a second owner key kept offline.

You need:

- the `oxabl-cache` binary (operator.md, "Install and host the server");
- the server URL, the tenant id and a tenant administrator's token, from the
  [operator](operator.md). If someone else holds that token, they run step 2 for you.

## Roles and scopes

A cache has four member roles. A role says what a member may sign. A token scope says what the server
lets a member call. Both must allow the action, so each member needs a role and a matching token:

| Role | Signs | Token scopes | Mode |
| --- | --- | --- | --- |
| `owner` | rosters | `read`, `cache:admin` | `developer` |
| `ci` | trusted entries | `read`, `write:trusted` | `ci` |
| `developer` | their own personal entries | `read`, `write:personal` | `developer` |
| `reader` | nothing | `read` | `read-only` |

A `reader` cannot sign, so it cannot write. A token with `write:trusted` does not make a `reader` a
writer: the client stops with "the identity is not a ci member of the roster".

## 1. Make the owner identity and protect it

```sh
oxabl-cache identity new
New identity passphrase: 
New identity passphrase (again): 
identity file      /home/you/.config/oxabl-cache/identity
descriptor         oxabl-cache-member-1q…
signing key        ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ
signing key (hex)  255e544b772cf6a996ae7277b3c43f199f51aa15c8a2484890dbcb9d90254e74
```

The file holds the identity string, encrypted to your passphrase. `identity new` never overwrites an
existing file. Protect it:

- Choose a long passphrase and keep it in a password manager. Nobody can recover it.
- Back up the identity file next to the passphrase's record, not in the repository.
- Never put an owner identity in CI. An owner's key signs rosters. It is not a job secret.
- `oxabl-cache identity remember` keeps the passphrase in the operating system's keyring, so
  nothing asks for it on this computer. `identity forget` removes it.

Keep the signing key from the output. The server needs its hex form in step 2.

## 2. Create the cache

The server picks the cache id and checks every roster signature against the owner keys you register
now, so the owner key exists before the cache does. The tenant administrator calls the tenant API.
This call reads the token from a file (never on the command line):

```sh
( umask 077; printf 'Authorization: Bearer %s\n' "$(cat acme-admin.token)" > acme-admin.hdr )

curl -sS -H @acme-admin.hdr -H 'content-type: application/json' \
  https://cache.example.com/v1/tenant/caches \
  -d '{"display_name":"codebase","owner_keys":["255e544b772cf6a996ae7277b3c43f199f51aa15c8a2484890dbcb9d90254e74"]}'
{"id":"c_6v2k9q3m8x1r4t7w0y5z2b8n4d"}
```

`owner_keys` takes the 64-character hex key. The display name is yours to choose, and the operator sees
it. The same request again returns 200 with the same id.

Then make the owner's token. It holds `read` and `cache:admin` for this one cache. The cache is not
pinned in a repository yet, so `oxabl-cache token create` cannot run: use the call. The secret appears
once, so write it to a file:

```sh
( umask 077
  curl -sS -H @acme-admin.hdr -H 'content-type: application/json' \
    https://cache.example.com/v1/tenant/tokens \
    -d '{"name":"owner","scopes":["read","cache:admin"],"caches":["c_6v2k9q3m8x1r4t7w0y5z2b8n4d"],"expires_in_days":90}' \
    > owner.json
  jq -r .token owner.json > owner.token
  jq 'del(.token)' owner.json; rm owner.json )
{"expires_at":1802179860,"id":"1sssp8eqbr4s6thj","principal":"p_…"}
```

Always name the cache in `caches`. A token lives 90 days by default and one year at most. Mint a new
token before it expires (operations.md, "Renewing tokens"). Once `.oxabl/cache.toml` exists,
every later token comes from `oxabl-cache token create`.

## 3. Publish the first roster and pin the cache

```sh
oxabl-cache init --server https://cache.example.com \
  --tenant t_2h8k4m6p9q1r3s5t7v0w2x4y6z --cache c_6v2k9q3m8x1r4t7w0y5z2b8n4d \
  --label owner --token-file owner.token
# .oxabl/cache.toml
server = "https://cache.example.com"
tenant = "t_2h8k4m6p9q1r3s5t7v0w2x4y6z"
cache = "c_6v2k9q3m8x1r4t7w0y5z2b8n4d"
owner_keys = ["ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ"]
min_roster_version = 1

owner key  ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ
```

`init` asks for the identity passphrase, publishes roster version 1 at epoch 1 with you as the only
owner, and prints the block. It does not write the file. Put the block in
`.oxabl/cache.toml` at the repository root and commit it. The file holds no secret. Whoever can change
it can change what the build compiles, so it is the root of trust (security.md, "Actors and trust").

`init` refuses a cache that has a roster ("the cache already has a roster").

**Check.** From the repository, `status` shows a cache that is on, with you as the only member:

```sh
oxabl-cache --token-file owner.token status
cache               on
mode                developer
principal           owner (owner)
roster              v1, signed by ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ
epoch               1
min_roster_version  1
dictionary          v0
quota               0 live of 21474836480 hard, 0 pending, 0 stored (0 reclaimable)
members
  owner                    owner
```

## 4. Add a second owner key and keep it offline

One owner key is one point of failure. If you lose it, nobody can change the roster again
(operations.md, "Losing an owner key"). Add a second key now, and keep it where nobody uses it.

1. On a second computer, or from a removable drive, make the second identity. Use a different
   passphrase and store it separately:

   ```sh
   oxabl-cache identity new --file /media/offline/identity
   ```

2. Run `owner add` on your own computer, with the second identity file attached. The new owner's key
   co-signs the roster, so the command needs that file once. Detach it afterwards:

   ```sh
   oxabl-cache --token-file owner.token owner add <descriptor> --label offline \
     --cosign-identity-file /media/offline/identity
   published roster v2 at epoch 1
   set in .oxabl/cache.toml:
   owner_keys = ["ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ", "ed25519:rM09A6LI2gBy8TBi+6s8EnXIISlRCXUXa0Bjpic0fuU"]
   min_roster_version = 2
   and register the keys on the server, as the tenant administrator:
   PUT /v1/tenant/caches/c_6v2k9q3m8x1r4t7w0y5z2b8n4d/owner-keys {"owner_keys":["255e54…","accd3d…"]}
   ```

   Copy the whole descriptor. A cut-off descriptor fails with "not a member descriptor": the real one
   is about 130 characters.

3. Register both keys on the server, with the `PUT` the command printed (the tenant administrator
   runs it, with the hex keys it printed):

   ```sh
   curl -sS -X PUT -H @acme-admin.hdr -H 'content-type: application/json' \
     https://cache.example.com/v1/tenant/caches/c_6v2k9q3m8x1r4t7w0y5z2b8n4d/owner-keys \
     -d '{"owner_keys":["255e544b…","accd3d03…"]}'
   ```

   The server answers 204.

4. Replace the two lines in `.oxabl/cache.toml` with the ones the command printed, and commit.

5. Put the offline identity file and its passphrase in two different safe places.

**Check.** The roster has two owners and two signatures. The offline identity opens the cache by
itself, which proves the key works:

```sh
oxabl-cache --token-file owner.token --identity-file /media/offline/identity status
cache               on
mode                developer
principal           offline (owner)
roster              v2, signed by ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ, ed25519:rM09A6LI2gBy8TBi+6s8EnXIISlRCXUXa0Bjpic0fuU
```

Run this check on a computer without the daily identity if you can. Do it again after every change
of owners.

## What the owner key controls

The owner key signs every roster. A roster says who is a member, which role each has, and which
epoch is current. A client trusts a roster only if a key pinned in `.oxabl/cache.toml` signed it. So
whoever holds an owner key decides who can read and who can write the trusted namespace. The server,
the operator and the storage provider cannot. data-handling.md lists what each party can see.

## Next

- Add [CI](ci.md) with one command (`oxabl-cache onboard ci`), and [developers](developer.md) with
  `member add` plus a token.
- Rotate and remove members: operations.md, "Rotating a member".
- Train a compression dictionary after the first full build: `oxabl-cache dict train`
  (client.md, "Verification and dictionaries").
