#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                "/home/tom/code/fram/wt-core-target-production-5db9b38"))))
(when-not (.isFile (io/file fram "bin/fram-server"))
  (throw (ex-info "current Fram checkout is required" {:fram fram})))
(def run-writer (str root "/cli/run-fact-internal.clj"))
(def evidence-writer (str root "/cli/delivery-evidence-internal.clj"))
(def north-mcp (str root "/bin/north-mcp"))
(def conformance
  (json/parse-string
   (slurp (str root "/sdk/test/fixtures/delivery-conformance.json"))))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/terminal-projection.clj"))
(load-file evidence-writer)

(def checks (atom []))
(def test-log (atom nil))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port [] (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try (with-open [socket (java.net.Socket.)]
         (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
         true)
       (catch Exception _ false)))
(defn eventually [predicate]
  (loop [n 0]
    (cond (predicate) true
          (>= n 200) false
          :else (do (Thread/sleep 25) (recur (inc n))))))
(defn facts-of [port subject]
  (let [rows (north.coord/show-rows port subject)]
    (reduce (fn [facts [predicate value]]
              (update facts predicate (fnil conj #{}) value))
            {}
            rows)))
(defn shell [& args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"FRAM_LOG" @test-log}}
         args))
(defn bars-cli [port & args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"FRAM_LOG" @test-log "NORTH_PORT" (str port)}}
         "bb" (str root "/cli/bars-cli.clj") args))
(defn north-cli
  "The real bash entrypoint, so verb wiring and pre-write hooks are covered."
  [port & args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"FRAM_LOG" @test-log "NORTH_PORT" (str port)}}
         (str root "/bin/north") args))
(defn evidence-cli
  "The real agent-facing CLI with NO run reservation in its environment."
  [port args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"FRAM_LOG" @test-log
                                 "NORTH_PORT" (str port)
                                 "AGENT_ID" "lane-unreserved-probe"
                                 "AGENT_TOPOLOGY" "worker"}}
         (str root "/bin/north") "evidence" args))
(defn mcp-request [environment method params]
  (let [request
        (json/generate-string
         {"jsonrpc" "2.0" "id" 1 "method" method "params" params})
        result
        (proc/shell {:out :string :err :string :continue true
                     :in (str request "\n")
                     :extra-env (merge {"FRAM_LOG" @test-log} environment)}
                    "bb" north-mcp)
        output (str/trim (:out result))]
    (assoc result :response
           (when (and (zero? (:exit result)) (not (str/blank? output)))
             (json/parse-string output)))))
(defn reserve-request [run thread reporter capability]
  {"run" run "thread" thread "reporter" reporter
   "capabilitySha256" (north.terminal-projection/sha256 capability)})
(defn record-request [run thread reporter capability bar observed]
  {"run" run "thread" thread "reporter" reporter
   "capability" capability "bar" bar "observed" observed})
(defn v2-snapshot [run thread reporter evidence]
  (json/generate-string
   (array-map
    "version" north.terminal-projection/delivery-evidence-version
    "run" run
    "thread" thread
    "reporter" reporter
    "contractOrigin" "accepted"
    "baselineDoneWhen" ["tests pass"]
    "doneWhen" ["tests pass"]
    "matches" [{"bar" "tests pass" "evidence" [evidence]}])))
(defn worker-defined-v2-snapshot
  [run thread reporter bar evidence]
  (json/generate-string
   (array-map
    "version" north.terminal-projection/delivery-evidence-version
    "run" run
    "thread" thread
    "reporter" reporter
    "contractOrigin" "worker-defined"
    "baselineDoneWhen" []
    "doneWhen" [bar]
    "matches" [{"bar" bar "evidence" [evidence]}])))
(defn run-payload [thread agent evidence]
  [["kind" "run"] ["thread" thread] ["agent" agent]
   ["duration_ms" "125"] ["outcome" "ran"] ["process_outcome" "ran"]
   ["delivery_outcome" "reported"]
   ["delivery_reason" "complete_run_scoped_done_bar_evidence_self_reported"]
   ["delivery_evidence" evidence]
   ["delivery_evidence_sha256" (north.terminal-projection/sha256 evidence)]])
(defn unverified-run-payload [thread agent]
  [["kind" "run"] ["thread" thread] ["agent" agent]
   ["duration_ms" "125"] ["outcome" "ran"] ["process_outcome" "ran"]
   ["delivery_outcome" "unverified"]
   ["delivery_reason" "delivery_bar_evidence_incomplete"]])

(let [attempts (atom 0)
      result
      (with-redefs
       [north.coord/append!
        (fn [& _]
          (swap! attempts inc)
          (throw (ex-info "simulated thread projection outage" {})))]
        (north.delivery-evidence-internal/best-effort-thread-projection!
         7977 "@thread-probe" "tests pass" "exit 0" []))]
  (check "human thread evidence outage cannot reverse canonical run success"
         (and (nil? result) (= 1 @attempts))))

;; The same rule covers the supersession half: a failed retract of the stale
;; human line is a projection nuisance, never a reversal of a committed record.
(let [retracts (atom 0)
      result
      (with-redefs
       [north.coord/append! (fn [& _] {:ok 1})
        north.coord/retract!
        (fn [& _]
          (swap! retracts inc)
          (throw (ex-info "simulated projection retract outage" {})))]
        (north.delivery-evidence-internal/best-effort-thread-projection!
         7977 "@thread-probe" "tests pass" "corrected" ["typo"]))]
  (check "a failed supersession projection retract cannot reverse run success"
         (and (nil? result) (= 1 @retracts))))

(let [error
      (try
        (north.delivery-evidence-internal/parse-request
         (apply str
                (repeat
                 (inc north.terminal-projection/max-delivery-writer-request-utf8-bytes)
                 " ")))
        nil
        (catch clojure.lang.ExceptionInfo caught caught))]
  (check "writer request byte cap rejects before JSON parsing"
         (= "delivery evidence request exceeds its UTF-8 byte limit"
            (some-> error .getMessage))))

