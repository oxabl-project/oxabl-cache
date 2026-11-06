CREATE TABLE telemetry (
    cache_id    BYTEA NOT NULL REFERENCES caches (cache_id),
    seq         BIGINT NOT NULL CHECK (seq > 0),
    received_at BIGINT NOT NULL,
    body        BYTEA NOT NULL,
    PRIMARY KEY (cache_id, seq)
);
CREATE INDEX telemetry_received ON telemetry (cache_id, received_at);
