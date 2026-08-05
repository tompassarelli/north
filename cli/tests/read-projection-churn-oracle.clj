#!/usr/bin/env bb
;; Deterministic reliability oracle for North read projections under write churn.
;;
;; The harness always creates a temporary FRAMLOG and an isolated current Fram
;; FRAMRPC server. It refuses port 7977 at every socket boundary. Defaults exercise
;; three corpus sizes, three writer counts, and five trials per combination.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(import '[java.net ServerSocket]
        '[java.util.concurrent CountDownLatch TimeUnit])

(def protected-ports #{7977 48942})
(def roster-control "oracle-roster")
(def steer-control "oracle-steer")
(def inbox-recipient "oracle-inbox")
(def baseline-run "@run:oracle-roster-baseline")
(def baseline-message "@msg:oracle-inbox-baseline")
(def baseline-concern "@concern-oracle-baseline")
(def test-space "north-coordination")

(def defaults
  {:corpus-sizes [50000 100000 200000]
   :writers [1 4 8]
   :trials 5
   :writes-per-writer 200
   :read-timeout-ms 10000})

(defn usage! [message]
  (when message
    (binding [*out* *err*] (println message)))
  (println "usage: read-projection-churn-oracle.clj [--corpus-sizes N,N] [--writers N,N] [--trials N] [--writes-per-writer N] [--read-timeout-ms N]")
  (System/exit (if message 2 0)))

(defn positive-int [option raw]
  (let [value (parse-long raw)]
    (when-not (and value (pos? value))
      (usage! (str option " requires a positive integer")))
    value))

(defn positive-int-list [option raw]
  (let [values (mapv #(positive-int option %) (str/split raw #"," -1))]
    (when (empty? values)
      (usage! (str option " requires at least one value")))
    (vec (distinct values))))

(defn parse-args [args]
  (loop [remaining args options defaults]
    (if (empty? remaining)
      options
      (let [[option value & more] remaining]
        (when (or (#{"--help" "-h"} option) (nil? value))
          (if (#{"--help" "-h"} option)
            (usage! nil)
            (usage! (str option " requires a value"))))
        (recur
         more
         (case option
           "--corpus-sizes" (assoc options :corpus-sizes
                                   (positive-int-list option value))
           "--writers" (assoc options :writers
                              (positive-int-list option value))
           "--trials" (assoc options :trials (positive-int option value))
           "--writes-per-writer"
           (assoc options :writes-per-writer (positive-int option value))
           "--read-timeout-ms"
           (assoc options :read-timeout-ms (positive-int option value))
           (usage! (str "unknown option " option))))))))

(def options (parse-args *command-line-args*))
(def north-root
  (.getCanonicalPath
   (io/file (.getParent (io/file (or *file*
                                     (System/getProperty "babashka.file"))))
            "../..")))
(def fram-root
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (System/getenv "FRAM_HOME")
      "/home/tom/code/fram/main"))
(def north-out (str north-root "/out"))
(def fram-out (str fram-root "/out"))
(def read-classpath (str north-out java.io.File/pathSeparator fram-out))
(cp/add-classpath read-classpath)
(load-file (str north-root "/cli/coord.clj"))
(alter-var-root #'north.coord/telemetry-partition-enabled?
                (constantly (fn [] false)))
(load-file (str fram-root "/database.clj"))
(require '[database :as database]
         '[fram.store :as store]
         '[fram.types :as t])
(load-file (str north-root "/cli/agent-provenance.clj"))

(when-not (.isFile (io/file fram-root "bin/fram-server"))
  (throw (ex-info "current Fram checkout lacks bin/fram-server"
                  {:fram fram-root})))
(when-not (.isDirectory (io/file north-out))
  (throw (ex-info "North output directory is missing; run ./build.sh"
                  {:out north-out})))

(defn free-high-port []
  (loop []
    (let [port (with-open [server (ServerSocket. 0)]
                 (.getLocalPort server))]
      (if (or (contains? protected-ports port) (< port 10000))
        (recur)
        port))))

(defn require-scratch-port! [port]
  (when (contains? protected-ports port)
    (throw (ex-info "refusing protected coordinator port" {:port port})))
  port)

(defn require-scratch-log! [directory log]
  (let [directory (.getCanonicalPath (io/file directory))
        log (.getCanonicalPath (io/file log))]
    (when-not (str/starts-with? log (str directory java.io.File/separator))
      (throw (ex-info "scratch log escaped its temporary directory"
                      {:directory directory :log log})))
    log))

(defn eventually [f]
  (loop [remaining 600]
    (cond
      (try (f) (catch Throwable _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(defn stop-process! [process]
  (try (proc/destroy-tree process) (catch Throwable _ nil))
  (let [java-process ^Process (:proc process)]
    (when (and java-process
               (not (.waitFor java-process 5 TimeUnit/SECONDS)))
      (.destroyForcibly java-process)
      (.waitFor java-process 5 TimeUnit/SECONDS))))

(defn identity-facts []
  (let [facts
        {"kind" "lane"
         "role" "implementer"
         "goal" "read projection churn oracle fixture"
         "provider" "openai"
         "provider_target" "oracle-subscription"
         "live_input" "streaming"
         "live_input_state" "armed"
         "live_input_epoch" "00000000-0000-4000-8000-000000000001"
         "model" "oracle-model"
         "effort" "high"
         "composition_kind" "preset"
         "composition_id" "implementer"
         "composition_overrides" "[]"
         "repo" north-root
         "spawned_at" "2026-07-25T00:00:00Z"
         "display_handle" "oracle-implementer"
         "display_name" "oracle · implementer"}]
    (assoc facts "identity_manifest_sha256"
           ((ns-resolve 'north.agent-provenance 'manifest-sha256) facts))))

(defn fixture-facts []
  (vec
   (concat
    (map (fn [[predicate value]]
           [(str "@agent:" steer-control) predicate value])
         (sort-by key (identity-facts)))
    [[baseline-run "agent" roster-control]
     [baseline-run "at" "2026-07-25T00:00:00Z"]
     [baseline-run "kind" "run"]
     [baseline-run "outcome" "ran"]
     [baseline-message "to" inbox-recipient]
     [baseline-message "from" "oracle-sender"]
     [baseline-message "subject" "baseline"]
     [baseline-message "body" "projection oracle baseline"]
     [baseline-message "sent_at" "2026-07-25T00:00:00Z"]
     [baseline-concern "kind" "concern"]
     [baseline-concern "repo" north-root]
     [baseline-concern "intent" "projection oracle baseline"]
     [baseline-concern "reached" "building"]
     [baseline-concern "touches" "cli/tests/read-projection-churn-oracle.clj"]])))

(defn seed-log! [log target-facts]
  (let [fixtures (fixture-facts)]
    (when (< target-facts (count fixtures))
      (throw (ex-info "corpus size is smaller than the required fixture"
                      {:requested target-facts :minimum (count fixtures)})))
    (database/create-triple-log! log test-space)
    (let [db (database/open-database! log test-space)
          rows
          (concat
           fixtures
           (map (fn [index]
                  [(format "@oracle:bulk:%09d" index)
                   "oracle_payload" (str "value-" index)])
                (range (- target-facts (count fixtures)))))]
      (doseq [chunk (partition-all 1000 rows)]
        (let [result
              (database/commit!
               db {:operations
                   (mapv (fn [[subject predicate value]]
                           (store/assert-operation
                            (t/triple subject predicate value)))
                         chunk)})]
          (when-not (:ok result)
            (throw (ex-info "FRAMLOG corpus seed failed" result))))))
    {:target target-facts :fixtures (count fixtures)}))

(def inherited-selector-keys ["NORTH_PORT"])

(defn isolated-environment []
  (apply dissoc (into {} (System/getenv)) inherited-selector-keys))

(defn server-env []
  (merge (isolated-environment)
         {"FRAM_SERVER_RUNTIME" "jvm-dev"
          "FRAM_SERVER_QUIET" "1"
          "FRAM_SERVER_XMX" "2g"}))

(defn start-server! [port log server-output]
  (require-scratch-port! port)
  (proc/process
   {:dir fram-root
    :out server-output
    :err :out
    :env (server-env)}
   (str fram-root "/bin/fram-server") "serve" (str port) log test-space))

(defn process-env [port _log]
  (merge
   (server-env)
   {"FRAM_SPACE_ID" test-space
    "NORTH_FRAMRPC_HOST" "127.0.0.1"
    "NORTH_TELEMETRY_PARTITION" "0"
    "NORTH_PORT" (str (require-scratch-port! port))
    "NORTH_HOME" north-root
    "NORTH_AGENTS_LIB" "1"
    "NO_COLOR" "1"
    "AGENT_TOPOLOGY" "orchestrator"
    "NORTH_COORD_CONNECT_TIMEOUT_MS" "1000"
    "NORTH_COORD_READ_TIMEOUT_MS" (str (:read-timeout-ms options))}))

(defn run-bounded [argv port log]
  (let [started (System/nanoTime)
        process (proc/process argv
                              {:dir north-root :out :string :err :string
                               :env (process-env port log)})
        result (deref process (+ (:read-timeout-ms options) 2000) ::timeout)
        elapsed-ms (/ (- (System/nanoTime) started) 1000000.0)]
    (if (= result ::timeout)
      (do
        (stop-process! process)
        {:timeout true :exit nil :out "" :err ""
         :latency-ms elapsed-ms})
      {:timeout false
       :exit (:exit result)
       :out (or (:out result) "")
       :err (or (:err result) "")
       :latency-ms elapsed-ms})))

(defn parse-last-edn [output]
  (some->> (str/split-lines output)
           (remove str/blank?)
           last
           edn/read-string))

(defn result [surface classification process detail]
  {:surface surface
   :classification classification
   :latency-ms (:latency-ms process)
   :detail detail})

(defn failed-process-classification [process]
  (let [message (str/lower-case (str (:out process) "\n" (:err process)))]
    (cond
      (:timeout process) :unavailable
      (or (str/includes? message "unavailable")
          (str/includes? message "timed out")
          (str/includes? message "timeout")
          (str/includes? message "connection"))
      :unavailable
      :else :malformed)))

(defn classify-set [surface process expected baseline observed]
  (let [expected-set (set expected)]
    (cond
      (:timeout process)
      (result surface :unavailable process "subprocess deadline exceeded")

      (not (zero? (:exit process)))
      (result surface (failed-process-classification process) process
              (str/trim (str (:err process) " " (:out process))))

      (not (set/subset? observed expected-set))
      (result surface :malformed process
              (str "unexpected="
                   (pr-str (set/difference observed expected-set))))

      (not (contains? observed baseline))
      (result surface :false-empty process
              (str "immutable anchor absent; observed=" (count observed)))

      (= observed expected-set)
      (result surface :healthy process
              (str "observed all " (count expected) " expected anchors"))

      (= observed (set (take (count observed) expected)))
      (result surface :stale-but-honest process
              (str "complete prior prefix; missing="
                   (pr-str (drop (count observed) expected))))

      :else
      (result surface :malformed process
              (str "non-prefix omission="
                   (pr-str (sort (set/difference expected-set observed))))))))

(defn roster-read [port log expected]
  (let [expression
        (str "(load-file " (pr-str (str north-root "/cli/agents-cli.clj")) ")"
             "(prn (roster-run-entries " (pr-str [roster-control]) "))")
        process (run-bounded ["bb" "-cp" read-classpath "-e" expression] port log)]
    (if (or (:timeout process) (not (zero? (:exit process))))
      (result :roster (failed-process-classification process) process
              (str/trim (str (:err process) " " (:out process))))
      (try
        (let [response (parse-last-edn (:out process))
              projection (get-in response [:by-agent roster-control])]
          (cond
            (not= true (:ok response))
            (result :roster
                    (if (= :run-projection-unavailable (:reason response))
                      :unavailable :malformed)
                    process (pr-str response))

            (map? projection)
            (result :roster
                    (if (= :run-projection-unavailable (:err projection))
                      :unavailable :malformed)
                    process (pr-str projection))

            (not (vector? projection))
            (result :roster :malformed process (pr-str response))

            :else
            (classify-set :roster process expected baseline-run
                          (set (map :subject projection)))))
        (catch Throwable error
          (result :roster :malformed process (.getMessage error)))))))

(defn steer-read [port log trial]
  (let [process
        (run-bounded
         ["bb" "-cp" read-classpath
          (str north-root "/cli/msg-cli.clj") (str port) "send"
          "oracle-harness" steer-control "steer" (str "oracle trial " trial)]
         port log)
        message (str/lower-case (str (:out process) "\n" (:err process)))]
    (cond
      (:timeout process)
      (result :steer :unavailable process "subprocess deadline exceeded")

      (and (zero? (:exit process))
           (str/includes? message "queued for live injection"))
      (result :steer :healthy process "live lane admitted")

      (or (str/includes? message "lifecycle is unavailable")
          (str/includes? message "liveness is unavailable")
          (str/includes? message "timeout")
          (str/includes? message "connection"))
      (result :steer :unavailable process (str/trim message))

      (or (str/includes? message "not one exact committed managed lane")
          (str/includes? message "target is offline"))
      (result :steer :false-empty process (str/trim message))

      :else
      (result :steer :malformed process (str/trim message)))))

(defn inbox-read [port log expected]
  (let [process
        (run-bounded ["bb" "-cp" read-classpath
                      (str north-root "/cli/msg-cli.clj") (str port)
                      "inbox" inbox-recipient]
                     port log)
        observed
        (->> (re-seq #"(?m)^oracle-inbox-[A-Za-z0-9._:-]+"
                     (:out process))
             (map #(str "@msg:" %))
             set)]
    (classify-set :inbox process expected baseline-message observed)))

(defn concern-read [port log expected]
  (let [process
        (run-bounded ["bb" "-cp" read-classpath
                      (str north-root "/cli/concern-cli.clj")
                      (str port) "list-json"]
                     port log)]
    (if (or (:timeout process) (not (zero? (:exit process))))
      (result :concerns (failed-process-classification process) process
              (str/trim (str (:err process) " " (:out process))))
      (try
        (let [document (json/parse-string (:out process) true)
              rows (:concerns document)]
          (if-not (and (= 1 (:version document)) (vector? rows)
                       (every? #(string? (:id %)) rows))
            (result :concerns :malformed process
                    "concern list-json contract mismatch")
            (classify-set :concerns process expected baseline-concern
                          (set (map :id rows)))))
        (catch Throwable error
          (result :concerns :malformed process (.getMessage error)))))))

(defn checked-write! [port actions]
  (let [response (north.coord/transact! port actions)]
    (when-not (integer? (:ok response))
      (throw (ex-info "scratch write was not acknowledged"
                      {:actions actions :response response})))
    response))

(defn generation-facts [corpus writers trial]
  (let [suffix (format "%d-%d-%d" corpus writers trial)
        run (str "@run:oracle-roster-" suffix)
        message (str "@msg:oracle-inbox-" suffix)
        concern (str "@concern-oracle-" suffix)]
    {:run run
     :message message
     :concern concern
     :writes
     [[run [["agent" roster-control]
            ["at" (format "2026-07-25T01:%02d:%02dZ"
                          (mod writers 60) (mod trial 60))]
            ["kind" "run"]
            ["outcome" "ran"]]]
      [message [["from" "oracle-sender"]
                ["subject" (str "trial " suffix)]
                ["body" "projection oracle"]
                ["sent_at" "2026-07-25T00:00:00Z"]
                ["to" inbox-recipient]]]
      [concern [["kind" "concern"]
                ["repo" north-root]
                ["intent" (str "projection oracle " suffix)]
                ["reached" "building"]
                ["touches" "cli/tests/read-projection-churn-oracle.clj"]]]]}))

(defn publish-generation! [port _log generation]
  (doseq [[subject facts] (:writes generation)]
    (checked-write!
     port
     (mapv (fn [[predicate value]]
             {:op :assert :subject subject
              :predicate predicate :value value})
           facts))))

(defn churn-writer [port _log corpus writers trial writer-index start first-acks]
  (future
    (.await start)
    (loop [index 0 acknowledgements 0 failures [] last-write nil]
      (if (= index (:writes-per-writer options))
        {:acks acknowledgements :failures failures :last-write last-write}
        (let [subject
              (format "@oracle:churn:%d:%d:%d:%d:%06d"
                      corpus writers trial writer-index index)
              value (str "write-" writer-index "-" index)
              outcome
              (try
                (let [response (north.coord/append!
                                port subject "oracle_churn" value)]
                  (if (integer? (:ok response))
                    {:ok true}
                    {:ok false :error (pr-str response)}))
                (catch Throwable error
                  {:ok false :error (.getMessage error)}))]
          (when (zero? index) (.countDown first-acks))
          (Thread/sleep 1)
          (recur (inc index)
                 (+ acknowledgements (if (:ok outcome) 1 0))
                 (cond-> failures
                   (not (:ok outcome)) (conj (:error outcome)))
                 (if (:ok outcome) [subject value] last-write)))))))

(defn verify-writer-result [port _log writer-result]
  (let [[subject value] (:last-write writer-result)]
    (and (empty? (:failures writer-result))
         (= (:writes-per-writer options) (:acks writer-result))
         subject
         (contains?
          (set (north.coord/many port subject "oracle_churn"))
          value))))

(def results (atom []))
(def write-checks (atom []))

(defn print-result! [corpus writers trial outcome]
  (swap! results conj (assoc outcome
                             :corpus corpus :writers writers :trial trial))
  (println
   (str/join
    "\t"
    ["TRIAL" corpus writers trial (name (:surface outcome))
     (name (:classification outcome))
     (format "%.1f" (double (:latency-ms outcome)))
     (str/replace (or (:detail outcome) "") #"\s+" " ")])))

(defn run-trial! [port log corpus writers trial expected]
  (let [generation (generation-facts corpus writers trial)
        _ (publish-generation! port log generation)
        expected
        (-> expected
            (update :roster conj (:run generation))
            (update :inbox conj (:message generation))
            (update :concerns conj (:concern generation)))
        start (CountDownLatch. 1)
        first-acks (CountDownLatch. writers)
        writer-jobs
        (mapv #(churn-writer port log corpus writers trial % start first-acks)
              (range writers))]
    (.countDown start)
    (when-not (.await first-acks 10 TimeUnit/SECONDS)
      (throw (ex-info "writers did not begin before the read trial"
                      {:corpus corpus :writers writers :trial trial})))
    (let [reads
          [(future (roster-read port log (:roster expected)))
           (future (steer-read port log trial))
           (future (inbox-read port log (:inbox expected)))
           (future (concern-read port log (:concerns expected)))]
          outcomes (mapv deref reads)
          writer-results (mapv deref writer-jobs)
          healthy-writes?
          (every? #(verify-writer-result port log %) writer-results)]
      (doseq [outcome outcomes]
        (print-result! corpus writers trial outcome))
      (swap! write-checks conj
             {:corpus corpus :writers writers :trial trial
              :expected (* writers (:writes-per-writer options))
              :acked (reduce + (map :acks writer-results))
              :healthy healthy-writes?
              :failures (reduce + (map #(count (:failures %)) writer-results))})
      expected)))

(defn run-corpus! [corpus]
  (let [directory
        (.toFile
         (java.nio.file.Files/createTempDirectory
          (str "north-read-projection-oracle-" corpus "-")
          (make-array java.nio.file.attribute.FileAttribute 0)))
        log (require-scratch-log! directory (io/file directory "facts.framlog"))
        server-output (io/file directory "fram-server.log")
        port (free-high-port)
        seed (seed-log! log corpus)
        server (start-server! port log server-output)]
    (try
      (when-not
       (eventually
        #(let [status (north.coord/status port)]
           (and (= :ready (:state status))
                (= test-space (:space-id status)))))
        (throw (ex-info "scratch current Fram server did not become ready"
                        {:port port :log log :server-log (str server-output)})))
      (let [served-facts (:live-count (north.coord/status port))]
        (println
         (str/join "\t"
                   ["CONFIG" (str "corpus=" corpus)
                    (str "seeded=" (:target seed))
                    (str "served_facts=" served-facts)
                    (str "port=" port)
                    (str "log=" log)])))
      (loop [levels (:writers options)
             expected {:roster [baseline-run]
                       :inbox [baseline-message]
                       :concerns [baseline-concern]}]
        (when-let [writers (first levels)]
          (let [next-expected
                (reduce
                 (fn [current trial]
                   (run-trial! port log corpus writers trial current))
                 expected
                 (range 1 (inc (:trials options))))]
            (recur (rest levels) next-expected))))
      {:corpus corpus :port port :log log :server-log (str server-output)}
      (finally
        (stop-process! server)))))

(def degraded #{:unavailable :false-empty :malformed})

(defn run-oracle! []
  (println
   (str/join
    "\t"
    ["ORACLE"
     (str "corpus_sizes=" (str/join "," (:corpus-sizes options)))
     (str "writers=" (str/join "," (:writers options)))
     (str "trials=" (:trials options))
     (str "writes_per_writer=" (:writes-per-writer options))
     (str "protected_ports_refused="
          (str/join "," (sort protected-ports)))]))
  (println "row\tcorpus\twriters\ttrial\tsurface\tclassification\tlatency_ms\tdetail")
  (let [artifacts (mapv run-corpus! (:corpus-sizes options))
        reproduced?
        (boolean (some #(contains? degraded (:classification %)) @results))
        writes-healthy? (every? :healthy @write-checks)]
    (doseq [surface [:roster :steer :inbox :concerns]
            :let [surface-results (filter #(= surface (:surface %)) @results)
                  counts (frequencies (map :classification surface-results))]]
      (println
       (str/join
        "\t"
        ["SUMMARY" (name surface)
         (str "healthy=" (get counts :healthy 0))
         (str "stale-but-honest=" (get counts :stale-but-honest 0))
         (str "unavailable=" (get counts :unavailable 0))
         (str "false-empty=" (get counts :false-empty 0))
         (str "malformed=" (get counts :malformed 0))])))
    (doseq [{:keys [corpus writers trial expected acked healthy failures]}
            @write-checks]
      (println
       (str/join
        "\t"
        ["WRITE" corpus writers trial
         (str "expected=" expected)
         (str "acked=" acked)
         (str "failures=" failures)
         (str "post_read_resolved=" healthy)])))
    (println
     (str/join
      "\t"
      ["VERDICT"
       (cond
         (not writes-healthy?) "invalid-write-path-unhealthy"
         reproduced? "reproduced"
         :else "cannot-determine")
       (str "attempted_corpus_sizes=" (str/join "," (:corpus-sizes options)))
       (str "attempted_writer_levels=" (str/join "," (:writers options)))
       (str "trials_per_level=" (:trials options))
       (str "scratch_artifacts="
            (str/join "," (map (juxt :port :log :server-log) artifacts)))]))
    (when-not writes-healthy?
      (System/exit 1))))

(when-not (= "1" (System/getenv "NORTH_READ_PROJECTION_ORACLE_LIB"))
  (run-oracle!))
