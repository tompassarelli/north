;; Cockpit is a Store-only snapshot projection: authority, attempt fences,
;; replay position, and the conservative next tag all come from these facts.
;; BEAGLE_STORE_OUT=/path/to/store/out bb -cp out:"$BEAGLE_STORE_OUT" tests/cockpit_projection_test.clj
(require '[clojure.string :as str]
         '[north.main :as m]
         '[store.types :as t])

(def facts
  [(t/triple "@thread:dispatch" "title" "Store-only cockpit fixture")
   (t/triple "@thread:dispatch" "kind" "thread")
   (t/triple "@account:primary" "kind" "provider_account")
   (t/triple "@account:primary" "account_id" "primary")
   (t/triple "@account:primary" "provider" "codex")
   (t/triple "@account:primary" "provider_profile" "personal")
   (t/triple "@account:primary" "account_role" "execution")
   (t/triple "@account:primary" "execution_eligible" "true")
   (t/triple "@account:primary" "headroom" "82")
   (t/triple "@attempt:attempt-sha" "kind" "execution_attempt")
   (t/triple "@attempt:attempt-sha" "execution_attempt_manifest_sha256" "attempt-sha")
   (t/triple "@attempt:attempt-sha" "execution_attempt_run" "run-1")
   (t/triple "@attempt:attempt-sha" "execution_attempt_thread" "@thread:dispatch")
   (t/triple "@attempt:attempt-sha" "execution_attempt_account" "@account:primary")
   (t/triple "@attempt:attempt-sha" "execution_attempt_reserved_at" "2026-08-20T10:00:00")
   (t/triple "@attempt:attempt-sha" "execution_attempt_launch_intent_sha256" "launch-sha")
   (t/triple "@attempt:attempt-sha" "execution_attempt_provider_start_manifest_sha256" "start-sha")
   (t/triple "@attempt:attempt-sha" "execution_attempt_thread_lease" "{\"epoch\":7,\"holder\":\"attempt-sha\",\"resource\":\"thread:dispatch:dispatch\"}")
   (t/triple "@attempt:attempt-sha" "execution_attempt_account_lease" "{\"epoch\":8,\"holder\":\"attempt-sha\",\"resource\":\"codex-account:primary:slot:0\"}")
   (t/triple "@bridge-command:attempt-sha:1" "bridge.command/attempt-id" "attempt-sha")
   (t/triple "@bridge-command:attempt-sha:1" "bridge.command/ordinal" "1")
   (t/triple "@bridge-command:attempt-sha:1" "bridge.command/kind" "submit-input")
   (t/triple "@bridge-command:attempt-sha:1" "bridge.command/delivery" "queued-next-turn")
   (t/triple "@run:wire-0" "wire_run_id" "run-1")
   (t/triple "@run:wire-0" "wire_event_sequence" "0")
   (t/triple "@run:wire-0" "wire_event_json" "{\"kind\":\"reserved\"}")
   (t/triple "@run:wire-0" "wire_event_sha256" "digest-0")
   (t/triple "@run:wire-1" "wire_run_id" "run-1")
   (t/triple "@run:wire-1" "wire_event_sequence" "1")
   (t/triple "@run:wire-1" "wire_event_json" "{\"kind\":\"launch-intent\"}")
   (t/triple "@run:wire-1" "wire_event_sha256" "digest-1")
   ;; A gap must not inflate the greatest contiguous replay position.
   (t/triple "@run:wire-3" "wire_run_id" "run-1")
   (t/triple "@run:wire-3" "wire_event_sequence" "3")
   (t/triple "@run:wire-3" "wire_event_json" "{\"kind\":\"started\"}")
   (t/triple "@run:wire-3" "wire_event_sha256" "digest-3")])

(def output
  (with-redefs-fn {#'m/live-facts (fn [_] facts)}
    #(with-out-str (m/cmd-cockpit "ignored"))))

(def checks
  [["account role and eligibility come from account facts"
    (and (str/includes? output "role execution")
         (str/includes? output "execution-eligible true"))]
   ["attempt state and both Store lease fences are shown"
    (and (str/includes? output "provider-started")
         (str/includes? output "thread:dispatch:dispatch")
         (str/includes? output "codex-account:primary:slot:0"))]
   ["replay position stops at the greatest contiguous Store sequence"
    (str/includes? output "replay 1")]
   ["safe next derives an undelivered command from Store facts"
    (str/includes? output "safe-next send/submit-input")]])

(let [fails (remove second checks)]
  (doseq [[name ok] checks]
    (println (if ok "  [PASS] " "  [FAIL] ") name))
  (if (empty? fails)
    (println "\ncockpit projection:" (count checks) "/" (count checks) "PASS")
    (do
      (println "\ncockpit projection:" (count fails) "FAILED")
      (System/exit 1))))
