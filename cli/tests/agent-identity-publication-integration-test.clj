#!/usr/bin/env bb
;; Exact managed-identity publication against a throwaway Fram coordinator.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                "/home/tom/code/beagle/main/branch-core"))))
(when-not (.isFile (io/file fram "bin/fram-server"))
  (throw (ex-info "current Beagle branch-core engine is required" {:fram fram})))
(def writer (str root "/cli/agent-fact-internal.clj"))
(def test-terminal-publication-order
  ["process_outcome" "delivery_evidence" "delivery_evidence_sha256"
   "delivery_attestation" "delivery_attestation_sha256"
   "delivery_outcome" "delivery_reason" "outcome"])
(def test-route-generation-predicates
  #{"provider" "provider_target" "live_input" "live_input_state"
    "live_input_epoch" "model" "effort" "display_handle"})
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/agent-provenance.clj"))
(load-file (str root "/cli/terminal-projection.clj"))

(def checks (atom []))
(def test-log (atom nil))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port [] (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try (with-open [socket (java.net.Socket.)]
         (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100) true)
       (catch Exception _ false)))
(defn eventually [predicate]
  (loop [n 0]
    (cond (predicate) true (>= n 200) false
          :else (do (Thread/sleep 25) (recur (inc n))))))
(defn run-writer
  ([port operation subject value]
   (run-writer port operation subject value {}))
  ([port operation subject value extra-env]
   (let [result (proc/shell {:out :string :err :string :continue true
                             :extra-env (assoc extra-env
                                               "FRAM_LOG" @test-log)}
                            "bb" writer (str port) operation subject value)]
     {:exit (:exit result) :out (:out result) :err (:err result)})))
(defn run-managed-writer
  ([port operation subject value holder operation-id desired expected]
   (run-managed-writer
    port operation subject value holder operation-id desired expected ""))
  ([port operation subject value holder operation-id desired expected terminal-thread]
   (let [result
         (proc/shell
          {:out :string :err :string :continue true
           :extra-env {"FRAM_LOG" @test-log}}
          "bb" writer (str port) operation subject value holder operation-id
          (if desired (json/generate-string desired) "")
          (if expected (json/generate-string expected) "")
          terminal-thread)]
     {:exit (:exit result)
      :out (:out result)
      :err (:err result)
      :result
      (try
        (:result (json/parse-string
                  (or (last (str/split-lines (:out result))) "") true))
        (catch Throwable _ nil))})))
