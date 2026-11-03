ALTER TABLE tenants ADD COLUMN deleted_at INTEGER;
ALTER TABLE caches ADD COLUMN deleted_at INTEGER;
CREATE INDEX tenants_deleted ON tenants (deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX caches_deleted ON caches (deleted_at) WHERE deleted_at IS NOT NULL;
