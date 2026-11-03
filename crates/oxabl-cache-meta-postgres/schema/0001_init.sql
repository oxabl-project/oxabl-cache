CREATE TABLE tenants (
    tenant_id   BYTEA PRIMARY KEY,
    name        TEXT NOT NULL,
    status      BIGINT NOT NULL CHECK (status IN (0, 1)),
    hard_bytes  BIGINT CHECK (hard_bytes IS NULL OR hard_bytes >= 0),
    egress_bytes BIGINT CHECK (egress_bytes IS NULL OR egress_bytes >= 0),
    rate        BIGINT NOT NULL,
    burst       BIGINT NOT NULL,
    created_at  BIGINT NOT NULL
);

CREATE TABLE caches (
    cache_id       BYTEA PRIMARY KEY,
    tenant_id      BYTEA NOT NULL REFERENCES tenants (tenant_id),
    display_name   TEXT NOT NULL,
    retention_days BIGINT NOT NULL CHECK (retention_days >= 0),
    hard_bytes     BIGINT NOT NULL CHECK (hard_bytes >= 0),
    epoch          BIGINT NOT NULL CHECK (epoch >= 0),
    roster_version BIGINT NOT NULL CHECK (roster_version >= 0),
    live_bytes     BIGINT NOT NULL CHECK (live_bytes >= 0),
    pending_bytes  BIGINT NOT NULL CHECK (pending_bytes >= 0),
    created_at     BIGINT NOT NULL
);
CREATE INDEX caches_tenant ON caches (tenant_id);

CREATE TABLE cache_owner_keys (
    cache_id BYTEA NOT NULL REFERENCES caches (cache_id),
    sign_key BYTEA NOT NULL,
    PRIMARY KEY (cache_id, sign_key)
);

CREATE TABLE users (
    principal_id BYTEA PRIMARY KEY,
    tenant_id    BYTEA NOT NULL REFERENCES tenants (tenant_id),
    issuer       TEXT NOT NULL,
    subject      TEXT NOT NULL,
    label        TEXT NOT NULL,
    created_at   BIGINT NOT NULL,
    disabled_at  BIGINT,
    UNIQUE (tenant_id, issuer, subject)
);

CREATE TABLE grants (
    principal_id BYTEA NOT NULL,
    tenant_id    BYTEA NOT NULL REFERENCES tenants (tenant_id),
    cache_id     BYTEA NOT NULL,
    scopes       BIGINT NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (principal_id, cache_id)
);

CREATE TABLE tokens (
    credential_id BYTEA PRIMARY KEY,
    tenant_id     BYTEA NOT NULL REFERENCES tenants (tenant_id),
    principal_id  BYTEA NOT NULL,
    name          TEXT NOT NULL,
    secret_hash   BYTEA NOT NULL,
    created_at    BIGINT NOT NULL,
    expires_at    BIGINT NOT NULL,
    revoked_at    BIGINT
);
CREATE INDEX tokens_principal ON tokens (principal_id);

CREATE TABLE token_grants (
    credential_id BYTEA NOT NULL REFERENCES tokens (credential_id),
    cache_id      BYTEA NOT NULL,
    scopes        BIGINT NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (credential_id, cache_id)
);

CREATE TABLE sessions (
    credential_id BYTEA PRIMARY KEY,
    tenant_id     BYTEA NOT NULL REFERENCES tenants (tenant_id),
    principal_id  BYTEA NOT NULL,
    name          TEXT NOT NULL,
    secret_hash   BYTEA NOT NULL,
    created_at    BIGINT NOT NULL,
    expires_at    BIGINT NOT NULL,
    revoked_at    BIGINT
);
CREATE INDEX sessions_principal ON sessions (principal_id);

CREATE TABLE session_grants (
    credential_id BYTEA NOT NULL REFERENCES sessions (credential_id),
    cache_id      BYTEA NOT NULL,
    scopes        BIGINT NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (credential_id, cache_id)
);

