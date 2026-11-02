CREATE TABLE operator_tokens (
    credential_id BLOB PRIMARY KEY,
    name          TEXT NOT NULL,
    secret_hash   BLOB NOT NULL,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    revoked_at    INTEGER
);
