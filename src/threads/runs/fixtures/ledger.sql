CREATE TABLE events(
  seq INTEGER PRIMARY KEY,
  ts TEXT NOT NULL,
  item TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('claim','release','handoff','need','unneed','run','note','capture','close','reopen','KICKOFF','STUCK','LANDED','QUESTION','LAND-READY','RECYCLE-READY','ANSWER','LEARN')),
  actor TEXT NOT NULL DEFAULT '',
  tier TEXT NOT NULL DEFAULT '',
  data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))
) STRICT;
CREATE INDEX events_item ON events(item, seq);
CREATE TABLE claims(
  item TEXT PRIMARY KEY CHECK(item GLOB '*#[0-9]*'),
  owner TEXT NOT NULL CHECK(length(owner) > 0),
  since TEXT NOT NULL,
  eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)
) STRICT;
CREATE TABLE runs(
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
  brief_origin TEXT
, brief_path TEXT, brief_sha256 TEXT) STRICT;
CREATE INDEX runs_item ON runs(item, ended);
CREATE TABLE run_usage(
  agent TEXT NOT NULL,
  request TEXT NOT NULL,
  ts TEXT NOT NULL,
  account TEXT NOT NULL,
  weight REAL NOT NULL CHECK(weight >= 0),
  PRIMARY KEY(agent, request)
) STRICT;
CREATE TABLE run_costs(account TEXT NOT NULL, started TEXT NOT NULL, ended TEXT NOT NULL, agent TEXT NOT NULL, cost_usd REAL NOT NULL, PRIMARY KEY(account, started, ended, agent)) STRICT;
CREATE INDEX run_costs_agent ON run_costs(agent);
