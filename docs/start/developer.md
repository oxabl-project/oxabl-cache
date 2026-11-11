# Getting started: developer

A developer reads what CI wrote and keeps their own compiled output in a personal namespace that
nobody else reads. This path ends with a computer that is a member of the cache and fetches from it.

You need: a clone of the repository (it holds `.oxabl/cache.toml`) and a cache owner who adds you.
You can join in the desktop app or on the command line. Both make the same member.

## Join in the desktop app

The desktop app joins a cache in three steps (desktop/README.md, "Joining"):

1. The owner runs `oxabl-cache invite --name <codebase>` and sends you the one-line invite. It starts
   with `oxc-invite:` and holds no secret.
2. You paste the invite and sign in. When the invite names no sign-in, paste an access token from your
   administrator instead. The app keeps the token in the operating system's keyring.
3. The app makes this computer's identity and shows a request. Copy it and send it to the owner, who
   runs it once. The app notices and says "You're in".

Copy the identity passphrase that the app offers into your password manager.

## Join on the command line

1. **Make your identity.** The command asks for a passphrase twice. The file holds the identity
   encrypted to it:

   ```sh
   oxabl-cache identity new
   New identity passphrase: 
   New identity passphrase (again): 
   identity file      /home/you/.config/oxabl-cache/identity
   descriptor         oxabl-cache-member-1q…
   signing key        ed25519:nmBebqk1SQiRYngh/vwTUSTYKlzigrlyGRy4jH0uJKE
   signing key (hex)  9e605e6ea935490891627821fefc135124d82a5ce282b972191cb88c7d2e24a1
   ```

   An identity belongs to one computer. A second computer gets its own. To stop the prompts, run
   `oxabl-cache identity remember`: it stores the passphrase in the operating system's keyring.

2. **Send the descriptor to an owner.** Copy all of it, about 130 characters. The owner runs:

   ```sh
   oxabl-cache member add <descriptor> --label alice --role developer
   published roster v5 at epoch 1
   ```

   The label is the name the team sees. It need not be your real name.

3. **Get a credential.** Use one of two ways:

   - **An access token.** The tenant administrator makes a token with the scopes `read` and
     `write:personal` for the cache (owner.md, "Roles and scopes"; ci.md, step 3 shows the call) and
     sends it to you. Save it in a file of mode 0600. A token lives 90 days by default.
   - **A sign-in.** If your administrator registered your identity provider (below), run `login`:

     ```sh
     oxabl-cache login --issuer https://idp.example --client-id <client id>
     ```

     It prints a verification page and a code. Open the page, enter the code and approve. The command
     stores a session that lasts 12 hours. `oxabl-cache logout` revokes it and deletes the file.

   A person who holds a token but is not in the roster sees "the identity is not in the roster". The
   owner has not run step 2 yet.

## First fetch

From the clone, check that the cache is on and that you are a developer. With a token file, add
`--token-file`; after `login`, add nothing:

```sh
oxabl-cache --token-file alice.token status
cache               on
mode                developer
principal           alice (developer)
roster              v5, signed by ed25519:JV5US3cs9qmWrnJ3s8Q/GZ9RqhXIokhIkNvLnZAlTnQ
epoch               1
min_roster_version  2
dictionary          v0
quota               0 live of 21474836480 hard, 0 pending, 0 stored (0 reclaimable)
members
  owner                    owner
  ci-main                  ci
  alice                    developer
```

Then fetch a sample of what CI stored. `verify` downloads entries into a scratch directory, checks
every signature, and exits 1 unless every sampled entry verifies:

```sh
oxabl-cache --token-file alice.token verify --sample 20
sampled      20
verified     20
not trusted  0
```

`sampled 0` means CI has not filled the cache yet. That is a failed check, not a pass. Ask the CI
maintainer to run the fill job (ci.md).

Your build then fetches by itself. In `developer` mode it reads the trusted entries that CI wrote,
then your own personal entries, and writes only personal entries. To fetch one key by hand:

```sh
oxabl-cache --token-file alice.token fetch --key <64 hex digits> --stage stage
```

Your build reads the token from `OXABL_CACHE_TOKEN`. Export it from a secret manager, not from a
shell profile. Never paste a token into a command line: no flag accepts it.

## When you leave

The owner removes you with `member remove`. That starts a new epoch, and the administrator revokes
your token. You lose access at once, and the cache turns cold once for everyone
(operations.md, "Rotating a member").

## For the administrator: sign-in setup

Sign-in needs three calls, once per tenant and once per person, by a tenant administrator
(protocol.md, "Tenant administration"). They use the header file from ci.md, step 3:

```sh
# once: the identity provider
curl -sS -H @acme-admin.hdr -H 'content-type: application/json' \
  https://cache.example.com/v1/tenant/oidc/issuers \
  -d '{"issuer":"https://idp.example","shape":"custom","id_claims":["sub"]}'

# per person: a user, keyed by the issuer and the subject claim
curl -sS -H @acme-admin.hdr -H 'content-type: application/json' \
  https://cache.example.com/v1/tenant/users \
  -d '{"issuer":"https://idp.example","subject":"<sub>","label":"alice"}'
{"id":"p_34g4zt7sxx6e2mvss9wnaftwe0"}

# per person: the scopes on the cache, for the id the last call returned
curl -sS -X PUT -H @acme-admin.hdr -H 'content-type: application/json' \
  https://cache.example.com/v1/tenant/grants \
  -d '{"principal":"p_34g4zt7sxx6e2mvss9wnaftwe0","cache":"c_6v2k9q3m8x1r4t7w0y5z2b8n4d","scopes":["read","write:personal"]}'
```

The identity provider must put exactly `oxabl-cache:<tenant id>` in the `aud` claim of the ID token,
and it needs a public client for the device grant. An owner can then put the issuer and the client id
in the invite: `oxabl-cache invite --name <codebase> --issuer <url> --client-id <id>`.
