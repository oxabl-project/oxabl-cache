CREATE TABLE oidc_issuers (
    tenant_id  BYTEA NOT NULL REFERENCES tenants (tenant_id),
    issuer     TEXT NOT NULL,
    shape      BIGINT NOT NULL CHECK (shape IN (0, 1, 2)),
    created_at BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, issuer)
);

CREATE TABLE oidc_issuer_claims (
    tenant_id BYTEA NOT NULL,
    issuer    TEXT NOT NULL,
    claim     TEXT NOT NULL,
    PRIMARY KEY (tenant_id, issuer, claim),
    FOREIGN KEY (tenant_id, issuer) REFERENCES oidc_issuers (tenant_id, issuer)
);

CREATE TABLE workload_policies (
    policy_id  BYTEA PRIMARY KEY,
    tenant_id  BYTEA NOT NULL,
    issuer     TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (tenant_id, name),
    FOREIGN KEY (tenant_id, issuer) REFERENCES oidc_issuers (tenant_id, issuer)
);
CREATE INDEX workload_policies_issuer ON workload_policies (tenant_id, issuer);

CREATE TABLE workload_policy_claims (
    policy_id  BYTEA NOT NULL REFERENCES workload_policies (policy_id),
    claim      TEXT NOT NULL,
    value_hash BYTEA NOT NULL,
    PRIMARY KEY (policy_id, claim)
);

CREATE TABLE workload_policy_grants (
    policy_id BYTEA NOT NULL REFERENCES workload_policies (policy_id),
    cache_id  BYTEA NOT NULL,
    scopes    BIGINT NOT NULL CHECK (scopes > 0),
    PRIMARY KEY (policy_id, cache_id)
);