CREATE TABLE rosters (
    cache_id   BYTEA PRIMARY KEY REFERENCES caches (cache_id),
    version    BIGINT NOT NULL,
    epoch      BIGINT NOT NULL,
    roster     BYTEA NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE roster_signers (
    cache_id BYTEA NOT NULL REFERENCES caches (cache_id),
    sign_key BYTEA NOT NULL,
    role     BIGINT NOT NULL,
    PRIMARY KEY (cache_id, sign_key, role)
);

CREATE TABLE packs (
    cache_id        BYTEA NOT NULL REFERENCES caches (cache_id),
    pack_id         BYTEA NOT NULL,
    ns              BYTEA NOT NULL,
    uploader        BYTEA NOT NULL,
    client_ref      BYTEA NOT NULL,
    size            BIGINT NOT NULL CHECK (size > 0),
    etag            TEXT,
    live_bytes      BIGINT NOT NULL CHECK (live_bytes >= 0 AND live_bytes <= size),
    state           BIGINT NOT NULL CHECK (state IN (0, 1, 2)),
    lease_until     BIGINT NOT NULL,
    created_at      BIGINT NOT NULL,
    dead_since      BIGINT,
    commit_digest   BYTEA,
    commit_statuses BYTEA,
    PRIMARY KEY (cache_id, pack_id),
    UNIQUE (cache_id, uploader, client_ref)
);
CREATE INDEX packs_state ON packs (state, dead_since);

CREATE TABLE segments (
    cache_id    BYTEA NOT NULL,
    segment_id  BYTEA NOT NULL,
    pack_id     BYTEA NOT NULL,
    byte_offset BIGINT NOT NULL CHECK (byte_offset >= 0),
    byte_length BIGINT NOT NULL CHECK (byte_length > 0),
    kind        BIGINT NOT NULL CHECK (kind IN (0, 1)),
    created_at  BIGINT NOT NULL,
    dead_since  BIGINT,
    PRIMARY KEY (cache_id, segment_id),
    FOREIGN KEY (cache_id, pack_id) REFERENCES packs (cache_id, pack_id)
);
CREATE INDEX segments_pack ON segments (cache_id, pack_id);

CREATE TABLE entries (
    cache_id   BYTEA NOT NULL,
    ns         BYTEA NOT NULL,
    lookup_id  BYTEA NOT NULL,
    segment_id BYTEA NOT NULL,
    dict_id    BYTEA,
    epoch      BIGINT NOT NULL CHECK (epoch >= 0),
    record     BYTEA NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (cache_id, ns, lookup_id),
    FOREIGN KEY (cache_id, segment_id) REFERENCES segments (cache_id, segment_id),
    FOREIGN KEY (cache_id, dict_id) REFERENCES segments (cache_id, segment_id)
);
CREATE UNIQUE INDEX entries_segment ON entries (cache_id, segment_id);
CREATE INDEX entries_dict ON entries (cache_id, dict_id);
CREATE INDEX entries_epoch ON entries (cache_id, epoch);

CREATE TABLE entry_access (
    cache_id   BYTEA NOT NULL,
    ns         BYTEA NOT NULL,
    lookup_id  BYTEA NOT NULL,
    access_day BIGINT NOT NULL,
    PRIMARY KEY (cache_id, ns, lookup_id),
    FOREIGN KEY (cache_id, ns, lookup_id) REFERENCES entries (cache_id, ns, lookup_id)
);
CREATE INDEX entry_access_day ON entry_access (cache_id, access_day);

CREATE TABLE dictionary_pointers (
    cache_id   BYTEA PRIMARY KEY REFERENCES caches (cache_id),
    version    BIGINT NOT NULL,
    epoch      BIGINT NOT NULL,
    segment_id BYTEA NOT NULL,
    pointer    BYTEA NOT NULL,
    updated_at BIGINT NOT NULL,
    FOREIGN KEY (cache_id, segment_id) REFERENCES segments (cache_id, segment_id)
);

CREATE TABLE usage_ledger (
    node_id   BYTEA NOT NULL,
    flush_seq BIGINT NOT NULL,
    tenant_id BYTEA NOT NULL,
    cache_id  BYTEA NOT NULL,
    day       BIGINT NOT NULL,
    metric    TEXT NOT NULL,
    value     BIGINT NOT NULL CHECK (value >= 0),
    UNIQUE (node_id, flush_seq, tenant_id, cache_id, day, metric)
);
CREATE INDEX usage_ledger_day ON usage_ledger (day);

CREATE TABLE ledger_nodes (
    node_id       BYTEA PRIMARY KEY,
    max_flush_seq BIGINT NOT NULL,
    flushed_at    BIGINT NOT NULL
);

CREATE TABLE ledger_days (
    day       BIGINT PRIMARY KEY,
    closed_at BIGINT NOT NULL
);

CREATE TABLE nodes (
    node_id      BYTEA PRIMARY KEY,
    heartbeat_at BIGINT NOT NULL
);

CREATE TABLE scrub_queue (
    cache_id  BYTEA NOT NULL,
    pack_id   BYTEA NOT NULL,
    queued_at BIGINT NOT NULL,
    PRIMARY KEY (cache_id, pack_id)
);
