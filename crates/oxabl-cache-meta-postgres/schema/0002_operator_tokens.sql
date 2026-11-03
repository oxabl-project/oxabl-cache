CREATE TABLE operator_tokens (
    credential_id BYTEA PRIMARY KEY,
    name          TEXT NOT NULL,
    secret_hash   BYTEA NOT NULL,
    created_at    BIGINT NOT NULL,
    expires_at    BIGINT NOT NULL,
    revoked_at    BIGINT
);