(let [port (free-port)
      tmp (.toFile (java.nio.file.Files/createTempDirectory
                    "north-run-publication" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "coordination.framlog")
      daemon (do
               (proc/process
                {:dir fram :out :string :err :string
                 :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                             "FRAM_SERVER_QUIET" "1"
                             "FRAM_SERVER_XMX" "1g"}}
                (str fram "/bin/fram-server") "serve" (str port)
                (.getCanonicalPath log) "north-coordination"))
      run "@run-publication-v2"
      thread "@thread-publication-v2"
      reporter "@agent:lane-probe"
      capability (apply str (repeat 64 "a"))]
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
    (let [partial-run "@run-failed-reservation-partial"
          fresh-run "@run-failed-reservation-recovery"]
      (north.coord/append! port partial-run "run_reservation_agent" reporter)
      (let [poisoned
            (shell "bb" run-writer (str port) partial-run
                   (json/generate-string
                    (unverified-run-payload (subs thread 1)
                                            (subs reporter (count "@agent:")))))
            recovered
            (shell "bb" run-writer (str port) fresh-run
                   (json/generate-string
                    (unverified-run-payload (subs thread 1)
                                            (subs reporter (count "@agent:")))))]
        (check "partial failed reservation cannot masquerade as telemetry"
               (and (not (zero? (:exit poisoned)))
                    (nil? (get (facts-of port partial-run) "kind"))))
        (check "fresh telemetry-only run commits after reservation failure"
               (and (zero? (:exit recovered))
                    (= "ran"
                       (north.terminal-projection/committed-run-process-outcome
                        (facts-of port fresh-run)))))))
    (let [non-thread "@factful-non-thread"
          rejected-run "@run-non-thread-reservation"]
      (north.coord/append! port non-thread "done_when" "looks thread-like")
      (let [rejected
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request rejected-run non-thread reporter capability)))]
        (check "reservation requires a title-bearing North thread"
               (and (not (zero? (:exit rejected)))
                    (empty? (facts-of port rejected-run))))))
    ;; The delegate path reserves against a thread captured MOMENTS earlier; pin
    ;; that exact ordering (capture-shaped facts, then an immediate reserve).
    (let [fresh-thread "@019f9d70-8727-74e3-8168-7d5082b47e54"
          fresh-thread-run "@run-freshly-captured-thread"]
      (doseq [[predicate value] [["kind" "thread"]
                                 ["title" "Freshly captured thread"]
                                 ["done_when" "Probe: reserve. Expected: ok true."]
                                 ["committed" "2026-07-26"]]]
        (north.coord/append! port fresh-thread predicate value))
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request fresh-thread-run fresh-thread reporter
                                     (apply str (repeat 64 "e")))))]
        (check "a freshly captured thread reserves delivery evidence"
               (and (zero? (:exit reserved))
                    (north.terminal-projection/run-reservation-valid?
                     (facts-of port fresh-thread-run))))
        (let [stored (facts-of port fresh-thread-run)
              receipt (north.terminal-projection/singleton-value
                       stored "run_reservation_manifest_sha256")
              competing "@agent:lane-competing-holder"
              refused
              (shell "bb" evidence-writer (str port) "reserve"
                     (json/generate-string
                      (reserve-request fresh-thread-run fresh-thread competing
                                       (apply str (repeat 64 "d")))))
              diagnostic (:err refused)]
          (check "a legitimate concurrent holder is refused without mutation"
                 (and (not (zero? (:exit refused)))
                      (= stored (facts-of port fresh-thread-run))))
          (check "reservation refusal names exact run, holder, receipt, and reason"
                 (and (str/includes? diagnostic
                                     (str "run=" fresh-thread-run))
                      (str/includes? diagnostic (str "holder=" reporter))
                      (str/includes? diagnostic (str "receipt=" receipt))
                      (str/includes? diagnostic
                                     "reason=existing-reservation"))))))
    ;; ACKNOWLEDGEMENT LOSS. Publication is atomic, so the only thing a caller
    ;; can lose after success is the receipt. A same-run retry carrying the
    ;; EXACT run/thread/reporter/capability is replaying its own reservation and
    ;; must get the canonical acknowledgement back, byte for byte, without
    ;; touching the graph. Anything else on that subject is a refusal.
    (let [replay-run "@run-acknowledgement-loss"
          replay-thread "@thread-acknowledgement-loss"
          replay-capability (apply str (repeat 64 "9"))
          other-thread "@thread-acknowledgement-loss-other"]
      (doseq [[subject title] [[replay-thread "Acknowledgement loss replay"]
                               [other-thread "Other reservation subject"]]]
        (north.coord/append! port subject "title" title)
        (north.coord/append! port subject "done_when" "publication is atomic"))
      (let [first-reserve
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request replay-run replay-thread reporter
                                     replay-capability)))
            stored (facts-of port replay-run)
            replay
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request replay-run replay-thread reporter
                                     replay-capability)))
            wrong-reporter
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request replay-run replay-thread
                                     "@agent:lane-other-holder"
                                     replay-capability)))
            wrong-capability
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request replay-run replay-thread reporter
                                     (apply str (repeat 64 "7")))))
            wrong-thread
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request replay-run other-thread reporter
                                     replay-capability)))]
        (check "an exact same-run replay returns the canonical acknowledgement"
               (and (zero? (:exit first-reserve))
                    (zero? (:exit replay))
                    (= (str/trim (:out first-reserve)) (str/trim (:out replay)))
                    (not (str/blank? (:out replay)))))
        (check "an exact same-run replay mutates nothing"
               (and (north.terminal-projection/run-reservation-valid? stored)
                    (= (set north.terminal-projection/run-reservation-predicates)
                       (set (keys stored)))
                    (= stored (facts-of port replay-run))))
        (check "a different reporter, capability, or thread is refused, never replayed"
               (and (every? #(not (zero? (:exit %)))
                            [wrong-reporter wrong-capability wrong-thread])
                    (every? #(str/includes? (:err %) "reason=existing-reservation")
                            [wrong-reporter wrong-capability wrong-thread])
                    (= stored (facts-of port replay-run))))))
    ;; A SUBSET of a reservation — the exact residue a non-atomic publisher
    ;; could leave — is never completed in place and never replayed.
    (let [partial-run "@run-partial-reservation-subject"
          partial-thread "@thread-partial-reservation-subject"]
      (north.coord/append! port partial-thread "title" "Partial reservation subject")
      (north.coord/append! port partial-thread "done_when" "publication is atomic")
      (north.coord/append! port partial-run "run_reservation_agent" reporter)
      (north.coord/append! port partial-run "run_reservation_version"
                           north.terminal-projection/run-reservation-version)
      (let [before (facts-of port partial-run)
            refused
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request partial-run partial-thread reporter
                                     capability)))]
        (check "a partial reservation subject refuses without completing itself in place"
               (and (not (zero? (:exit refused)))
                    (str/includes? (:err refused) "reason=run-subject-not-fresh")
                    (= before (facts-of port partial-run))))))
    (let [oversized-run "@run-oversized-contract"
          oversized-thread "@thread-oversized-contract"]
      (north.coord/append! port oversized-thread "title" "Oversized contract")
      (doseq [index (range (inc north.terminal-projection/max-delivery-bars))]
        (north.coord/append! port oversized-thread "done_when"
                             (format "probe %02d" index)))
      (let [rejected
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request oversized-run oversized-thread
                                     reporter capability)))]
        (check "oversized done_when contract is rejected before partial reservation"
               (and (not (zero? (:exit rejected)))
                    (empty? (facts-of port oversized-run))))))
    (let [oversized-run "@run-oversized-bar"
          oversized-thread "@thread-oversized-bar"]
      (north.coord/append! port oversized-thread "title" "Oversized bar")
      (north.coord/append!
       port oversized-thread "done_when"
       (apply str
              (repeat (inc north.terminal-projection/max-delivery-bar-utf8-bytes)
                      "a")))
      (let [rejected
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request oversized-run oversized-thread
                                     reporter capability)))]
        (check "513-byte done_when is rejected before partial reservation"
               (and (not (zero? (:exit rejected)))
                    (empty? (facts-of port oversized-run))))))
    (let [bounded-run "@run-multibyte-boundary"
          bounded-thread "@thread-multibyte-boundary"
          bounded-capability (apply str (repeat 64 "8"))
          exact-bar (apply str (repeat 128 "🧪"))
          exact-observed
          (apply str
                 (repeat
                  north.terminal-projection/max-delivery-observed-utf8-bytes
                  "o"))]
      (north.coord/append! port bounded-thread "title" "Multibyte boundary")
      (north.coord/append! port bounded-thread "done_when" exact-bar)
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request bounded-run bounded-thread reporter
                                     bounded-capability)))
            exact
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request bounded-run bounded-thread reporter
                                    bounded-capability exact-bar exact-observed)))
            over
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request bounded-run bounded-thread reporter
                                    bounded-capability
                                    (str exact-bar "🧪") exact-observed)))
            observed-over
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request
                     bounded-run bounded-thread reporter bounded-capability
                     exact-bar (str exact-observed "o"))))]
        (check "exact multibyte bar/observation byte boundaries are accepted"
               (and (zero? (:exit reserved)) (zero? (:exit exact))
                    (= 1 (count (get (facts-of port bounded-run)
                                     "run_bar_evidence" #{})))))
        (check "one multibyte scalar over the bar limit is rejected without a record"
               (and (not (zero? (:exit over)))
                    (= 1 (count (get (facts-of port bounded-run)
                                     "run_bar_evidence" #{})))))
        (check "2049-byte observation is rejected without a record"
               (and (not (zero? (:exit observed-over)))
                    (= 1 (count (get (facts-of port bounded-run)
                                     "run_bar_evidence" #{})))))))
    (north.coord/append! port thread "title" "Publication test")
    (north.coord/append! port thread "done_when" "tests pass")
    (let [result (shell "bb" evidence-writer (str port) "reserve"
                        (json/generate-string
                         (reserve-request run thread reporter capability)))]
      (when-not (zero? (:exit result)) (binding [*out* *err*] (println (:err result))))
      (check "fresh run reservation commits before execution" (zero? (:exit result)))
      (check "reservation is singleton and digest-valid"
             (north.terminal-projection/run-reservation-valid? (facts-of port run))))
    ;; Reserving twice is no longer one case. Publication is atomic, so an exact
    ;; repeat of THIS run's own request can only mean a lost acknowledgement and
    ;; replays; any rebinding of the immutable run subject is still refused.
    (let [before (facts-of port run)
          duplicate (shell "bb" evidence-writer (str port) "reserve"
                           (json/generate-string
                            (reserve-request run thread reporter capability)))
          rebound (shell "bb" evidence-writer (str port) "reserve"
                         (json/generate-string
                          (reserve-request run thread "@agent:lane-rebinding"
                                           capability)))]
      (check "an exact same-run reserve replays instead of publishing twice"
             (and (zero? (:exit duplicate))
                  (= before (facts-of port run))))
      (check "a rebound run subject cannot be reserved twice"
             (and (not (zero? (:exit rebound)))
                  (= before (facts-of port run)))))
    (let [wrong-cap (shell "bb" evidence-writer (str port) "record"
                           (json/generate-string
                            (record-request run thread reporter
                                            (apply str (repeat 64 "b"))
                                            "tests pass" "24/24")))]
      (check "wrong run capability cannot author evidence" (not (zero? (:exit wrong-cap)))))
    (let [mcp-run "@run-mcp-evidence"
          mcp-thread "@thread-mcp-evidence"
          mcp-reporter "@agent:readonly-mcp-worker"
          mcp-capability (apply str (repeat 64 "6"))
          mcp-bar "read-only worker records exact proof"
          mcp-environment
          {"NORTH_BIN" (str root "/bin/north")
           "NORTH_PORT" (str port)
           "AGENT_ID" (subs mcp-reporter (count "@agent:"))
           "AGENT_TOPOLOGY" "worker"
           "NORTH_RUN_ID" (subs mcp-run 1)
           "NORTH_THREAD_ID" (subs mcp-thread 1)
           "NORTH_RUN_CAPABILITY" mcp-capability
           "FRAM_LOG" @test-log}]
      (north.coord/append! port mcp-thread "title" "MCP evidence binding")
      (north.coord/append! port mcp-thread "done_when" mcp-bar)
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request mcp-run mcp-thread mcp-reporter
                                     mcp-capability)))
            listed (mcp-request mcp-environment "tools/list" {})
            descriptor
            (some #(when (= "evidence_record" (get % "name")) %)
                  (get-in listed [:response "result" "tools"]))
            forged
            (mcp-request
             mcp-environment "tools/call"
             {"name" "evidence_record"
              "arguments"
              {"bar" mcp-bar "observed" "exit 0"
               "run" "run-other" "thread" "thread-other"
               "reporter" "agent:other" "capability" (apply str (repeat 64 "5"))}})
            after-forgery (facts-of port mcp-run)
            wrong-log
            (mcp-request
             (assoc mcp-environment "FRAM_LOG" (str @test-log ".wrong"))
             "tools/call"
             {"name" "evidence_record"
              "arguments" {"bar" mcp-bar "observed" "exit 0"}})
            after-wrong-log (facts-of port mcp-run)
            recorded
            (mcp-request
             mcp-environment "tools/call"
             {"name" "evidence_record"
              "arguments" {"bar" mcp-bar "observed" "exit 0"}})
            stored (facts-of port mcp-run)
            record
            (some-> recorded :response (get-in ["result" "content" 0 "text"])
                    json/parse-string)]
        (check "MCP evidence run reserves against the exact coordinator"
               (zero? (:exit reserved)))
        (check "evidence_record exposes only bar and observed in its MCP schema"
               (= {"type" "object"
                   "properties" {"bar" {"type" "string" "minLength" 1}
                                 "observed" {"type" "string" "minLength" 1}}
                   "required" ["bar" "observed"]
                   "additionalProperties" false}
                  (get descriptor "inputSchema")))
        (check "evidence_record rejects caller-supplied run identity fields"
               (and (true? (get-in forged [:response "result" "isError"]))
                    (empty? (get after-forgery "run_bar_evidence" #{}))))
        (check "evidence_record preserves the managed Fram log fence"
               (and (true? (get-in wrong-log [:response "result" "isError"]))
                    (empty? (get after-wrong-log "run_bar_evidence" #{}))))
        (check "read-only worker records evidence through MCP end-to-end"
               (and (false? (get-in recorded [:response "result" "isError"]))
                    (= mcp-run (get record "run"))
                    (= mcp-thread (get record "thread"))
                    (= mcp-reporter (get record "reporter"))
                    (= mcp-bar (get record "bar"))
                    (= "exit 0" (get record "observed"))
                    (= 1 (count (get stored "run_bar_evidence" #{})))
                    (= #{(str mcp-bar " → exit 0")}
                       (get (facts-of port mcp-thread) "bar_evidence"))))))
    (let [normalized-run "@run-normalized-bar"
          normalized-thread "@thread-normalized-bar"
          normalized-capability (apply str (repeat 64 "e"))]
      (north.coord/append! port normalized-thread "done_when" "  padded probe  ")
      (north.coord/append! port normalized-thread "title" "Normalized bar")
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request normalized-run normalized-thread reporter
                                     normalized-capability)))
            normalized
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request normalized-run normalized-thread reporter
                                    normalized-capability
                                    "padded probe" "normalized, exit 0")))]
        (check "normalized done-bar reservation commits" (zero? (:exit reserved)))
        (check "done-bar matching uses the same ASCII-space normalization as proof snapshots"
               (zero? (:exit normalized)))))
    (let [recorded (shell "bb" evidence-writer (str port) "record"
                          (json/generate-string
                           (record-request run thread reporter capability
                                           "tests pass" "24/24, exit 0")))
          first-record (json/parse-string (:out recorded))
          retried (shell "bb" evidence-writer (str port) "record"
                         (json/generate-string
                          (record-request run thread reporter capability
                                          "tests pass" "24/24, exit 0")))
          ;; A typo used to burn the bar's only slot for the life of the run.
          corrected (shell "bb" evidence-writer (str port) "record"
                           (json/generate-string
                            (record-request run thread reporter capability
                                            "tests pass" "typo: 23/24")))
          corrected-record (json/parse-string (:out corrected))
          after-supersede (get (facts-of port run) "run_bar_evidence" #{})
          thread-after-supersede (get (facts-of port thread) "bar_evidence" #{})
          history-events
          (:events
           (north.coord/occurrence-window
            port 0 (north.coord/cur-ver port)))
          restored (shell "bb" evidence-writer (str port) "record"
                          (json/generate-string
                           (record-request run thread reporter capability
                                           "tests pass" "24/24, exit 0")))
          record (json/parse-string (:out restored))
          snapshot (v2-snapshot run thread reporter record)]
      (when-not (zero? (:exit recorded)) (binding [*out* *err*] (println (:err recorded))))
      (check "same run/bar observation retry returns the one committed record"
             (and (zero? (:exit retried))
                  (= (:out recorded) (:out retried))
                  (= 1 (count (get (facts-of port run)
                                   "run_bar_evidence" #{})))))
      (check "a same-run re-record supersedes the prior observation in place"
             (and (zero? (:exit corrected))
                  (= 1 (count after-supersede))
                  (= "typo: 23/24" (get corrected-record "observed"))
                  (= #{(str/trim (:out corrected))}
                     (set (map str/trim after-supersede)))))
      (check "supersession keeps run/thread/reporter provenance intact"
             (= (select-keys first-record ["bar" "reporter" "run" "thread" "version"])
                (select-keys corrected-record
                             ["bar" "reporter" "run" "thread" "version"])))
      (check "the superseded observation stays in the append-only log"
             (and (some #(and (= :retract (:operation %))
                              (= run (:subject %))
                              (= "run_bar_evidence" (:predicate %))
                              (str/includes? (:value %) "24/24, exit 0"))
                        history-events)
                  (some #(and (= :assert (:operation %))
                              (= run (:subject %))
                              (= "run_bar_evidence" (:predicate %))
                              (str/includes? (:value %) "24/24, exit 0"))
                        history-events)))
      (check "the human thread projection follows the correction"
             (= #{"tests pass → typo: 23/24"} thread-after-supersede))
      (check "the corrected observation can itself be corrected back"
             (and (zero? (:exit restored))
                  (= 1 (count (get (facts-of port run) "run_bar_evidence" #{})))
                  (= "24/24, exit 0" (get record "observed"))
                  (= #{"tests pass → 24/24, exit 0"}
                     (get (facts-of port thread) "bar_evidence"))))
      (doseq [[label injected]
              [["uncited valid"
                (json/generate-string
                 (into (sorted-map)
                       (assoc record
                              "bar" "uncited extra bar"
                              "observed" "not in snapshot"
                              "recordedAt" "2026-07-18T10:00:01Z")))]
               ["malformed" "{"]
               ["duplicate bar"
                (json/generate-string
                 (into (sorted-map)
                       (assoc record
                              "observed" "second stored observation"
                              "recordedAt" "2026-07-18T10:00:02Z")))]]]
        (north.coord/append! port run "run_bar_evidence" injected)
        (let [rejected
              (shell "bb" run-writer (str port) run
                     (json/generate-string
                      (run-payload (subs thread 1)
                                   (subs reporter (count "@agent:"))
                                   snapshot)))]
          (check (str "run marker rejects " label " stored evidence")
                 (and (not (zero? (:exit rejected)))
                      (nil? (get (facts-of port run) "kind")))))
        (north.coord/retract! port run "run_bar_evidence" injected))
      (let [relabelled-map
            (-> (json/parse-string snapshot)
                (assoc "contractOrigin" "worker-defined")
                (assoc "baselineDoneWhen" []))
            relabelled (json/generate-string relabelled-map)
            rejected
            (shell "bb" run-writer (str port) run
                   (json/generate-string
                    (run-payload (subs thread 1)
                                 (subs reporter (count "@agent:"))
                                 relabelled)))]
        (check "run snapshot cannot relabel an accepted reservation as worker-defined"
               (and (not (zero? (:exit rejected)))
                    (nil? (get (facts-of port run) "kind")))))
      (north.coord/append! port thread "done_when" "late weaker bar")
      (let [changed
            (shell "bb" run-writer (str port) run
                   (json/generate-string
                    (run-payload (subs thread 1)
                                 (subs reporter (count "@agent:"))
                                 snapshot)))]
        (check "run publication rejects a changed current done-bar set"
               (and (not (zero? (:exit changed)))
                    (nil? (get (facts-of port run) "kind")))))
      (north.coord/retract! port thread "done_when" "late weaker bar")
      (let [terminal-facts
            (run-payload (subs thread 1)
                         (subs reporter (count "@agent:"))
                         snapshot)
            published
            (shell "bb" run-writer (str port) run
                   (json/generate-string terminal-facts))
            stored (facts-of port run)
            terminal-pairs (set terminal-facts)
            terminal-rows
            (->> (:events
                  (north.coord/occurrence-window
                   port 0 (north.coord/cur-ver port)))
                 (filter #(and (= :assert (:operation %))
                               (= run (:subject %))
                               (terminal-pairs
                                [(:predicate %) (:value %)]))))
            terminal-transactions (set (map :version terminal-rows))]
      (when-not (zero? (:exit published)) (binding [*out* *err*] (println (:err published))))
      (check "writer-scoped run evidence records" (zero? (:exit recorded)))
      (check "v2 reported run commits with exact stored evidence" (zero? (:exit published)))
      (check "every terminal run fact including kind shares one transaction"
             (and (= (count terminal-facts) (count terminal-rows))
                  (= 1 (count terminal-transactions))
                  (some #(and (= "kind" (:predicate %))
                              (= "run" (:value %)))
                        terminal-rows)))
      (check "kind is the final discoverability marker"
             (= "ran"
                (north.terminal-projection/committed-run-process-outcome stored)))
      (let [reused (shell "bb" run-writer (str port) run
                          (json/generate-string
                           (run-payload (subs thread 1)
                                        (subs reporter (count "@agent:"))
                                        snapshot)))]
        (check "committed run subject reuse is rejected" (not (zero? (:exit reused)))))
      (north.coord/retract! port thread "bar_evidence"
                            "tests pass → 24/24, exit 0")
      (let [replayed
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request run thread reporter capability
                                    "tests pass" "24/24, exit 0")))]
        (check "exact post-terminal replay heals only the human projection"
               (and (zero? (:exit replayed))
                    (= (:out restored) (:out replayed))
                    (= 1 (count (get (facts-of port run)
                                     "run_bar_evidence" #{})))
                    (= #{"tests pass → 24/24, exit 0"}
                       (get (facts-of port thread) "bar_evidence")))))
      (let [late-evidence
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request run thread reporter capability
                                    "tests pass" "late replacement")))]
        (check "terminal publication closes new writer-scoped evidence"
               (not (zero? (:exit late-evidence)))))
      (let [forged-map (assoc (json/parse-string snapshot)
                              "threadOutcome" "fabricated narrative")
            forged (json/generate-string forged-map)]
        (check "unbound narrative fields fail pure Clojure proof validation"
               (not
                (north.terminal-projection/delivery-projection-valid?
                 {"outcome" "ran" "process_outcome" "ran"
                  "delivery_outcome" "reported"
                  "delivery_reason" "complete_run_scoped_done_bar_evidence_self_reported"
                  "delivery_evidence" forged
                  "delivery_evidence_sha256"
                  (north.terminal-projection/sha256 forged)})))))
      )
    (check "shared TS/Clojure valid instant fixtures agree"
           (every? north.terminal-projection/instant?
                   (get conformance "validInstants")))
    (check "shared TS/Clojure invalid instant fixtures agree"
           (not-any? north.terminal-projection/instant?
                     (get conformance "invalidInstants")))
    (let [reservation-body (into (sorted-map) (get conformance "reservationBody"))
          reservation-facts
          (assoc (into {} (map (fn [[predicate value]]
                                 [predicate #{value}])
                               reservation-body))
                 "run_reservation_manifest_sha256"
                 #{(get conformance "reservationManifestSha256")})]
    (check "shared TS/Clojure reservation manifest digest agrees"
             (and (= (get conformance "reservationManifestSha256")
                     (north.terminal-projection/run-reservation-manifest-sha256
                      reservation-body))
                  (north.terminal-projection/run-reservation-valid?
                   reservation-facts))))
    (let [limits (get conformance "limits")]
      (check "shared TS/Clojure evidence byte and count limits agree"
             (= limits
                {"maxBars" north.terminal-projection/max-delivery-bars
                 "maxBarUtf8Bytes"
                 north.terminal-projection/max-delivery-bar-utf8-bytes
                 "maxObservedUtf8Bytes"
                 north.terminal-projection/max-delivery-observed-utf8-bytes
                 "maxEnvelopeUtf8Bytes"
                 north.terminal-projection/max-delivery-envelope-utf8-bytes
                 "maxRecordUtf8Bytes"
                 north.terminal-projection/max-run-bar-evidence-record-utf8-bytes
                 "maxReservationBaselineUtf8Bytes"
                 north.terminal-projection/max-run-reservation-baseline-utf8-bytes
                 "maxWriterRequestUtf8Bytes"
                 north.terminal-projection/max-delivery-writer-request-utf8-bytes
                 "maxThreadIdUtf8Bytes"
                 north.terminal-projection/max-delivery-thread-id-utf8-bytes
                 "maxRunIdUtf8Bytes"
                 north.terminal-projection/max-delivery-run-id-utf8-bytes
                 "maxAgentIdUtf8Bytes"
                 north.terminal-projection/max-delivery-agent-id-utf8-bytes
                 "maxAttestationUtf8Bytes"
                 north.terminal-projection/max-delivery-attestation-utf8-bytes}))
      (check "Clojure evidence bounds count multibyte UTF-8 bytes"
             (and
              (north.terminal-projection/bounded-nonblank-text?
               (apply str (repeat 128 "🧪"))
               north.terminal-projection/max-delivery-bar-utf8-bytes)
              (not
               (north.terminal-projection/bounded-nonblank-text?
                (apply str (repeat 129 "🧪"))
                north.terminal-projection/max-delivery-bar-utf8-bytes))))
      (check "shared proof text canonicalization fixtures agree"
             (every?
              (fn [case]
                (= (get case "canonical")
                   (north.terminal-projection/canonical-evidence-text
                    (get case "raw"))))
              (get conformance "textCases")))
      (check "shared thread entity grammar fixtures agree"
             (and
              (every? north.terminal-projection/valid-thread-entity?
                      (get conformance "validThreadEntities"))
              (not-any? north.terminal-projection/valid-thread-entity?
                        (get conformance "invalidThreadEntities"))))
      (check "raw done_when floods cannot collapse under the 32-bar cap"
             (nil?
              (north.terminal-projection/canonical-done-when
               {"done_when"
                (set
                 (map (fn [index]
                        (str (apply str (repeat (inc index) " "))
                             "tests pass"))
                      (range
                       (inc
                        north.terminal-projection/max-delivery-bars))))}))))
    (let [flood-run "@run-record-flood"
          flood-thread "@thread-record-flood"
          flood-capability (apply str (repeat 64 "9"))
          flood-bar "one bounded observation"]
      (north.coord/append! port flood-thread "title" "Evidence flood")
      (north.coord/append! port flood-thread "done_when" flood-bar)
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request flood-run flood-thread reporter
                                     flood-capability)))
            attempts
            (doall
             (repeatedly
              16
              #(future
                 (shell "bb" evidence-writer (str port) "record"
                        (json/generate-string
                         (record-request flood-run flood-thread reporter
                                         flood-capability flood-bar
                                         "same observed result"))))))
            results (mapv deref attempts)
            stored (get (facts-of port flood-run) "run_bar_evidence" #{})]
        (check "same-bar append flood converges to one idempotent record"
               (and (zero? (:exit reserved))
                    (every? #(zero? (:exit %)) results)
                    (= 1 (count (set (map :out results))))
                    (= 1 (count stored))))))
    (let [race-run "@run-record-conflict-race"
          race-thread "@thread-record-conflict-race"
          race-capability (apply str (repeat 64 "7"))
          race-bar "one winner only"]
      (north.coord/append! port race-thread "title" "Evidence conflict race")
      (north.coord/append! port race-thread "done_when" race-bar)
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request race-run race-thread reporter
                                     race-capability)))
            left
            (future
              (shell "bb" evidence-writer (str port) "record"
                     (json/generate-string
                      (record-request race-run race-thread reporter
                                      race-capability race-bar "left result"))))
            right
            (future
              (shell "bb" evidence-writer (str port) "record"
                     (json/generate-string
                      (record-request race-run race-thread reporter
                                      race-capability race-bar "right result"))))
            results [@left @right]
            successes (filterv #(zero? (:exit %)) results)
            stored (get (facts-of port race-run) "run_bar_evidence" #{})]
        ;; Supersession makes both differing observations legal writes, but the
        ;; bar still holds exactly ONE live record and it must be one an actual
        ;; writer acknowledged — never a torn or duplicated pair.
        (check "concurrent differing observations converge on one live record"
               (and (zero? (:exit reserved))
                    (= 2 (count successes))
                    (= 1 (count stored))
                    (contains? (set (map #(json/parse-string (:out %)) successes))
                               (json/parse-string (first stored)))))))
    (let [worker-run "@run-worker-defined-contract"
          worker-thread "@thread-worker-defined-contract"
          worker-capability (apply str (repeat 64 "f"))
          worker-bar "worker-defined probe"]
      (north.coord/append! port worker-thread "title" "Worker-defined contract")
      (let [reserved
            (shell "bb" evidence-writer (str port) "reserve"
                   (json/generate-string
                    (reserve-request worker-run worker-thread reporter
                                     worker-capability)))]
        (check "empty starting contract reserves explicitly as worker-defined"
               (and (zero? (:exit reserved))
                    (= #{"worker-defined"}
                       (get (facts-of port worker-run)
                            "run_reservation_contract_origin")))))
      (north.coord/append! port worker-thread "done_when" worker-bar)
      (let [recorded
            (shell "bb" evidence-writer (str port) "record"
                   (json/generate-string
                    (record-request worker-run worker-thread reporter
                                    worker-capability worker-bar "exit 0")))
            record (json/parse-string (:out recorded))
            snapshot
            (worker-defined-v2-snapshot
             worker-run worker-thread reporter worker-bar record)
            published
            (shell "bb" run-writer (str port) worker-run
                   (json/generate-string
                    (run-payload (subs worker-thread 1)
                                 (subs reporter (count "@agent:"))
                                 snapshot)))]
        (check "worker-defined contract remains explicit through run commit"
               (and (zero? (:exit recorded)) (zero? (:exit published))
                    (= "ran"
                       (north.terminal-projection/committed-run-process-outcome
                        (facts-of port worker-run)))))))
    ;; Supersession is scoped to ONE run: a second run on the same thread and
    ;; bar writes its own record and can never rewrite or erase the first.
    (let [thread-two "@thread-two-run-isolation"
          run-a "@run-isolation-a"
          run-b "@run-isolation-b"
          bar "isolated probe"
          capability-a (apply str (repeat 64 "1"))
          capability-b (apply str (repeat 64 "2"))]
      (north.coord/append! port thread-two "title" "Two-run isolation")
      (north.coord/append! port thread-two "done_when" bar)
      (let [reserved-a (shell "bb" evidence-writer (str port) "reserve"
                              (json/generate-string
                               (reserve-request run-a thread-two reporter
                                                capability-a)))
            reserved-b (shell "bb" evidence-writer (str port) "reserve"
                              (json/generate-string
                               (reserve-request run-b thread-two reporter
                                                capability-b)))
            recorded-a (shell "bb" evidence-writer (str port) "record"
                              (json/generate-string
                               (record-request run-a thread-two reporter
                                               capability-a bar "run A result")))
            recorded-b (shell "bb" evidence-writer (str port) "record"
                              (json/generate-string
                               (record-request run-b thread-two reporter
                                               capability-b bar "run B result")))
            cross (shell "bb" evidence-writer (str port) "record"
                         (json/generate-string
                          (record-request run-a thread-two reporter
                                          capability-b bar "forged correction")))
            stored-a (get (facts-of port run-a) "run_bar_evidence" #{})
            stored-b (get (facts-of port run-b) "run_bar_evidence" #{})]
        (check "another run cannot supersede this run's observation"
               (and (zero? (:exit reserved-a)) (zero? (:exit reserved-b))
                    (zero? (:exit recorded-a)) (zero? (:exit recorded-b))
                    (= 1 (count stored-a)) (= 1 (count stored-b))
                    (= "run A result"
                       (get (json/parse-string (first stored-a)) "observed"))
                    (= "run B result"
                       (get (json/parse-string (first stored-b)) "observed"))))
        (check "a wrong capability cannot supersede an existing observation"
               (and (not (zero? (:exit cross)))
                    (= "run A result"
                       (get (json/parse-string
                             (first (get (facts-of port run-a)
                                         "run_bar_evidence" #{})))
                            "observed"))))))
    ;; MULTI-BAR RECOVERY: the bound stays, but the error names the bars and one
    ;; documented verb gets the same thread to a clean reserve.
    (let [crowded-thread "@thread-crowded-contract"
          crowded-run "@run-crowded-contract"
          crowded-capability (apply str (repeat 64 "3"))]
      (north.coord/append! port crowded-thread "title" "Crowded contract")
      (doseq [index (range (inc north.terminal-projection/max-delivery-bars))]
        (north.coord/append! port crowded-thread "done_when"
                             (format "crowded probe %02d" index)))
      ;; Half the bars were answered on earlier runs; those are the stale ones.
      (doseq [index (range 20)]
        (north.coord/append! port crowded-thread "bar_evidence"
                             (format "crowded probe %02d → exit 0" index)))
      (let [rejected (shell "bb" evidence-writer (str port) "reserve"
                            (json/generate-string
                             (reserve-request crowded-run crowded-thread reporter
                                              crowded-capability)))
            listed (bars-cli port "list" (subs crowded-thread 1))
            dry (bars-cli port "prune" (subs crowded-thread 1) "--dry-run")
            bars-after-dry (count (get (facts-of port crowded-thread)
                                       "done_when" #{}))
            bars-before bars-after-dry
            pruned (bars-cli port "prune" (subs crowded-thread 1))
            bars-after (count (get (facts-of port crowded-thread) "done_when" #{}))
            reserved (shell "bb" evidence-writer (str port) "reserve"
                            (json/generate-string
                             (reserve-request crowded-run crowded-thread reporter
                                              crowded-capability)))]
        (check "the reserve error names the active bars verbatim"
               (and (not (zero? (:exit rejected)))
                    (str/includes? (:err rejected) "crowded probe 00")
                    (str/includes? (:err rejected) "crowded probe 32")
                    (str/includes? (:err rejected) "north bars prune")))
        (check "bars list shows the evidenced/open split and the blocked reserve"
               (and (zero? (:exit listed))
                    (str/includes? (:out listed) "✓")
                    (str/includes? (:out listed) "○")
                    (str/includes? (:out listed) "reserve BLOCKED")))
        (check "bars prune --dry-run retires nothing"
               (and (zero? (:exit dry))
                    (str/includes? (:out dry) "would retire")
                    (= 33 bars-after-dry)))
        (check "one prune step returns a crowded thread to a clean reserve"
               (and (zero? (:exit pruned))
                    (= 33 bars-before)
                    (= 13 bars-after)
                    (zero? (:exit reserved))
                    (north.terminal-projection/run-reservation-valid?
                     (facts-of port crowded-run))))
        (check "an Nth done_when tell warns that the reserve limit is exceeded"
               (let [warned (bars-cli port "check" (subs crowded-thread 1)
                                      "one more probe")]
                 (and (zero? (:exit warned))
                      (not (str/includes? (:err warned) "WARNING")))))
        (check "north bars is wired as a first-class CLI verb"
               (let [via-cli (north-cli port "bars" "list" (subs crowded-thread 1))]
                 (and (zero? (:exit via-cli))
                      (str/includes? (:out via-cli) "DONE BARS on"))))
        (check "the warning fires exactly when the next bar breaks the limit"
               (do
                 (doseq [index (range 13 33)]
                   (north.coord/append! port crowded-thread "done_when"
                                        (format "refilled probe %02d" index)))
                 (let [warned (bars-cli port "check" (subs crowded-thread 1)
                                        "one bar too many")
                       oversized (bars-cli port "check" (subs crowded-thread 1)
                                           (apply str (repeat 600 "x")))]
                   (and (zero? (:exit warned))
                        (str/includes? (:err warned) "reserve limit is 32")
                        (str/includes? (:err warned) "north bars prune")
                        (str/includes? (:err oversized) "per-bar reserve limit"))))))
        ;; Only the ADVISORY hook is under test here: the sandboxed fram write
        ;; behind it is fenced to a different log and is expected to be refused.
        (check "an Nth done_when tell warns before the write lands"
               (let [told (north-cli port "tell" (subs crowded-thread 1)
                                     "done_when" "one bar too many")]
                 (str/includes? (:err told) "reserve limit is 32"))))
    ;; UNRESERVED FALLBACK: a lane with no reservation records at a visibly
    ;; lower tier instead of losing the observation.
    (let [unreserved-thread "@thread-unreserved-fallback"
          long-bar (str "Probe: run the suite. Expected: exit 0. "
                        (apply str (repeat 600 "d")))]
      (north.coord/append! port unreserved-thread "title" "Unreserved fallback")
      (north.coord/append! port unreserved-thread "done_when" long-bar)
      (let [recorded (evidence-cli port ["record" "--thread"
                                         (subs unreserved-thread 1)
                                         long-bar "24/24, exit 0"])
            acknowledgement (json/parse-string (:out recorded))
            facts (facts-of port unreserved-thread)
            corrected (evidence-cli port ["record" "--thread"
                                          (subs unreserved-thread 1)
                                          long-bar "corrected: 24/24, exit 0"])
            after (facts-of port unreserved-thread)
            unknown-bar (evidence-cli port ["record" "--thread"
                                            (subs unreserved-thread 1)
                                            "never a bar on this thread" "exit 0"])
            no-thread (evidence-cli port ["record" long-bar "exit 0"])]
        (check "an unreserved lane records instead of erroring"
               (and (zero? (:exit recorded))
                    (= "unreserved" (get acknowledgement "scope"))
                    (= unreserved-thread (get acknowledgement "thread"))
                    (= 1 (count (get facts "bar_evidence_unreserved" #{})))))
        (check "unreserved evidence is bar-length tolerant beyond the reserve limit"
               (and (> (north.terminal-projection/utf8-byte-count long-bar)
                       north.terminal-projection/max-delivery-bar-utf8-bytes)
                    (<= (north.terminal-projection/utf8-byte-count long-bar)
                        north.terminal-projection/max-unreserved-bar-utf8-bytes)))
        (check "unreserved evidence never masquerades as run-bound verification"
               (and (empty? (get facts "bar_evidence" #{}))
                    (empty? (get facts "run_bar_evidence" #{}))
                    (every? #(str/starts-with?
                              %
                              north.terminal-projection/unreserved-bar-evidence-marker)
                            (get facts "bar_evidence_unreserved" #{}))
                    (not (north.terminal-projection/evidence-reports-bar?
                          long-bar
                          (first (get facts "bar_evidence_unreserved" #{}))))
                    (str/includes? (:err recorded) "UNRESERVED")))
        (check "an unreserved re-record supersedes its own stale line"
               (and (zero? (:exit corrected))
                    (= 1 (count (get after "bar_evidence_unreserved" #{})))
                    (str/includes? (first (get after "bar_evidence_unreserved" #{}))
                                   "corrected: 24/24, exit 0")))
        (check "unreserved evidence still requires an active done_when"
               (and (not (zero? (:exit unknown-bar)))
                    (str/includes? (:err unknown-bar) "active bars")))
        (check "no reservation and no thread names the exact recovery command"
               (and (not (zero? (:exit no-thread)))
                    (str/includes? (:err no-thread) "--thread")))))
    ;; BRACES: JSON-shaped observations are ordinary content inside the byte and
    ;; control-character rules, on every writer surface.
    (let [brace-thread "@thread-brace-content"
          brace-run "@run-brace-content"
          brace-capability (apply str (repeat 64 "4"))
          brace-bar "Probe: bun test. Expected: {\"pass\": true}"
          brace-observed "{\"exit\":0,\"failures\":[]} · {nested {braces}}"]
      (north.coord/append! port brace-thread "title" "Brace content")
      (north.coord/append! port brace-thread "done_when" brace-bar)
      (let [reserved (shell "bb" evidence-writer (str port) "reserve"
                            (json/generate-string
                             (reserve-request brace-run brace-thread reporter
                                              brace-capability)))
            recorded (shell "bb" evidence-writer (str port) "record"
                            (json/generate-string
                             (record-request brace-run brace-thread reporter
                                             brace-capability brace-bar
                                             brace-observed)))
            record (json/parse-string (:out recorded))]
        (check "braces are legal content in bars and observations"
               (and (zero? (:exit reserved)) (zero? (:exit recorded))
                    (= brace-bar (get record "bar"))
                    (= brace-observed (get record "observed"))
                    (= #{(str brace-bar " → " brace-observed)}
                       (get (facts-of port brace-thread) "bar_evidence"))))
        (check "brace text survives canonicalization unchanged"
               (= brace-observed
                  (north.terminal-projection/canonical-evidence-text
                   brace-observed)))))
    ;; Two competing publishers cannot create a valid mixed reservation. Depending
    ;; on scheduling, one wins cleanly or both observe the conflict and fail.
    (let [race-run "@run-reservation-race"
          left (future
                 (shell "bb" evidence-writer (str port) "reserve"
                        (json/generate-string
                         (reserve-request race-run thread reporter
                                          (apply str (repeat 64 "c"))))))
          right (future
                  (shell "bb" evidence-writer (str port) "reserve"
                         (json/generate-string
                          (reserve-request race-run thread "@agent:other-lane"
                                           (apply str (repeat 64 "d"))))))
          results [@left @right]
          successes (count (filter #(zero? (:exit %)) results))
          stored (facts-of port race-run)]
      (check "competing reservation publishers cannot both succeed" (<= successes 1))
      (check "a winning competing reservation is exact; a collision is invalid"
             (if (= successes 1)
               (north.terminal-projection/run-reservation-valid? stored)
               (not (north.terminal-projection/run-reservation-valid? stored)))))
    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks]
        (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed"
                         (- (count @checks) (count failed))
                         (count @checks)))
        (when (seq failed) (System/exit 1))))))
