CREATE TABLE oidc_issuers (
    tenant_id  BLOB NOT NULL REFERENCES tenants (tenant_id),
    issuer     TEXT NOT NULL,
    shape      INTEGER NOT NULL CHECK (shape IN (0, 1, 2)),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, issuer)
);

CREATE TABLE oidc_issuer_claims (
    tenant_id BLOB NOT NULL,
    issuer    TEXT NOT NULL,
    claim     TEXT NOT NULL,
    PRIMARY KEY (tenant_id, issuer, claim),
    FOREIGN KEY (tenant_id, issuer) REFERENCES oidc_issuers (tenant_id, issuer)
);

CREATE TABLE workload_policies (
    policy_id  BLOB PRIMARY KEY,
    tenant_id  BLOB NOT NULL,
    issuer     TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (tenant_id, name),
    FOREIGN KEY (tenant_id, issuer) REFERENCES oidc_issuers (tenant_id, issuer)
);
CREATE INDEX workload_policies_issuer ON workload_policies (tenant_id, issuer);

CREATE TABLE workload_policy_claims (
    policy_id  BLOB NOT NULL REFERENCES workload_policies (policy_id),
    claim      TEXT NOT NULL,
    value_hash BLOB NOT NULL,
    PRIMARY KEY (policy_id, claim)
);

CREATE TABLE workload_policy_grants (
    policy_id BLOB NOT NULL REFERENCES workload_policies (policy_id),
    cache_id  BLOB NOT NULL,
    scopes    INTEGER NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (policy_id, cache_id)
);
