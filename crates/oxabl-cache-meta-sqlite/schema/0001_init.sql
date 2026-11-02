CREATE TABLE tenants (
    tenant_id   BLOB PRIMARY KEY,
    name        TEXT NOT NULL,
    status      INTEGER NOT NULL CHECK (status IN (0, 1)),
    hard_bytes  INTEGER CHECK (hard_bytes IS NULL OR hard_bytes >= 0),
    egress_bytes INTEGER CHECK (egress_bytes IS NULL OR egress_bytes >= 0),
    rate        INTEGER NOT NULL,
    burst       INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE caches (
    cache_id       BLOB PRIMARY KEY,
    tenant_id      BLOB NOT NULL REFERENCES tenants (tenant_id),
    display_name   TEXT NOT NULL,
    retention_days INTEGER NOT NULL CHECK (retention_days >= 0),
    hard_bytes     INTEGER NOT NULL CHECK (hard_bytes >= 0),
    epoch          INTEGER NOT NULL CHECK (epoch >= 0),
    roster_version INTEGER NOT NULL CHECK (roster_version >= 0),
    live_bytes     INTEGER NOT NULL CHECK (live_bytes >= 0),
    pending_bytes  INTEGER NOT NULL CHECK (pending_bytes >= 0),
    created_at     INTEGER NOT NULL
);
CREATE INDEX caches_tenant ON caches (tenant_id);

CREATE TABLE cache_owner_keys (
    cache_id BLOB NOT NULL REFERENCES caches (cache_id),
    sign_key BLOB NOT NULL,
    PRIMARY KEY (cache_id, sign_key)
);

CREATE TABLE users (
    principal_id BLOB PRIMARY KEY,
    tenant_id    BLOB NOT NULL REFERENCES tenants (tenant_id),
    issuer       TEXT NOT NULL,
    subject      TEXT NOT NULL,
    label        TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    disabled_at  INTEGER,
    UNIQUE (tenant_id, issuer, subject)
);

CREATE TABLE grants (
    principal_id BLOB NOT NULL,
    tenant_id    BLOB NOT NULL REFERENCES tenants (tenant_id),
    cache_id     BLOB NOT NULL,
    scopes       INTEGER NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (principal_id, cache_id)
);

CREATE TABLE tokens (
    credential_id BLOB PRIMARY KEY,
    tenant_id     BLOB NOT NULL REFERENCES tenants (tenant_id),
    principal_id  BLOB NOT NULL,
    name          TEXT NOT NULL,
    secret_hash   BLOB NOT NULL,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    revoked_at    INTEGER
);
CREATE INDEX tokens_principal ON tokens (principal_id);

CREATE TABLE token_grants (
    credential_id BLOB NOT NULL REFERENCES tokens (credential_id),
    cache_id      BLOB NOT NULL,
    scopes        INTEGER NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (credential_id, cache_id)
);

CREATE TABLE sessions (
    credential_id BLOB PRIMARY KEY,
    tenant_id     BLOB NOT NULL REFERENCES tenants (tenant_id),
    principal_id  BLOB NOT NULL,
    name          TEXT NOT NULL,
    secret_hash   BLOB NOT NULL,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    revoked_at    INTEGER
);
CREATE INDEX sessions_principal ON sessions (principal_id);

CREATE TABLE session_grants (
    credential_id BLOB NOT NULL REFERENCES sessions (credential_id),
    cache_id      BLOB NOT NULL,
    scopes        INTEGER NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (credential_id, cache_id)
);

CREATE TABLE rosters (
    cache_id   BLOB PRIMARY KEY REFERENCES caches (cache_id),
    version    INTEGER NOT NULL,
    epoch      INTEGER NOT NULL,
    roster     BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE roster_signers (
    cache_id BLOB NOT NULL REFERENCES caches (cache_id),
    sign_key BLOB NOT NULL,
    role     INTEGER NOT NULL,
    PRIMARY KEY (cache_id, sign_key, role)
);

CREATE TABLE packs (
    cache_id        BLOB NOT NULL REFERENCES caches (cache_id),
    pack_id         BLOB NOT NULL,
    ns              BLOB NOT NULL,
    uploader        BLOB NOT NULL,
    client_ref      BLOB NOT NULL,
    size            INTEGER NOT NULL CHECK (size > 0),
    etag            TEXT,
    live_bytes      INTEGER NOT NULL CHECK (live_bytes >= 0 AND live_bytes <= size),
    state           INTEGER NOT NULL CHECK (state IN (0, 1, 2)),
    lease_until     INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    dead_since      INTEGER,
    commit_digest   BLOB,
    commit_statuses BLOB,
    PRIMARY KEY (cache_id, pack_id),
    UNIQUE (cache_id, uploader, client_ref)
);
CREATE INDEX packs_state ON packs (state, dead_since);

CREATE TABLE segments (
    cache_id    BLOB NOT NULL,
    segment_id  BLOB NOT NULL,
    pack_id     BLOB NOT NULL,
    byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    kind        INTEGER NOT NULL CHECK (kind IN (0, 1)),
    created_at  INTEGER NOT NULL,
    dead_since  INTEGER,
    PRIMARY KEY (cache_id, segment_id),
    FOREIGN KEY (cache_id, pack_id) REFERENCES packs (cache_id, pack_id)
);
CREATE INDEX segments_pack ON segments (cache_id, pack_id);

CREATE TABLE entries (
    cache_id   BLOB NOT NULL,
    ns         BLOB NOT NULL,
    lookup_id  BLOB NOT NULL,
    segment_id BLOB NOT NULL,
    dict_id    BLOB,
    epoch      INTEGER NOT NULL CHECK (epoch >= 0),
    record     BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (cache_id, ns, lookup_id),
    FOREIGN KEY (cache_id, segment_id) REFERENCES segments (cache_id, segment_id),
    FOREIGN KEY (cache_id, dict_id) REFERENCES segments (cache_id, segment_id)
);
CREATE UNIQUE INDEX entries_segment ON entries (cache_id, segment_id);
CREATE INDEX entries_dict ON entries (cache_id, dict_id);
CREATE INDEX entries_epoch ON entries (cache_id, epoch);

CREATE TABLE entry_access (
    cache_id   BLOB NOT NULL,
    ns         BLOB NOT NULL,
    lookup_id  BLOB NOT NULL,
    access_day INTEGER NOT NULL,
    PRIMARY KEY (cache_id, ns, lookup_id),
    FOREIGN KEY (cache_id, ns, lookup_id) REFERENCES entries (cache_id, ns, lookup_id)
);
CREATE INDEX entry_access_day ON entry_access (cache_id, access_day);

CREATE TABLE dictionary_pointers (
    cache_id   BLOB PRIMARY KEY REFERENCES caches (cache_id),
    version    INTEGER NOT NULL,
    epoch      INTEGER NOT NULL,
    segment_id BLOB NOT NULL,
    pointer    BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (cache_id, segment_id) REFERENCES segments (cache_id, segment_id)
);

CREATE TABLE usage_ledger (
    node_id   BLOB NOT NULL,
    flush_seq INTEGER NOT NULL,
    tenant_id BLOB NOT NULL,
    cache_id  BLOB NOT NULL,
    day       INTEGER NOT NULL,
    metric    TEXT NOT NULL,
    value     INTEGER NOT NULL CHECK (value >= 0),
    UNIQUE (node_id, flush_seq, tenant_id, cache_id, day, metric)
);
CREATE INDEX usage_ledger_day ON usage_ledger (day);

CREATE TABLE ledger_nodes (
    node_id       BLOB PRIMARY KEY,
    max_flush_seq INTEGER NOT NULL,
    flushed_at    INTEGER NOT NULL
);

CREATE TABLE ledger_days (
    day       INTEGER PRIMARY KEY,
    closed_at INTEGER NOT NULL
);

CREATE TABLE nodes (
    node_id      BLOB PRIMARY KEY,
    heartbeat_at INTEGER NOT NULL
);

CREATE TABLE scrub_queue (
    cache_id  BLOB NOT NULL,
    pack_id   BLOB NOT NULL,
    queued_at INTEGER NOT NULL,
    PRIMARY KEY (cache_id, pack_id)
);
