#!/usr/bin/env bb
;; Exact managed-identity publication against a throwaway Fram coordinator.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram (str (System/getProperty "user.home") "/code/fram"))
(def writer (str root "/cli/agent-fact-internal.clj"))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/agent-provenance.clj"))
(load-file (str root "/cli/terminal-projection.clj"))

(def checks (atom []))
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
(defn run-writer [port operation subject value]
  (let [result (proc/shell {:out :string :err :string :continue true}
                           "bb" writer (str port) operation subject value)]
    {:exit (:exit result) :out (:out result) :err (:err result)}))
(defn entity-facts [port subject]
  (let [rows (:ok (north.coord/send-op
                   port {:op :query
                         :query {:find "identity_test"
                                 :rules [{:head {:rel "identity_test"
                                                 :args [{:var "p"} {:var "r"}]}
                                          :body [{:rel "triple"
                                                  :args [subject {:var "p"} {:var "r"}]}]}]}}))]
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
(defn log-ops [file]
  (with-open [reader (io/reader file)]
    (mapv edn/read-string (line-seq reader))))

(let [port (free-port)
      tmp (.toFile (java.nio.file.Files/createTempDirectory
                    "north-identity-publication" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "facts.log")
      daemon (do
               (spit log "")
               (proc/process {:dir fram :out :string :err :string}
                             "bb" "-cp" "out" "coord_daemon.clj"
                             "serve-flat" (str port) (.getPath log)))
      subject "@agent:identity-publication-probe"
      preset {"kind" "lane" "role" "integrator" "model" "claude-opus-4-8"
              "provider" "anthropic" "provider_target" "claude-a" "effort" "high"
              "composition_kind" "preset" "composition_id" "integrator"
              "composition_overrides" "[\"tier\"]"
              "composition_override_reason" "critical seam" "repo" "north"
              "goal" "prove atomic publication" "spawned_at" "2026-07-17T01:00:00Z"
              "display_handle" "anthropic-a-opus-high-gaffer-integrator-probe"
              "display_name" "anthropic:claude-a · opus · high · gaffer:integrator"}
      bespoke {"kind" "lane" "role" "migration-forensics" "model" "gpt-5.6-sol"
               "provider" "openai" "provider_target" "codex-b" "effort" "xhigh"
               "composition_kind" "bespoke" "composition_id" "migration-forensics"
               "nearest_preset" "analyst" "bespoke_reason" "cross-schema archaeology"
               "promotion_candidate" "false"
               "composition_contract_sha256" (apply str (repeat 64 "a"))
               "composition_contract_fingerprint_version" "v1"
               "composition_contract_fingerprint_domain" "north:bespoke-contract:v1"
               "repo" "north" "goal" "prove clean sequential reuse"
               "spawned_at" "2026-07-17T01:01:00Z"
               "display_handle" "openai-b-sol-xhigh-gaffer-bespoke-probe"
               "display_name" "openai:codex-b · sol · xhigh · gaffer:bespoke:migration-forensics"}]
  (try
    (check "throwaway coordinator starts" (eventually #(port-open? port)))
    (let [first-result (run-writer port "publish" subject (json/generate-string preset))
          stored (scalar-facts (entity-facts port subject))]
      (check "preset publication returns a synchronous acknowledgement" (zero? (:exit first-result)))
      (check "commit marker matches the exact current canonical projection"
             (= (north.agent-provenance/manifest-sha256 stored)
                (get stored "identity_manifest_sha256"))))

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
    (let [before-op-count (count (log-ops log))
          second-result (run-writer port "publish" subject (json/generate-string bespoke))
          generation-ops (->> (log-ops log)
                              (drop before-op-count)
                              (filter #(= subject (:l %)))
                              vec)
          raw-stored (entity-facts port subject)
          stored (scalar-facts raw-stored)]
      (check "sequential reuse publishes the second shape" (zero? (:exit second-result)))
      (check "identity reuse withdraws identity and terminal markers before any body mutation"
             (= [["retract" "identity_manifest_sha256"]
                 ["retract" "terminal_manifest_sha256"]]
                (mapv (juxt :op :p) (take 2 generation-ops))))
      (check "identity reuse withdraws the legacy outcome before process_outcome"
             (= [["retract" "outcome"] ["retract" "process_outcome"]]
                (mapv (juxt :op :p) (take 2 (drop 2 generation-ops)))))
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
                 "model" "claude-opus-4-8" "effort" "high"
                 "display_handle" "anthropic-c-opus-high-gaffer-bespoke-probe"
                 "display_name" "anthropic:claude-c · opus · high · gaffer:bespoke:migration-forensics"}
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

    (let [retask {"goal" "new durable goal"
                  "display_name" "anthropic:claude-c · opus · high · gaffer:bespoke:migration-forensics · new durable goal"}
          retask-result (run-writer port "retask" subject (json/generate-string retask))
          raw-stored (entity-facts port subject)
          stored (scalar-facts raw-stored)]
      (check "typed retask is acknowledged" (zero? (:exit retask-result)))
      (check "typed retask leaves exactly one goal and one display cache"
             (and (= #{"new durable goal"} (get raw-stored "goal"))
                  (= #{(get retask "display_name")} (get raw-stored "display_name"))))
      (check "typed retask recommits a startup-valid identity"
             (and (north.agent-provenance/managed-valid? stored)
                  (= (north.agent-provenance/manifest-sha256 stored)
                     (get stored "identity_manifest_sha256")))))

    (let [before (entity-facts port subject)
          invalid (assoc bespoke "composition_contract_sha256" "not-a-hash")
          rejected (run-writer port "publish" subject (json/generate-string invalid))]
      (check "invalid identity is rejected before mutating the committed generation"
             (and (not (zero? (:exit rejected)))
                  (= before (entity-facts port subject)))))

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
          _ (mapv deref attempts)
          raw-stored (entity-facts port race-subject)
          stored (scalar-facts raw-stored)
          markers (get raw-stored "identity_manifest_sha256" #{})]
      (check "concurrent identity publication never blesses a mixed body"
             (or (empty? markers)
                 (and (= 1 (count markers))
                      (north.agent-provenance/managed-valid? stored)
                      (= (first markers)
                         (north.agent-provenance/manifest-sha256 stored))))))

    (let [race-subject "@agent:identity-route-retask-race"
          seeded
          (run-writer port "publish" race-subject
                      (json/generate-string preset))
          route
          {"provider" "openai" "provider_target" "codex-race"
           "model" "gpt-5.6-sol" "effort" "high"
           "display_handle" "openai-race-sol-high-integrator"
           "display_name" "openai:codex-race · sol · high · gaffer:integrator"}
          operations
          (mapv
           (fn [index]
             (future
               (if (even? index)
                 (run-writer port "route" race-subject
                             (json/generate-string route))
                 (run-writer
                  port "retask" race-subject
                  (json/generate-string
                   {"goal" (str "racing retask " index)
                    "display_name" (str "racing retask " index)})))))
           (range 16))
          _ (mapv deref operations)
          raw-stored (entity-facts port race-subject)
          stored (scalar-facts raw-stored)
          markers (get raw-stored "identity_manifest_sha256" #{})]
      (check "route/retask share the global identity marker seam"
             (and (zero? (:exit seeded))
                  (or (empty? markers)
                      (and (= 1 (count markers))
                           (north.agent-provenance/managed-valid? stored)
                           (= (first markers)
                              (north.agent-provenance/manifest-sha256 stored)))))))

    (let [worker-subject "@agent:delivery-worker"
          verifier-subject "@agent:delivery-verifier"
          worker (assoc preset
                        "role" "integrator" "composition_id" "integrator"
                        "goal" "deliver a proof-carrying change"
                        "display_handle" "anthropic-a-opus-high-integrator-worker"
                        "display_name" "anthropic:claude-a · opus · high · gaffer:integrator")
          verifier (assoc preset
                          "role" "verifier" "composition_id" "verifier"
                          "goal" "independently attest delivery"
                          "display_handle" "anthropic-a-opus-high-verifier-proof"
                          "display_name" "anthropic:claude-a · opus · high · gaffer:verifier")
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
                                     "NORTH_PORT" (str port)}}
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
