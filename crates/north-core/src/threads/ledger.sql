CREATE TABLE IF NOT EXISTS events(
  seq INTEGER PRIMARY KEY,
  ts TEXT NOT NULL,
  item TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('claim','release','handoff','need','unneed','run','note','capture','close','reopen','MESSAGE','GATE','GATE-ANSWER','JOB-START','JOB-BEAT','JOB-EXIT','CONTROL','KICKOFF','STUCK','LANDED','QUESTION','LAND-READY','RECYCLE-READY','ANSWER','LEARN')),
  actor TEXT NOT NULL DEFAULT '',
  tier TEXT NOT NULL DEFAULT '',
  data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))
) STRICT;
CREATE TABLE IF NOT EXISTS claims(
  item TEXT PRIMARY KEY CHECK(item GLOB '*#[0-9]*'),
  owner TEXT NOT NULL CHECK(length(owner) > 0),
  since TEXT NOT NULL,
  eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)
) STRICT;
CREATE TABLE IF NOT EXISTS runs(
  agent TEXT PRIMARY KEY,
  item TEXT,
  follows TEXT,
  session TEXT,
  tier TEXT NOT NULL,
  category TEXT NOT NULL,
  started TEXT,
  ended TEXT NOT NULL,
  minutes INTEGER NOT NULL CHECK(minutes >= 0),
  eta_min INTEGER,
  tokens INTEGER,
  peak_ctx INTEGER,
  outcome TEXT NOT NULL CHECK(outcome IN ('done','not done','escalated','handoff','split','unclear','running','idle')),
  report TEXT NOT NULL DEFAULT '',
  note TEXT,
  repeats INTEGER,
  landed INTEGER,
  arm TEXT,
  spec TEXT,
  scope TEXT,
  surface TEXT,
  verify TEXT,
  lens TEXT,
  flaws INTEGER,
  expects_landing INTEGER,
  staffed TEXT,
  first_commit TEXT,
  review_requested TEXT,
  landed_at TEXT,
  landed_sha TEXT,
  turns INTEGER,
  role TEXT NOT NULL DEFAULT 'worker',
  compactions INTEGER,
  handoff_fires INTEGER,
  handoff_ignored INTEGER,
  input_tokens INTEGER,
  cache_read_tokens INTEGER,
  idle_gap_misses INTEGER,
  account TEXT,
  cost_usd REAL,
  model TEXT,
  effort TEXT,
  fast INTEGER CHECK(fast IN (0, 1)),
  brief_text TEXT,
  brief_origin TEXT,
  provider TEXT,
  brief_path TEXT,
  brief_sha256 TEXT,
  tool_calls INTEGER,
  active_min REAL,
  wait_min REAL
) STRICT;
CREATE TABLE IF NOT EXISTS run_usage(
  agent TEXT NOT NULL,
  request TEXT NOT NULL,
  ts TEXT NOT NULL,
  account TEXT NOT NULL,
  weight REAL NOT NULL CHECK(weight >= 0),
  PRIMARY KEY(agent, request)
) STRICT;
CREATE TABLE IF NOT EXISTS tier_choice(
  cell TEXT PRIMARY KEY,
  tier TEXT NOT NULL,
  since TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS gh_cache(
  repo TEXT PRIMARY KEY,
  fetched REAL NOT NULL,
  issues TEXT NOT NULL CHECK(json_valid(issues))
) STRICT;
CREATE TABLE IF NOT EXISTS blocks(
  item TEXT PRIMARY KEY,
  reason TEXT NOT NULL,
  since TEXT NOT NULL,
  by TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS run_events(
  agent TEXT NOT NULL,
  ts TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('turn','tool_error','retry')),
  detail TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TABLE IF NOT EXISTS job_runs(
  name TEXT NOT NULL,
  host TEXT NOT NULL,
  started TEXT NOT NULL,
  finished TEXT,
  rc INTEGER,
  PRIMARY KEY(name, host, started)
) STRICT;
CREATE TABLE IF NOT EXISTS job_queue(
  id INTEGER PRIMARY KEY,
  placement TEXT NOT NULL CHECK(length(placement) > 0),
  item TEXT,
  cmd TEXT NOT NULL CHECK(json_valid(cmd)),
  state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','taken','done','failed','dropped','requeued')),
  queued TEXT NOT NULL,
  taken TEXT,
  finished TEXT,
  result TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS items(
  ns TEXT NOT NULL,
  number INTEGER NOT NULL CHECK(number > 0),
  title TEXT NOT NULL CHECK(length(title) > 0),
  body TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL DEFAULT 'open' CHECK(state IN ('open','closed')),
  created TEXT NOT NULL,
  closed TEXT,
  due TEXT,
  PRIMARY KEY(ns, number)
) STRICT;
CREATE TABLE IF NOT EXISTS item_needs(
  item TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY(item, needs)
) STRICT;
CREATE TABLE IF NOT EXISTS run_costs(
  account TEXT NOT NULL,
  started TEXT NOT NULL,
  ended TEXT NOT NULL,
  agent TEXT NOT NULL,
  cost_usd REAL NOT NULL,
  PRIMARY KEY(account, started, ended, agent)
) STRICT;
CREATE TABLE IF NOT EXISTS calibration(
  at TEXT NOT NULL,
  kind TEXT NOT NULL,
  cell TEXT NOT NULL,
  n INTEGER NOT NULL,
  p25 REAL,
  median REAL,
  p75 REAL,
  value REAL,
  source TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS events_item ON events(item, seq);
CREATE INDEX IF NOT EXISTS run_events_agent ON run_events(agent, ts);
CREATE INDEX IF NOT EXISTS runs_item ON runs(item, ended);
CREATE INDEX IF NOT EXISTS run_costs_agent ON run_costs(agent);
