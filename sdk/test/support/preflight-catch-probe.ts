// Fault injection for the preflight CLI's terminal catch (thread 019f9cc2).
// The catch is the last message before an admission failure reaches an operator's
// terminal; drive it with the throw shapes that used to produce either a blank
// line or the exact string the Clojure caller printed for a subprocess it never
// heard from at all.
import { rejectionText } from "../../src/routing-economics-preflight-cli";

const shapes: Record<string, unknown> = {
  "non-error": "a bare string rejection",
  "empty-error": new Error(""),
  object: { code: "ENOENT", detail: "not an Error instance" },
};

const requested = process.argv[2] ?? "";
const thrown = shapes[requested];
if (!(requested in shapes)) {
  console.error(`preflight-catch-probe: unknown throw shape ${JSON.stringify(requested)}`);
  process.exit(2);
}

console.error(rejectionText(thrown));
process.exit(1);
