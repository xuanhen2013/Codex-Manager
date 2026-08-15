CREATE TABLE IF NOT EXISTS proxy_pools (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  failure_threshold INTEGER NOT NULL DEFAULT 3,
  recovery_threshold INTEGER NOT NULL DEFAULT 2,
  heartbeat_interval_secs INTEGER NOT NULL DEFAULT 60,
  cooldown_secs INTEGER NOT NULL DEFAULT 300,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS proxy_pool_members (
  pool_id TEXT NOT NULL REFERENCES proxy_pools(id) ON DELETE CASCADE,
  proxy_profile_id TEXT NOT NULL UNIQUE REFERENCES proxy_profiles(id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  health_status TEXT NOT NULL DEFAULT 'unchecked',
  consecutive_failures INTEGER NOT NULL DEFAULT 0,
  consecutive_successes INTEGER NOT NULL DEFAULT 0,
  cooldown_until INTEGER,
  last_check_at INTEGER,
  last_error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (pool_id, proxy_profile_id)
);

CREATE TABLE IF NOT EXISTS account_proxy_pool_bindings (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  pool_id TEXT NOT NULL REFERENCES proxy_pools(id) ON DELETE CASCADE,
  current_proxy_profile_id TEXT REFERENCES proxy_profiles(id) ON DELETE SET NULL,
  assigned_at INTEGER NOT NULL,
  last_switched_at INTEGER,
  last_switch_reason TEXT,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS proxy_pool_switch_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  pool_id TEXT NOT NULL REFERENCES proxy_pools(id) ON DELETE CASCADE,
  from_proxy_profile_id TEXT,
  to_proxy_profile_id TEXT,
  reason TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_proxy_pool_members_pool_order
  ON proxy_pool_members(pool_id, enabled DESC, sort_order ASC, proxy_profile_id ASC);

CREATE INDEX IF NOT EXISTS idx_proxy_pool_members_health
  ON proxy_pool_members(health_status, cooldown_until, last_check_at);

CREATE INDEX IF NOT EXISTS idx_account_proxy_pool_bindings_pool
  ON account_proxy_pool_bindings(pool_id, current_proxy_profile_id, account_id);

CREATE INDEX IF NOT EXISTS idx_proxy_pool_switch_logs_account_created
  ON proxy_pool_switch_logs(account_id, created_at DESC, id DESC);
