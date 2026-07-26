#!/usr/bin/env bb
;; routing-preflight-failure-naming-test.clj — thread 019f9cc2.
;;
;; Observed 2026-07-26: three simultaneous `north delegate` admissions (role
;; executor, three different threads, one openai pin with valid pin-evidence);
;; two died printing exactly "routing economics preflight failed" and NOTHING
;; else, while the third admitted. An immediate sequential retry of the two
;; admitted fine.
;;
;; The bare string was the delegate CLI's empty-output fallback. `run` reports a
;; subprocess timeout as {:timeout true :ok false} with NEITHER :out NOR :err,
;; so the message builder saw an empty string and printed an adjective that
;; named no layer, no budget and no cause — while the concurrency defect that
;; actually fired (a cold coordinator scan under three concurrent preflights
;; overrunning the old 10 000 ms ceiling) stayed completely invisible.
;;
;; This pins BOTH halves of the repair:
;;   1. every non-ok `run` shape produces a message that names its own cause;
;;   2. the outer preflight budget stays strictly above the projector budgets it
;;      wraps, so an inner NAMED refusal always wins the race against the outer
;;      kill. Invert that ordering and defect (A) comes straight back.

(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(def message #'preflight-failure-message)
(def budget routing-economics-preflight-timeout-ms)

;; The subprocess DID speak: carry its rejection verbatim, never a summary.
(let [rejection (str "managed North routing preflight explicit provider/account/model "
                     "selectors require current typed pinEvidence")]
  (check "preflight failure carries the subprocess's own rejection text verbatim"
         (= rejection (message {:ok false :exit 1 :err rejection :out ""} budget)))
  (check "preflight failure joins stderr and stdout when both spoke"
         (= (str rejection "\n{\"partial\":true}")
            (message {:ok false :exit 1 :err rejection :out "{\"partial\":true}"} budget))))

;; The three shapes that carry NO subprocess output. Each must name itself.
(let [timed-out (message {:timeout true :ok false} budget)]
  (check "preflight timeout names the budget it exceeded"
         (and (str/includes? timed-out (str budget "ms"))
              (str/includes? timed-out "exceeded")))
  (check "preflight timeout points at the contended coordinator"
         (str/includes? timed-out "coordinator"))
  (check "preflight timeout is never the bare fallback adjective"
         (not= (str/trim timed-out) "routing economics preflight failed")))

(check "preflight spawn failure names the exec error"
       (str/includes? (message {:error "Cannot run program \"bun\"" :ok false} budget)
                      "Cannot run program"))

(check "preflight silent non-zero exit names the exit code"
       (str/includes? (message {:ok false :exit 137 :out "" :err ""} budget)
                      "exited 137"))

;; `run`'s own catch must never hand back a nil :error: .getMessage is nil for
;; several JDK process exceptions, and a nil reads exactly like "nothing was
;; reported at all" — the same swallow one layer down.
(check "run names a spawn exception even when getMessage is nil"
       (let [result (run ["/nonexistent/north-preflight-probe"] :timeout 1000)]
         (and (false? (:ok result))
              (seq (str (:error result))))))

;; Budget ORDERING is the structural half of the fix. The preflight subprocess
;; makes up to three `bb orchestration-project-cli.clj` round trips (staffing
;; bundle, policy-pin, catalog-pin) at 30 000 ms each plus a 5 000 ms canonical
;; assessment validator. If the outer budget drops below that reachable inner
;; ceiling, the outer kill destroys the inner named refusal again.
(check "preflight outer budget exceeds the reachable inner projector ceiling"
       (> budget (+ (* 3 30000) 5000)))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nrouting preflight failure naming: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