(defn entity-facts [port subject]
  (let [rows (north.coord/show-rows port subject)]
    (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))
(defn scalar-facts [facts]
  (into {} (keep (fn [[predicate values]]
                   (when (= 1 (count values)) [predicate (first values)]))) facts))
(defn reserve-run!
  [port run reporter thread capability-digest & [baseline]]
  (let [baseline (or baseline ["tests pass"])
        projection
        (sorted-map
         "run_capability_sha256" capability-digest
         "run_reservation_agent" reporter
         "run_reservation_contract_origin"
         (if (seq baseline) "accepted" "worker-defined")
         "run_reservation_done_when" (json/generate-string baseline)
         "run_reservation_thread" thread
         "run_reservation_version" north.terminal-projection/run-reservation-version
         "run_reserved_at" "2026-07-18T09:59:00Z")
        marker
        (north.terminal-projection/run-reservation-manifest-sha256 projection)]
    (doseq [[predicate value] projection]
      (north.coord/append! port run predicate value))
    (north.coord/append! port run "run_reservation_manifest_sha256" marker)))
(defn log-ops [port lower-exclusive]
  (:events
   (north.coord/occurrence-window
    port lower-exclusive (north.coord/cur-ver port))))

(defn identity-write-resource [subject]
  (str "managed-agent-write:"
       (north.terminal-projection/sha256 subject)))

(defn release-lease! [port {:keys [resource holder epoch]}]
  (north.coord/release-lease!
   port {:resource resource :holder holder :epoch epoch}))

(defn seed-identity! [port subject identity]
  (doseq [[predicate value] identity]
    (north.coord/append! port subject predicate value))
  (north.coord/append!
   port subject "identity_manifest_sha256"
   (north.agent-provenance/manifest-sha256 identity)))

(defn apply-prefix! [port subject operations count]
  (doseq [[operation predicate value] (take count operations)]
    (case operation
      :put (north.coord/append! port subject predicate value)
      :retract (north.coord/retract! port subject predicate value))))

(let [port (free-port)
      tmp (.toFile (java.nio.file.Files/createTempDirectory
                    "north-identity-publication" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "coordination.framlog")
      daemon (do
               (proc/process
                {:dir fram :out :string :err :string
                 :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                             "FRAM_SERVER_QUIET" "1"
                             "FRAM_SERVER_XMX" "1g"}}
                (str fram "/bin/fram-server") "serve" (str port)
                (.getCanonicalPath log) "north-coordination"))
      subject "@agent:identity-publication-probe"
      preset {"kind" "lane" "role" "integrator" "model" "claude-opus-4-8"
              "provider" "anthropic" "provider_target" "claude-a" "effort" "high"
              "live_input" "streaming" "live_input_state" "armed"
              "live_input_epoch" "00000000-0000-4000-8000-000000000101"
              "composition_kind" "preset" "composition_id" "integrator"
              "composition_overrides" "[\"tier\"]"
              "composition_override_reason" "critical seam" "repo" "north"
              "goal" "prove atomic publication" "spawned_at" "2026-07-17T01:00:00Z"
              "display_handle" "anthropic-a-opus-high-orchestration-integrator-probe"
              "display_name" "anthropic:claude-a · opus · high · orchestration:integrator"}
      bespoke {"kind" "lane" "role" "migration-forensics" "model" "gpt-5.6-sol"
               "provider" "openai" "provider_target" "codex-b" "effort" "xhigh"
               "live_input" "turn-framed" "live_input_state" "frozen"
               "live_input_epoch" "00000000-0000-4000-8000-000000000102"
               "composition_kind" "bespoke" "composition_id" "migration-forensics"
               "nearest_preset" "analyst" "bespoke_reason" "cross-schema archaeology"
               "promotion_candidate" "false"
               "composition_contract_sha256" (apply str (repeat 64 "a"))
               "composition_contract_fingerprint_version" "v1"
               "composition_contract_fingerprint_domain" "north:bespoke-contract:v1"
               "repo" "north" "goal" "prove clean sequential reuse"
               "spawned_at" "2026-07-17T01:01:00Z"
               "display_handle" "openai-b-sol-xhigh-orchestration-bespoke-probe"
               "display_name" "openai:codex-b · sol · xhigh · orchestration:bespoke:migration-forensics"}]
  (reset! test-log (.getCanonicalPath log))
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] @test-log)))
  (try
    (check "throwaway current Fram server starts"
           (eventually
            #(try
               (let [status (north.coord/status port)]
                 (and (= :ready (:state status))
                      (= "north-coordination" (:space-id status))))
               (catch Exception _ false))))
    (let [first-result (run-writer port "publish" subject (json/generate-string preset))
          stored (scalar-facts (entity-facts port subject))]
      (check "preset publication returns a synchronous acknowledgement" (zero? (:exit first-result)))
      (check "commit marker matches the exact current canonical projection"
             (= (north.agent-provenance/manifest-sha256 stored)
                (get stored "identity_manifest_sha256"))))

    ;; Writable lanes provision an isolated worktree and publish its abspath +
    ;; branch alongside the base identity. Regression: validate-publish! once
    ;; rejected both with "unsupported managed identity predicate", so no
    ;; worktree-allocated lane could reach startup acknowledgement.
    (let [writable-subject "@agent:identity-publication-writable"
          writable (assoc preset
                          "worktree" "/home/tom/code/worktrees/north/lane-probe"
                          "branch" "agent/lane-probe")
          result (run-writer port "publish" writable-subject
                             (json/generate-string writable))
          raw-stored (entity-facts port writable-subject)
          stored (scalar-facts raw-stored)]
      (check "writable identity carrying worktree+branch publishes"
             (zero? (:exit result)))
      (check "writable identity stores exactly one worktree and one branch value"
             (and (= #{"/home/tom/code/worktrees/north/lane-probe"}
                     (get raw-stored "worktree"))
                  (= #{"agent/lane-probe"} (get raw-stored "branch"))))
      (check "identity marker binds worktree+branch across writer and reader"
             (and (= "/home/tom/code/worktrees/north/lane-probe"
                     (get stored "worktree"))
                  (= "agent/lane-probe" (get stored "branch"))
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    ;; Bounded auto-retry: a fresh identity carrying retry_of_agent (bare prior
    ;; agent id) must publish exactly like any other optional identity fact
    ;; (worktree/branch above). Regression: identity-predicates once omitted
    ;; retry_of_agent, so validate-publish! rejected the SDK's own emitted
    ;; retry_of_agent as an unsupported managed identity predicate.
    (let [retry-subject "@agent:identity-publication-retry-lane"
          retry-identity (assoc preset "retry_of_agent" "identity-publication-dead-lane")
          result (run-writer port "publish" retry-subject
                             (json/generate-string retry-identity))
          raw-stored (entity-facts port retry-subject)
          stored (scalar-facts raw-stored)]
      (check "retry lane carrying retry_of_agent publishes"
             (zero? (:exit result)))
      (check "retry lane stores exactly one retry_of_agent value"
             (= #{"identity-publication-dead-lane"} (get raw-stored "retry_of_agent")))
      (check "identity marker binds retry_of_agent across writer and reader"
             (and (= "identity-publication-dead-lane" (get stored "retry_of_agent"))
                  (north.agent-provenance/managed-valid? stored)
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    ;; A normal first attempt (no retry_of_agent at all) remains valid: the
    ;; predicate stays optional, never required-identity-predicates.
    (let [first-attempt-subject "@agent:identity-publication-first-attempt"
          result (run-writer port "publish" first-attempt-subject
                             (json/generate-string preset))
          raw-stored (entity-facts port first-attempt-subject)
          stored (scalar-facts raw-stored)]
      (check "first-attempt identity without retry_of_agent publishes"
             (zero? (:exit result)))
      (check "first-attempt identity carries no retry_of_agent and stays valid"
             (and (nil? (get raw-stored "retry_of_agent"))
                  (north.agent-provenance/managed-valid? stored)
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    ;; Fail-closed is preserved: only the registered vocabulary is accepted.
    (let [bogus-subject "@agent:identity-publication-bogus-predicate"
          bogus (assoc preset "totally_unregistered_pred" "x")
          rejected (run-writer port "publish" bogus-subject
                               (json/generate-string bogus))]
      (check "unregistered identity predicate is still rejected before mutation"
             (and (not (zero? (:exit rejected)))
                  (str/includes? (:err rejected)
                                 "unsupported managed identity predicate")
                  (empty? (entity-facts port bogus-subject)))))

    (let [terminal {"outcome" "ran" "process_outcome" "ran"
                    "delivery_outcome" "unverified"
                    "delivery_reason" "provider_terminal_success_without_external_verification"}
          terminal-result (run-writer port "terminal" subject (json/generate-string terminal))
          stored (scalar-facts (entity-facts port subject))]
      (check "terminal process and delivery axes publish together"
             (and (zero? (:exit terminal-result))
                  (= "ran" (get stored "process_outcome"))
                  (= "unverified" (get stored "delivery_outcome"))
                  (= "ran"
                     (north.terminal-projection/terminal-process-outcome stored)))))

    (let [recovery-subject "@agent:identity-publication-crash-retry"]
      (doseq [[predicate value] preset]
        (north.coord/append! port recovery-subject predicate value))
      (let [before (entity-facts port recovery-subject)
            recovered
            (run-writer port "publish" recovery-subject
                        (json/generate-string preset))
            after (entity-facts port recovery-subject)
            stored (scalar-facts after)]
        (check "byte-identical markerless publication retries by committing its marker"
               (and (nil? (get before "identity_manifest_sha256"))
                    (zero? (:exit recovered))
                    (= (dissoc before "identity_manifest_sha256")
                       (dissoc after "identity_manifest_sha256"))
                    (= (get stored "identity_manifest_sha256")
                       (north.agent-provenance/manifest-sha256 stored))))))

    (let [mismatch-subject "@agent:identity-publication-mismatched-retry"
          mismatched (assoc preset "goal" "different crashed body")]
      (doseq [[predicate value] mismatched]
        (north.coord/append! port mismatch-subject predicate value))
      (let [before (entity-facts port mismatch-subject)
            rejected
            (run-writer port "publish" mismatch-subject
                        (json/generate-string preset))]
        (check "mismatched markerless body is rejected without mutation"
               (and (not (zero? (:exit rejected)))
                    (= before (entity-facts port mismatch-subject))
                    (nil? (get before "identity_manifest_sha256"))))))

    (let [before-version (north.coord/cur-ver port)
          second-result (run-writer port "publish" subject (json/generate-string bespoke))
          generation-ops (->> (log-ops port before-version)
                              (filter #(= subject (:subject %)))
                              vec)
          raw-stored (entity-facts port subject)
          stored (scalar-facts raw-stored)]
      (check "sequential reuse publishes the second shape" (zero? (:exit second-result)))
      (check "identity reuse withdraws identity and terminal markers before any body mutation"
             (= [[:retract "identity_manifest_sha256"]
                 [:retract "terminal_manifest_sha256"]]
                (mapv (juxt :operation :predicate)
                      (take 2 generation-ops))))
      (check "identity reuse withdraws the legacy outcome before process_outcome"
             (= [[:retract "outcome"] [:retract "process_outcome"]]
                (mapv (juxt :operation :predicate)
                      (take 2 (drop 2 generation-ops)))))
      (check "sequential reuse removes every stale optional preset field and outcome"
             (and (nil? (get raw-stored "composition_overrides"))
                  (nil? (get raw-stored "composition_override_reason"))
                  (nil? (get raw-stored "outcome"))
                  (nil? (get raw-stored "process_outcome"))
                  (nil? (get raw-stored "delivery_outcome"))
                  (nil? (get raw-stored "terminal_manifest_sha256"))
                  (= #{"analyst"} (get raw-stored "nearest_preset"))))
      (check "every managed identity predicate has exactly one live value"
             (every? #(= 1 (count %))
                     (vals (select-keys raw-stored north.agent-provenance/identity-predicates))))
      (check "bespoke generation is committed and canonical"
             (and (north.agent-provenance/managed-valid? stored)
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    (let [route {"provider" "anthropic" "provider_target" "claude-c"
                 "live_input" "streaming" "live_input_state" "armed"
                 "live_input_epoch" "00000000-0000-4000-8000-000000000103"
                 "model" "claude-opus-4-8" "effort" "high"
                 "display_handle" "anthropic-c-opus-high-orchestration-bespoke-probe"
                 "display_name" "anthropic:claude-c · opus · high · orchestration:bespoke:migration-forensics"}
          route-result (run-writer port "route" subject (json/generate-string route))
          raw-stored (entity-facts port subject)
          stored (scalar-facts raw-stored)]
      (check "fallback route update is acknowledged" (zero? (:exit route-result)))
      (check "route update retracts every previous multi-cardinality route value"
             (and (= #{"anthropic"} (get raw-stored "provider"))
                  (= #{"claude-c"} (get raw-stored "provider_target"))
                  (= #{"claude-opus-4-8"} (get raw-stored "model"))))
      (check "route update recommits the full current projection"
             (= (north.agent-provenance/manifest-sha256 stored)
                (get stored "identity_manifest_sha256"))))

    (let [goal {"goal" "new durable goal"
                  "display_name" "anthropic:claude-c · opus · high · orchestration:bespoke:migration-forensics · new durable goal"}
          goal-result (run-writer port "goal" subject (json/generate-string goal))
          raw-stored (entity-facts port subject)
          stored (scalar-facts raw-stored)]
      (check "typed goal is acknowledged" (zero? (:exit goal-result)))
      (check "typed goal leaves exactly one goal and one display cache"
             (and (= #{"new durable goal"} (get raw-stored "goal"))
                  (= #{(get goal "display_name")} (get raw-stored "display_name"))))
      (check "typed goal recommits a startup-valid identity"
             (and (north.agent-provenance/managed-valid? stored)
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    (let [before (entity-facts port subject)
          invalid (assoc bespoke "composition_contract_sha256" "not-a-hash")
          rejected (run-writer port "publish" subject (json/generate-string invalid))]
      (check "invalid identity is rejected before mutating the committed generation"
             (and (not (zero? (:exit rejected)))
                  (= before (entity-facts port subject)))))

    (let [before (entity-facts port subject)
          ambiguous (assoc bespoke "goal" " boundary whitespace ")
          rejected (run-writer port "publish"
                               subject (json/generate-string ambiguous))]
      (check "writer rejects reader-normalized boundary whitespace before mutation"
             (and (not (zero? (:exit rejected)))
                  (= before (entity-facts port subject)))))

    (let [policy-subject "@agent:identity-invalid-lease-policy"
          rejected
          (run-writer
           port "publish" policy-subject (json/generate-string preset)
           {"NORTH_IDENTITY_WRITER_TIMEOUT_MS" "10000"
            "NORTH_IDENTITY_WRITE_LEASE_TTL_MS" "10000"})]
      (check "write lease must outlive the process timeout before mutation"
             (and (not (zero? (:exit rejected)))
                  (empty? (entity-facts port policy-subject)))))

    ;; The accept side of the same policy, at TERMINAL-PATH scale. The SDK
    ;; derives the lease from the timeout it declares (identity.ts
    ;; internalWriteLeaseTtlMs = timeout + WRITE_LEASE_SAFETY_MARGIN_MS), and the
    ;; authoritative terminal marker declares TerminalPublicationBudget
    ;; .publicationTimeout(1) — ~72s on the default budget, up to ~240s at the
    ;; NORTH_TERMINAL_PUBLICATION_BUDGET_MS ceiling. A fixed 60s lease rejected
    ;; every one of those, which is what made lane terminals indeterminate
    ;; (thread 019f9c3b). Pin both ends of the derived range as ACCEPTED.
    (doseq [[label writer-timeout lease-ttl]
            [["default budget" "72000" "87000"]
             ["raised budget ceiling" "240000" "255000"]]]
      (let [policy-subject (str "@agent:identity-derived-lease-"
                                (str/replace label #"\s+" "-"))
            accepted
            (run-writer
             port "publish" policy-subject (json/generate-string preset)
             {"NORTH_IDENTITY_WRITER_TIMEOUT_MS" writer-timeout
              "NORTH_IDENTITY_WRITE_LEASE_TTL_MS" lease-ttl})]
        (check (str "SDK-derived lease is accepted at " label
                    " writer timeout " writer-timeout "ms")
               (and (zero? (:exit accepted))
                    (north.agent-provenance/managed-valid?
                     (scalar-facts (entity-facts port policy-subject)))))))

    (let [held-subject "@agent:identity-held-publish"
          winner (assoc preset
                        "goal" "winner remains authoritative"
                        "display_name" "winner remains authoritative")
          loser (assoc preset
                       "goal" "stale loser must not publish"
                       "display_name" "stale loser must not publish")
          seeded (run-writer port "publish" held-subject
                             (json/generate-string winner))
          before (entity-facts port held-subject)
          resource (identity-write-resource held-subject)
          holder "identity-publication-test-holder"
          lease (north.coord/acquire-lease! port resource holder 60000)
          rejected (run-writer port "publish" held-subject
                               (json/generate-string loser))
          after (entity-facts port held-subject)
          _ (release-lease! port lease)]
      (check "winner identity seeds before the rival publication probe"
             (and (zero? (:exit seeded)) (:ok lease)))
      (check "same-subject rival is rejected before mutation"
             (and (not (zero? (:exit rejected))) (= before after)))
      (check "rejected rival preserves the winner marker and exact body"
             (let [stored (scalar-facts after)]
               (and (north.agent-provenance/managed-valid? stored)
                    (= (get stored "identity_manifest_sha256")
                       (north.agent-provenance/manifest-sha256 stored))))))

    (doseq [{:keys [operation payload verify]}
            [{:operation "route"
              :payload {"provider" "openai" "provider_target" "codex-held"
                        "live_input" "turn-framed" "live_input_state" "frozen"
                        "live_input_epoch" "00000000-0000-4000-8000-000000000104"
                        "model" "gpt-5.6-sol" "effort" "high"
                        "display_handle" "openai-held-sol-high-integrator"
                        "display_name" "openai:codex-held · sol · high · orchestration:integrator"}
              :verify #(= #{"gpt-5.6-sol"} (get % "model"))}
             {:operation "goal"
              :payload {"goal" "held goal committed"
                        "display_name" "held goal committed"}
              :verify #(= #{"held goal committed"} (get % "goal"))}
             {:operation "terminal"
              :payload {"outcome" "ran" "process_outcome" "ran"
                        "delivery_outcome" "unverified"
                        "delivery_reason"
                        "provider_terminal_success_without_external_verification"}
              :verify #(some? (get % "terminal_manifest_sha256"))}]]
      (let [held-subject (str "@agent:identity-held-" operation)
            seeded (run-writer
                    port "publish" held-subject
                    (json/generate-string
                     (assoc preset
                            "goal" (str "held " operation)
                            "display_handle" (str "held-" operation)
                            "display_name" (str "held " operation))))
            resource (identity-write-resource held-subject)
            holder (str "held-lifecycle-" operation)
            acquired
            (north.coord/acquire-lease! port resource holder 60000)
            lease {:resource resource :holder holder :epoch (:epoch acquired)}
            pending
            (future
              (run-writer port operation held-subject
                          (json/generate-string payload)))
            _ (Thread/sleep 150)
            waited? (not (realized? pending))
            still-held?
            (:valid? (north.coord/check-lease! port lease))
            released (release-lease! port lease)
            result (deref pending 8000 {:exit -99 :err "writer did not return"})
            after (entity-facts port held-subject)
            stored (scalar-facts after)]
        (check (str operation " waits while the subject lease is held")
               (and (zero? (:exit seeded)) (:ok acquired)
                    waited? still-held? (:ok released) (:released? released)))
        (check (str operation " succeeds after the prior writer releases")
               (and (zero? (:exit result))
                    (verify after)
                    (north.agent-provenance/managed-valid? stored)
                    (= (get stored "identity_manifest_sha256")
                       (north.agent-provenance/manifest-sha256 stored))))
        (when (= "terminal" operation)
          (check "held terminal release commits an exact terminal projection"
                 (= (get stored "terminal_manifest_sha256")
                    (north.terminal-projection/terminal-manifest-sha256 stored))))))

    (let [held-subject "@agent:identity-held-past-budget"
          seeded (run-writer port "publish" held-subject
                             (json/generate-string preset))
          before (entity-facts port held-subject)
          resource (identity-write-resource held-subject)
          holder "held-past-acquisition-budget"
          acquired
          (north.coord/acquire-lease! port resource holder 60000)
          started (System/nanoTime)
          rejected
          (run-writer
           port "goal" held-subject
           (json/generate-string
            {"goal" "must not land" "display_name" "must not land"})
           {"NORTH_IDENTITY_WRITER_TIMEOUT_MS" "1200"
            "NORTH_IDENTITY_WRITE_LEASE_TTL_MS" "5000"})
          elapsed-ms (/ (- (System/nanoTime) started) 1000000.0)
          _ (release-lease!
             port {:resource resource :holder holder :epoch (:epoch acquired)})]
      (check "held lifecycle lease exhausts its in-process budget without parent kill"
             (and (zero? (:exit seeded))
                  (:ok acquired)
                  (not (zero? (:exit rejected)))
                  (<= 500 elapsed-ms 3000)))
      (check "acquisition timeout occurs before any managed mutation"
             (= before (entity-facts port held-subject))))

    (let [stop-churn? (atom false)
          churn
          (future
            (loop [index 0]
              (when-not @stop-churn?
                (north.coord/append!
                 port "@identity-publication-global-churn" "sample" (str index))
                (recur (inc index)))))
          gate (java.util.concurrent.CountDownLatch. 1)
          publications
          (mapv
           (fn [index]
             (let [parallel-subject
                   (str "@agent:identity-parallel-" index)
                   identity
                   (assoc preset
                          "goal" (str "parallel identity " index)
                          "display_handle" (str "parallel-identity-" index)
                          "display_name" (str "parallel identity " index))]
               {:subject parallel-subject
                :result
                (future
                  (.await gate)
                  (run-writer port "publish" parallel-subject
                              (json/generate-string identity)))}))
           (range 8))
          _ (.countDown gate)
          results
          (mapv (fn [{:keys [subject result]}]
                  {:subject subject :result @result})
                publications)
          _ (reset! stop-churn? true)
          _ @churn]
      (check "eight distinct identities publish during unrelated global churn"
             (every? #(zero? (get-in % [:result :exit])) results))
      (check "every parallel publication has an exact committed identity"
             (every?
              (fn [{:keys [subject]}]
                (let [raw (entity-facts port subject)
                      stored (scalar-facts raw)]
                  (and (every? #(= 1 (count %))
                               (vals
                                (select-keys
                                 raw north.agent-provenance/identity-predicates)))
                       (north.agent-provenance/managed-valid? stored)
                       (= (get stored "identity_manifest_sha256")
                          (north.agent-provenance/manifest-sha256 stored)))))
              results)))

    (let [race-subject "@agent:identity-publish-race"
          attempts
          (mapv
           (fn [index]
             (future
               (run-writer
                port "publish" race-subject
                (json/generate-string
                 (assoc preset
                        "goal" (str "racing generation " index)
                        "display_name" (str "racing generation " index))))))
           (range 8))
          results (mapv deref attempts)
          raw-stored (entity-facts port race-subject)
          stored (scalar-facts raw-stored)
          markers (get raw-stored "identity_manifest_sha256" #{})]
      (check "at least one same-subject publication wins"
             (some #(zero? (:exit %)) results))
      (check "concurrent identity publication ends at one exact committed body"
             (and (= 1 (count markers))
                  (north.agent-provenance/managed-valid? stored)
                  (= (first markers)
                     (north.agent-provenance/manifest-sha256 stored)))))

    (let [race-subject "@agent:identity-route-goal-race"
          seeded
          (run-writer port "publish" race-subject
                      (json/generate-string preset))
          route
          {"provider" "openai" "provider_target" "codex-race"
           "live_input" "unsupported" "live_input_state" "frozen"
           "live_input_epoch" "00000000-0000-4000-8000-000000000105"
           "model" "gpt-5.6-sol" "effort" "high"
           "display_handle" "openai-race-sol-high-integrator"
           "display_name" "openai:codex-race · sol · high · orchestration:integrator"}
          operations
          (mapv
           (fn [index]
             (future
               (if (even? index)
                 (run-writer port "route" race-subject
                             (json/generate-string route))
                 (run-writer
                  port "goal" race-subject
                  (json/generate-string
                   {"goal" (str "racing goal " index)
                    "display_name" (str "racing goal " index)})))))
           (range 16))
          results (mapv deref operations)
          raw-stored (entity-facts port race-subject)
          stored (scalar-facts raw-stored)
          markers (get raw-stored "identity_manifest_sha256" #{})]
      (check "at least one concurrent route or goal wins"
             (some #(zero? (:exit %)) results))
      (check "route/goal share the subject-local identity marker seam"
             (and (zero? (:exit seeded))
                  (= 1 (count markers))
                  (north.agent-provenance/managed-valid? stored)
                  (= (first markers)
                     (north.agent-provenance/manifest-sha256 stored)))))

    ;; Caller-owned recovery protocol. Route transitions carry both complete
    ;; endpoints, so every killed durable prefix can be classified and rebuilt.
    (let [route-delta
          {"provider" "openai"
           "provider_target" "codex-recovery"
           "live_input" "streaming"
           "live_input_state" "armed"
           "live_input_epoch" "00000000-0000-4000-8000-000000000121"
           "model" "gpt-5.6-sol"
           "effort" "xhigh"
           "display_handle" "openai-recovery-sol-xhigh-integrator"
           "display_name" "openai:codex-recovery · sol · xhigh · orchestration:integrator"}
          desired (merge preset route-delta)
          old-marker (north.agent-provenance/manifest-sha256 preset)
          new-marker (north.agent-provenance/manifest-sha256 desired)
          transition
          (vec
           (concat
            [[:retract "identity_manifest_sha256" old-marker]]
            (for [predicate (sort (conj test-route-generation-predicates
                                       "display_name"))]
              [:retract predicate (get preset predicate)])
            (for [predicate (sort (conj test-route-generation-predicates
                                       "display_name"))]
              [:put predicate (get desired predicate)])
            [[:put "identity_manifest_sha256" new-marker]]))
          holder "managed-agent-writer:00000000-0000-4000-8000-000000000120"
          results
          (mapv
           (fn [prefix]
             (let [subject (str "@agent:managed-killed-prefix-" prefix)]
               (seed-identity! port subject preset)
               (apply-prefix! port subject transition prefix)
               (let [result
                     (run-managed-writer
                      port "route" subject (json/generate-string route-delta)
                      holder (str (java.util.UUID/randomUUID)) desired preset)
                     stored (scalar-facts (entity-facts port subject))]
                 {:result result
                  :exact (and (north.agent-provenance/managed-valid? stored)
                              (= desired
                                 (select-keys stored (keys desired)))
                              (= new-marker
                                 (get stored "identity_manifest_sha256")))})))
           (range (inc (count transition))))]
      (check "every durable route prefix recovers the exact desired generation"
             (every?
              #(and (zero? (get-in % [:result :exit]))
                    (= "committed" (get-in % [:result :result :status]))
                    (:exact %))
              results)))

    ;; Exact state after commit with the old epoch still leased is the real
    ;; commit-unknown incident. Same-holder replay must rotate/fence immediately;
    ;; the delayed old finally may not erase the successor epoch.
    (let [subject "@agent:managed-lost-ack"
          route-delta
          {"provider" "openai" "provider_target" "codex-lost-ack"
           "live_input" "streaming" "live_input_state" "armed"
           "live_input_epoch" "00000000-0000-4000-8000-000000000122"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-lost-ack-sol-xhigh-integrator"
           "display_name" "openai:codex-lost-ack · sol · xhigh · orchestration:integrator"}
          desired (merge preset route-delta)
          holder "managed-agent-writer:00000000-0000-4000-8000-000000000122"
          resource (identity-write-resource subject)
          old-lease (north.coord/acquire-lease! port resource holder 60000)
          _ (seed-identity! port subject desired)
          recovered (run-managed-writer
                     port "route" subject (json/generate-string route-delta)
                     holder (str (java.util.UUID/randomUUID)) desired preset)
          stale-write (north.coord/put-with-fence!
                       port {:resource resource :holder holder
                             :epoch (:epoch old-lease)}
                       subject "goal" "stale prior operation")
          stale-release (release-lease! port old-lease)
          stored (scalar-facts (entity-facts port subject))]
      (check "lost acknowledgement replays as committed through the retained same-holder fence"
             (and (:ok old-lease)
                  (zero? (:exit recovered))
                  (= "committed" (get-in recovered [:result :status]))
                  (= "exact_replay" (get-in recovered [:result :reason]))
                  (= desired (select-keys stored (keys desired)))))
      (check "stale same-holder release cannot erase the recovered epoch"
             (and (= :fence-lost (:reject stale-write))
                  (false? (:released? stale-release))
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    (let [subject "@agent:managed-conflicting-successor"
          route-delta
          {"provider" "openai" "provider_target" "codex-intended"
           "live_input" "streaming" "live_input_state" "armed"
           "live_input_epoch" "00000000-0000-4000-8000-000000000123"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-intended-sol-xhigh-integrator"
           "display_name" "openai:codex-intended · sol · xhigh · orchestration:integrator"}
          desired (merge preset route-delta)
          successor (assoc desired
                           "provider_target" "codex-successor"
                           "live_input_epoch" "00000000-0000-4000-8000-000000000124"
                           "display_handle" "openai-successor-sol-xhigh-integrator"
                           "display_name" "openai:codex-successor · sol · xhigh · orchestration:integrator")
          _ (seed-identity! port subject successor)
          before (entity-facts port subject)
          result (run-managed-writer
                  port "route" subject (json/generate-string route-delta)
                  "managed-agent-writer:00000000-0000-4000-8000-000000000123"
                  (str (java.util.UUID/randomUUID)) desired preset)]
      (check "recovery returns typed not_committed without overwriting a successor"
             (and (zero? (:exit result))
                  (= "not_committed" (get-in result [:result :status]))
                  (= "conflicting_generation" (get-in result [:result :reason]))
                  (= before (entity-facts port subject)))))

    (let [subject "@agent:managed-goal-before-route"
          goal-updated (assoc preset
                          "goal" "new goal-updated goal"
                          "display_name" "goal-updated display cache")
          route-delta
          {"provider" "openai" "provider_target" "codex-goal-updated"
           "live_input" "streaming" "live_input_state" "armed"
           "live_input_epoch" "00000000-0000-4000-8000-000000000131"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-goal-updated-sol-xhigh-integrator"
           "display_name" "stale pre-goal route display"}
          stale-desired (merge preset route-delta)
          effective (merge goal-updated
                           (select-keys route-delta
                                        test-route-generation-predicates))
          _seed (seed-identity! port subject preset)
          goal-result
          (run-writer port "goal" subject
                      (json/generate-string
                       (select-keys goal-updated ["goal" "display_name"])))
          route-result
          (run-managed-writer
           port "route" subject (json/generate-string route-delta)
           "managed-agent-writer:00000000-0000-4000-8000-000000000131"
           (str (java.util.UUID/randomUUID)) stale-desired preset)
          stored (scalar-facts (entity-facts port subject))]
      (check "goal before route rebases route axes without restoring stale goal/cache"
             (and (zero? (:exit goal-result))
                  (= "committed" (get-in route-result [:result :status]))
                  (= "rebased_goal_overlay" (get-in route-result [:result :reason]))
                  (= effective (select-keys stored (keys effective)))
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    (let [subject "@agent:managed-goal-before-freeze"
          goal-updated (assoc preset
                          "goal" "goal-updated before freeze"
                          "display_name" "goal-updated freeze display")
          freeze-delta
          (assoc (select-keys preset
                             ["provider" "provider_target" "live_input"
                              "model" "effort" "display_handle" "display_name"])
                 "live_input_state" "frozen"
                 "live_input_epoch" "00000000-0000-4000-8000-000000000132"
                 "display_name" "stale freeze display")
          stale-desired (merge preset freeze-delta)
          _seed (seed-identity! port subject preset)
          _goal (run-writer port "goal" subject
                              (json/generate-string
                               (select-keys goal-updated ["goal" "display_name"])))
          freeze-result
          (run-managed-writer
           port "route" subject (json/generate-string freeze-delta)
           "managed-agent-writer:00000000-0000-4000-8000-000000000132"
           (str (java.util.UUID/randomUUID)) stale-desired preset)
          stored (scalar-facts (entity-facts port subject))]
      (check "mandatory freeze after goal commits frozen while preserving the goal overlay"
             (and (= "committed" (get-in freeze-result [:result :status]))
                  (= "frozen" (get stored "live_input_state"))
                  (= (get goal-updated "goal") (get stored "goal"))
                  (= (get goal-updated "display_name") (get stored "display_name"))
                  (north.agent-provenance/managed-valid? stored))))

    (let [subject "@agent:managed-goal-before-terminal"
          terminal
          {"outcome" "died" "process_outcome" "died"
           "delivery_outcome" "blocked"
           "delivery_reason" "provider_process_died"}
          _seed (seed-identity! port subject preset)
          _goal (run-writer port "goal" subject
                              (json/generate-string
                               {"goal" "goal-updated before terminal"
                                "display_name" "goal-updated terminal display"}))
          terminal-result
          (run-managed-writer
           port "terminal" subject (json/generate-string terminal)
           "managed-agent-writer:00000000-0000-4000-8000-000000000133"
           (str (java.util.UUID/randomUUID)) nil preset)
          stored (scalar-facts (entity-facts port subject))]
      (check "terminal accepts a valid goal successor without weakening route generation checks"
             (and (= "committed" (get-in terminal-result [:result :status]))
                  (= "goal-updated before terminal" (get stored "goal"))
                  (= (north.terminal-projection/terminal-manifest-sha256 terminal)
                     (get stored "terminal_manifest_sha256")))))


    (let [subject "@agent:managed-goal-during-recovery"
          holder "managed-agent-writer:00000000-0000-4000-8000-000000000134"
          route-delta
          {"provider" "openai" "provider_target" "codex-goal-race"
           "live_input" "streaming" "live_input_state" "armed"
           "live_input_epoch" "00000000-0000-4000-8000-000000000134"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-goal-race-sol-xhigh-integrator"
           "display_name" "pre-goal race display"}
          desired (merge preset route-delta)
          terminal
          {"outcome" "died" "process_outcome" "died"
           "delivery_outcome" "blocked"
           "delivery_reason" "provider_process_died"}
          resource (identity-write-resource subject)
          _seed (seed-identity! port subject desired)
          old-lease (north.coord/acquire-lease! port resource holder 60000)
          waiting-goal
          (future
            (run-writer port "goal" subject
                        (json/generate-string
                         {"goal" "goal-updated during recovery"
                          "display_name" "goal-updated recovery display"})))
          _wait (Thread/sleep 100)
          recovery-result
          (run-managed-writer
           port "route" subject (json/generate-string route-delta)
           holder (str (java.util.UUID/randomUUID)) desired preset)
          goal-result (deref waiting-goal 10000 {:exit -1})
          terminal-result
          (run-managed-writer
           port "terminal" subject (json/generate-string terminal)
           holder (str (java.util.UUID/randomUUID)) nil desired)
          stale-release (release-lease! port old-lease)
          stored (scalar-facts (entity-facts port subject))]
      (check "same-holder lost-ack recovery fences first, then a waiting goal and terminal both commit"
             (and (:ok old-lease)
                  (= "committed" (get-in recovery-result [:result :status]))
                  (= "exact_replay" (get-in recovery-result [:result :reason]))
                  (zero? (:exit goal-result))
                  (= "committed" (get-in terminal-result [:result :status]))
                  (false? (:released? stale-release))
                  (= "goal-updated during recovery" (get stored "goal"))
                  (= (north.terminal-projection/terminal-manifest-sha256 terminal)
                     (get stored "terminal_manifest_sha256")))))

    (let [goal-updated
          (assoc preset
                 "goal" "goal survives every route prefix"
                 "display_name" "durable goal overlay")
          route-delta
          {"provider" "openai" "provider_target" "codex-goal-prefix"
           "live_input" "streaming" "live_input_state" "frozen"
           "live_input_epoch" "00000000-0000-4000-8000-000000000135"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-goal-prefix-sol-xhigh-integrator"
           "display_name" "stale caller cache"}
          stale-desired (merge preset route-delta)
          effective (merge goal-updated
                           (select-keys route-delta
                                        test-route-generation-predicates))
          old-marker (north.agent-provenance/manifest-sha256 goal-updated)
          new-marker (north.agent-provenance/manifest-sha256 effective)
          transition
          (vec
           (concat
            [[:retract "identity_manifest_sha256" old-marker]]
            (for [predicate (sort test-route-generation-predicates)]
              [:retract predicate (get goal-updated predicate)])
            (for [predicate (sort test-route-generation-predicates)]
              [:put predicate (get effective predicate)])
            [[:put "identity_manifest_sha256" new-marker]]))
          holder "managed-agent-writer:00000000-0000-4000-8000-000000000135"
          results
          (mapv
           (fn [prefix]
             (let [subject (str "@agent:managed-goal-prefix-" prefix)]
               (seed-identity! port subject goal-updated)
               (apply-prefix! port subject transition prefix)
               (let [result
                     (run-managed-writer
                      port "route" subject (json/generate-string route-delta)
                      holder (str (java.util.UUID/randomUUID))
                      stale-desired preset)
                     stored (scalar-facts (entity-facts port subject))]
                 {:result result
                  :exact (and (= effective
                                 (select-keys stored (keys effective)))
                              (= new-marker
                                 (get stored "identity_manifest_sha256")))})))
           (range (inc (count transition))))]
      (check "every killed route prefix preserves and recovers a committed goal overlay"
             (every?
              #(and (zero? (get-in % [:result :exit]))
                    (= "committed" (get-in % [:result :result :status]))
                    (:exact %))
              results)))

    (let [terminal
          {"outcome" "died" "process_outcome" "died"
           "delivery_outcome" "blocked"
           "delivery_reason" "provider_process_died"}
          operation-order
          (vec
           (concat
            (for [predicate test-terminal-publication-order
                  :let [value (get terminal predicate)]
                  :when value]
              [:put predicate value])
            [[:put "terminal_manifest_sha256"
              (north.terminal-projection/terminal-manifest-sha256 terminal)]]))
          results
          (mapv
           (fn [prefix]
             (let [subject (str "@agent:managed-terminal-prefix-" prefix)
                   holder (str "managed-agent-writer:"
                               (java.util.UUID/randomUUID))]
               (seed-identity! port subject preset)
               (apply-prefix! port subject operation-order prefix)
               (let [result
                     (run-managed-writer
                      port "terminal" subject (json/generate-string terminal)
                      holder (str (java.util.UUID/randomUUID)) nil preset)
                     stored (scalar-facts (entity-facts port subject))]
                 {:result result
                  :exact (= (north.terminal-projection/terminal-manifest-sha256 terminal)
                            (get stored "terminal_manifest_sha256"))})))
           (range (inc (count operation-order))))]
      (check "every durable terminal prefix recovers one exact terminal projection"
             (every?
              #(and (zero? (get-in % [:result :exit]))
                    (= "committed" (get-in % [:result :result :status]))
                    (:exact %))
              results)))

    (let [subject "@agent:managed-terminal-dominates-route"
          holder "managed-agent-writer:00000000-0000-4000-8000-000000000125"
          terminal
          {"outcome" "died" "process_outcome" "died"
           "delivery_outcome" "blocked"
           "delivery_reason" "provider_process_died"}
          route-delta
          {"provider" "openai" "provider_target" "codex-after-terminal"
           "live_input" "streaming" "live_input_state" "frozen"
           "live_input_epoch" "00000000-0000-4000-8000-000000000125"
           "model" "gpt-5.6-sol" "effort" "xhigh"
           "display_handle" "openai-after-terminal-sol-xhigh-integrator"
           "display_name" "openai:codex-after-terminal · sol · xhigh · orchestration:integrator"}
          desired (merge preset route-delta)
          _ (seed-identity! port subject preset)
          terminal-result
          (run-managed-writer
           port "terminal" subject (json/generate-string terminal)
           holder (str (java.util.UUID/randomUUID)) nil preset)
          before (entity-facts port subject)
          route-result
          (run-managed-writer
           port "route" subject (json/generate-string route-delta)
           holder (str (java.util.UUID/randomUUID)) desired preset)]
      (check "committed terminal is irreversible and rejects later route mutation"
             (and (= "committed" (get-in terminal-result [:result :status]))
                  (= "not_committed" (get-in route-result [:result :status]))
                  (= "terminal_committed" (get-in route-result [:result :reason]))
                  (= before (entity-facts port subject)))))

    (let [worker-subject "@agent:delivery-worker"
          verifier-subject "@agent:delivery-verifier"
          worker (assoc preset
                        "role" "integrator" "composition_id" "integrator"
                        "goal" "deliver a proof-carrying change"
                        "display_handle" "anthropic-a-opus-high-integrator-worker"
                        "display_name" "anthropic:claude-a · opus · high · orchestration:integrator")
          verifier (assoc preset
                          "role" "verifier" "composition_id" "verifier"
                          "goal" "independently attest delivery"
                          "display_handle" "anthropic-a-opus-high-verifier-proof"
                          "display_name" "anthropic:claude-a · opus · high · orchestration:verifier")
          run-evidence (array-map
                        "bar" "tests pass"
                        "observed" "24/24"
                        "recordedAt" "2026-07-18T09:59:59Z"
                        "reporter" worker-subject
                        "run" "@run-delivery-worker-proof"
                        "thread" "@thread-proof"
                        "version" "north:run-bar-evidence:v1")
          evidence (json/generate-string
                    (array-map
                     "version" "north:done-bars:v2"
                     "run" "@run-delivery-worker-proof"
                     "thread" "@thread-proof"
                     "reporter" worker-subject
                     "contractOrigin" "accepted"
                     "baselineDoneWhen" ["tests pass"]
                     "doneWhen" ["tests pass"]
                     "matches" [{"bar" "tests pass"
                                 "evidence" [run-evidence]}]))
          reported {"outcome" "ran" "process_outcome" "ran"
                    "delivery_outcome" "reported"
                    "delivery_reason" "complete_run_scoped_done_bar_evidence_self_reported"
                    "delivery_evidence" evidence
                    "delivery_evidence_sha256"
                    (north.terminal-projection/sha256 evidence)}]
      (check "delivery worker identity publishes"
             (zero? (:exit (run-writer port "publish" worker-subject
                                       (json/generate-string worker)))))
      (check "independent verifier identity publishes"
             (zero? (:exit (run-writer port "publish" verifier-subject
                                       (json/generate-string verifier)))))
      (north.coord/append! port "@thread-proof" "done_when" "tests pass")
      (let [missing-run-result
            (run-writer port "terminal" worker-subject
                        (json/generate-string reported))]
        (check "reported terminal rejects a missing reserved run"
               (and (not (zero? (:exit missing-run-result)))
                    (nil? (get (entity-facts port worker-subject)
                               "terminal_manifest_sha256")))))
      (reserve-run! port "@run-delivery-worker-proof" worker-subject
                    "@thread-proof" (apply str (repeat 64 "a")))
      (north.coord/append!
       port "@run-delivery-worker-proof" "run_bar_evidence"
       (json/generate-string (into (sorted-map) run-evidence)))
      (check "complete self-reported proof commits as reported"
             (zero? (:exit (run-writer port "terminal" worker-subject
                                       (json/generate-string reported)))))
      (doseq [[label injected]
              [["uncited valid"
                (json/generate-string
                 (into (sorted-map)
                       (assoc run-evidence
                              "bar" "uncited extra bar"
                              "observed" "not in snapshot"
                              "recordedAt" "2026-07-18T10:00:01Z")))]
               ["malformed" "{"]
               ["duplicate bar"
                (json/generate-string
                 (into (sorted-map)
                       (assoc run-evidence
                              "observed" "second stored observation"
                              "recordedAt" "2026-07-18T10:00:02Z")))]]]
        (north.coord/append! port "@run-delivery-worker-proof"
                             "run_bar_evidence" injected)
        (let [before (entity-facts port worker-subject)
              rejected
              (run-writer port "terminal" worker-subject
                          (json/generate-string reported))]
          (check (str "lane marker rejects " label " stored evidence")
                 (and (not (zero? (:exit rejected)))
                      (= before (entity-facts port worker-subject)))))
        (north.coord/retract! port "@run-delivery-worker-proof"
                              "run_bar_evidence" injected))
      (let [relabelled-evidence
            (json/generate-string
             (-> (json/parse-string evidence)
                 (assoc "contractOrigin" "worker-defined")
                 (assoc "baselineDoneWhen" [])))
            relabelled
            (assoc reported
                   "delivery_evidence" relabelled-evidence
                   "delivery_evidence_sha256"
                   (north.terminal-projection/sha256 relabelled-evidence))
            before (entity-facts port worker-subject)]
        (check "snapshot cannot relabel an accepted reservation as worker-defined"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string relabelled)))))
                    (= before (entity-facts port worker-subject)))))
      (north.coord/append! port "@thread-proof" "done_when" "late weaker bar")
      (let [before (entity-facts port worker-subject)
            changed
            (run-writer port "terminal" worker-subject
                        (json/generate-string reported))]
        (check "reported terminal rejects a changed current done-bar set"
               (and (not (zero? (:exit changed)))
                    (= before (entity-facts port worker-subject)))))
      (north.coord/retract! port "@thread-proof" "done_when" "late weaker bar")
      (let [fabricated-record (assoc run-evidence "observed" "not stored")
            fabricated-evidence
            (json/generate-string
             (assoc-in (json/parse-string evidence)
                       ["matches" 0 "evidence"] [fabricated-record]))
            fabricated
            (assoc reported
                   "delivery_evidence" fabricated-evidence
                   "delivery_evidence_sha256"
                   (north.terminal-projection/sha256 fabricated-evidence))
            before (entity-facts port worker-subject)]
        (check "reported terminal rejects a fabricated unstored run record"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string fabricated)))))
                    (= before (entity-facts port worker-subject)))))
      (let [cross-run "@run-delivery-cross-proof"
            cross-record (assoc run-evidence "run" cross-run)
            cross-evidence
            (json/generate-string
             (-> (json/parse-string evidence)
                 (assoc "run" cross-run)
                 (assoc-in ["matches" 0 "evidence"] [cross-record])))
            cross-reported
            (assoc reported
                   "delivery_evidence" cross-evidence
                   "delivery_evidence_sha256"
                   (north.terminal-projection/sha256 cross-evidence))
            before (entity-facts port worker-subject)]
        (reserve-run! port cross-run verifier-subject "@thread-proof"
                      (apply str (repeat 64 "b")))
        (north.coord/append! port cross-run "run_bar_evidence"
                            (json/generate-string (into (sorted-map) cross-record)))
        (check "reported terminal rejects a cross-agent run reservation"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string cross-reported)))))
                    (= before (entity-facts port worker-subject)))))
      (let [cross-run "@run-delivery-cross-thread-proof"
            cross-record (assoc run-evidence "run" cross-run)
            cross-evidence
            (json/generate-string
             (-> (json/parse-string evidence)
                 (assoc "run" cross-run)
                 (assoc-in ["matches" 0 "evidence"] [cross-record])))
            cross-reported
            (assoc reported
                   "delivery_evidence" cross-evidence
                   "delivery_evidence_sha256"
                   (north.terminal-projection/sha256 cross-evidence))
            before (entity-facts port worker-subject)]
        (reserve-run! port cross-run worker-subject "@different-thread"
                      (apply str (repeat 64 "c")))
        (north.coord/append! port cross-run "run_bar_evidence"
                            (json/generate-string (into (sorted-map) cross-record)))
        (check "reported terminal rejects a cross-thread run reservation"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string cross-reported)))))
                    (= before (entity-facts port worker-subject)))))
      (let [contradictory (assoc reported
                                 "outcome" "died"
                                 "process_outcome" "died")
            before (entity-facts port worker-subject)]
        (check "non-ran process cannot carry reported delivery proof"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string contradictory)))))
                    (= before (entity-facts port worker-subject)))))
      (let [forged-evidence (str/replace evidence worker-subject verifier-subject)
            forged (assoc reported
                          "delivery_evidence" forged-evidence
                          "delivery_evidence_sha256"
                          (north.terminal-projection/sha256 forged-evidence))
            before (entity-facts port worker-subject)]
        (check "caller-supplied reporter cannot forge managed terminal authority"
               (and (not (zero? (:exit
                                 (run-writer port "terminal" worker-subject
                                             (json/generate-string forged)))))
                    (= before (entity-facts port worker-subject)))))
      (let [self-result (run-writer port "attest" worker-subject
                                    (json/generate-string {"actor" worker-subject}))]
        (check "delivery worker cannot self-attest" (not (zero? (:exit self-result)))))
      (let [attested-result
            (proc/shell {:out :string :err :string :continue true
                         :extra-env {"AGENT_ID" "delivery-verifier"
                                     "NORTH_PORT" (str port)
                                     "FRAM_LOG" @test-log}}
                        (str root "/bin/north") "delivery" "attest"
                        "delivery-worker")
            stored (scalar-facts (entity-facts port worker-subject))]
        (check "public north delivery attest fails closed under shared-UID lanes"
               (and (not (zero? (:exit attested-result)))
                    (= "reported"
                       (north.terminal-projection/terminal-delivery-outcome stored))
                    (nil? (get stored "delivery_attestation"))))
        (check "failed attestation leaves the reported terminal manifest intact"
               (= (get stored "terminal_manifest_sha256")
                  (north.terminal-projection/terminal-manifest-sha256 stored)))))
    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks]
        (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed" (- (count @checks) (count failed)) (count @checks)))
        (when (seq failed) (System/exit 1))))))
