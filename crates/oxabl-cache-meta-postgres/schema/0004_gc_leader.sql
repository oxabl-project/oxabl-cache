CREATE TABLE gc_leader (
    id    SMALLINT PRIMARY KEY CHECK (id = 1),
    epoch BIGINT NOT NULL CHECK (epoch >= 0)
);
INSERT INTO gc_leader (id, epoch) VALUES (1, 0);
