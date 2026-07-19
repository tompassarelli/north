// Suite-wide hermeticity boundary (loaded via bunfig.toml [test] preload,
// before any test module is imported).
//
// When the suite runs inside a live managed north lane, the ambient
// NORTH_PORT points at the REAL coordinator on the session's port. Admission
// (requireCoordinator) and presence probes read NORTH_PORT, so a test that
// builds harnessOptions without first pinning its own fake coordinator would
// silently target the live coordinator. Because process.env is shared across
// test files and every coordinator-bearing suite snapshots+restores NORTH_PORT
// at module load, that live value leaks and races between files — surfacing as
// nondeterministic `north_coordinator_preflight_invalid_response` failures.
//
// Pin NORTH_PORT to a closed sentinel BEFORE any module snapshot, so every
// save/restore cycle carries the dead port, never the live coordinator. Suites
// that need a coordinator still stand up their own server and set NORTH_PORT to
// it in beforeAll/beforeEach; suites that assert a dead coordinator get exactly
// that. This is the "live coordinator = env hermeticity" contract in practice.
//
// A closed high port (nothing listens here) — connect() yields ECONNREFUSED,
// which admission treats as an unavailable coordinator, the honest default for
// a hermetic unit test.
const HERMETIC_DEAD_PORT = "59319";

if (process.env.NORTH_TEST_ALLOW_AMBIENT_COORDINATOR !== "1") {
  process.env.NORTH_PORT = HERMETIC_DEAD_PORT;
}
