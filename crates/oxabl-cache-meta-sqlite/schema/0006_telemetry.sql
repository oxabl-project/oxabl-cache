CREATE TABLE telemetry (
    cache_id    BLOB NOT NULL REFERENCES caches (cache_id),
    seq         INTEGER NOT NULL CHECK (seq > 0),
    received_at INTEGER NOT NULL,
    body        BLOB NOT NULL,
    PRIMARY KEY (cache_id, seq)
);
CREATE INDEX telemetry_received ON telemetry (cache_id, received_at);
