#!/usr/bin/env bb
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram "/home/tom/code/fram/wt-core-target-production-5db9b38")
(cp/add-classpath (str root "/out:" fram "/out"))
(def assignment-writer (str root "/cli/learning-assignment-internal.clj"))
(def run-writer (str root "/cli/run-fact-internal.clj"))
(load-file (str root "/cli/coord.clj"))
(alter-var-root #'north.coord/telemetry-partition-enabled?
                (constantly (fn [] false)))
(load-file (str fram "/database.clj"))
(require '[database :as database])

(def test-space "north-coordination")

(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))
(defn free-port [] (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try (with-open [socket (java.net.Socket.)]
         (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
         true)
       (catch Exception _ false)))
(defn eventually [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 600) false
          :else (do (Thread/sleep 25) (recur (inc attempt))))))
(defn shell [port & args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"FRAM_SPACE_ID" test-space
                                 "NORTH_FRAMRPC_HOST" "127.0.0.1"
                                 "NORTH_PORT" (str port)
                                 "NORTH_TELEMETRY_PARTITION" "0"}}
         "bb" "-cp" (str root "/out:" fram "/out") args))
(defn facts-of [port subject]
  (let [rows
        (north.coord/query-rows
         port {:find "learning_assignment_test"
               :rules [{:head {:rel "learning_assignment_test"
                               :args [{:var "p"} {:var "r"}]}
                        :body [{:rel "triple"
                                :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [facts [predicate value]]
              (update facts predicate (fnil conj #{}) value))
            {} rows)))

(defn learning-occurrences [port subject predicates]
  (let [upper (north.coord/cur-ver port)]
    (->> (:events (north.coord/occurrence-window port 0 upper))
         (filter #(and (= :assert (:operation %))
                       (= subject (:subject %))
                       (predicates (:predicate %))))
         count)))

(defn assignment [arm]
  [["learning_assignment_version" "north-learning-assignment:v1"]
   ["learning_policy_version" "north-learning-policy:v1"]
   ["learning_policy_sha256" (apply str (repeat 64 "a"))]
   ["learning_mode" "frozen"]
   ["learning_evidence_mode" "evaluation"]
   ["learning_experiment_id" "exp-fixture"]
   ["learning_episode_id" "episode-fixture"]
   ["learning_task_signature_sha256" (apply str (repeat 64 "b"))]
   ["learning_task_signature_coverage" "exact"]
   ["learning_risk" "p1"]
   ["learning_arm" (if (= arm "control") "control" "explore")]
   ["learning_axis" (if (= arm "control") "control" "prompt")]
   ["learning_arm_id" arm]
   ["learning_propensity" "1.000000000000"]
   ["learning_explore_propensity" "0.000000000000"]
   ["learning_narrowing_reason" (if (= arm "control") "mode:frozen" "explore:prompt:variant")]
   ["learning_baseline_sha256" (apply str (repeat 64 "c"))]
   ["learning_options_sha256" (apply str (repeat 64 "d"))]
   ["learning_assignment_sha256" (if (= arm "control")
                                    (apply str (repeat 64 "e"))
                                    (apply str (repeat 64 "f")))]])

(defn terminal-payload [assignment-facts]
  (vec (concat
        [["kind" "run"] ["thread" "thread-learning"] ["agent" "lane-learning"]
         ["duration_ms" "5"] ["outcome" "ran"] ["process_outcome" "ran"]
         ["delivery_outcome" "unverified"]
         ["delivery_reason" "delivery_bar_evidence_incomplete"]]
        assignment-facts)))

(let [port (free-port)
      temp (.toFile (java.nio.file.Files/createTempDirectory
                     "north-learning-assignment-"
                     (make-array java.nio.file.attribute.FileAttribute 0)))
      log-file (io/file temp "facts.framlog")
      log (.getCanonicalPath log-file)
      server-output (io/file temp "fram-server.log")
      server (do
               (database/create-triple-log! log test-space)
               (proc/process
                {:dir fram :out server-output :err :out
                 :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                             "FRAM_SERVER_QUIET" "1"
                             "FRAM_SERVER_XMX" "1g"}}
                (str fram "/bin/fram-server") "serve" (str port) log test-space))
      run "@run:learning-assignment-fixture"
      omitted-run "@run:learning-assignment-omitted"
      invalid-run "@run:learning-assignment-invalid"
      late-run "@run:learning-assignment-late"
      control (assignment "control")]
  (try
    (check "throwaway current Fram FRAMRPC server starts"
           (eventually #(and (port-open? port)
                             (= test-space
                                (:space-id (north.coord/status port))))))
    (let [publication (shell port assignment-writer (str port) run
                             (json/generate-string control))
          _ (when-not (zero? (:exit publication))
              (throw (ex-info "learning assignment fixture failed"
                              publication)))
          stored (facts-of port run)
          replay (shell port assignment-writer (str port) run
                        (json/generate-string control))
          changed (shell port assignment-writer (str port) run
                         (json/generate-string (assignment "variant")))]
      (check "assignment publishes atomically" (and (zero? (:exit publication))
                                                     (= (count control)
                                                        (count (select-keys stored (map first control))))))
      (check "exact replay is idempotent" (and (zero? (:exit replay))
                                                (true? (get (json/parse-string (:out replay)) "replay"))))
      (check "changed assignment is refused without mutation"
             (and (not (zero? (:exit changed))) (= stored (facts-of port run))))
      (let [learning-predicates (set (map first control))
            occurrences-before
            (learning-occurrences port run learning-predicates)
            terminal (shell port run-writer (str port) run
                            (json/generate-string (terminal-payload control)))
            occurrences-after
            (learning-occurrences port run learning-predicates)]
        (check "terminal publication repeats the exact pre-provider assignment"
               (and (zero? (:exit terminal)) (= #{"run"} (get (facts-of port run) "kind"))))
        (check "terminal publication adds no learning assignment occurrences"
               (= occurrences-before occurrences-after))))
    (let [published (shell port assignment-writer (str port) omitted-run
                           (json/generate-string control))
          terminal (shell port run-writer (str port) omitted-run
                          (json/generate-string (terminal-payload [])))]
      (check "second assignment publishes" (zero? (:exit published)))
      (check "terminal omission is refused before kind=run"
             (and (not (zero? (:exit terminal)))
                  (nil? (get (facts-of port omitted-run) "kind")))))
    (let [invalid (mapv (fn [[predicate value]]
                          [predicate (if (= predicate "learning_axis") "prompt" value)])
                        control)
          publication (shell port assignment-writer (str port) invalid-run
                             (json/generate-string invalid))]
      (check "server-side writer rejects inconsistent control assignment"
             (and (not (zero? (:exit publication)))
                  (empty? (facts-of port invalid-run)))))
    (let [terminal (shell port run-writer (str port) late-run
                          (json/generate-string (terminal-payload control)))]
      (check "terminal writer cannot introduce assignment after execution"
             (and (not (zero? (:exit terminal)))
                  (nil? (get (facts-of port late-run) "kind")))))
    (catch Throwable error
      (binding [*out* *err*]
        (when (.isFile server-output)
          (println (slurp server-output))))
      (throw error))
    (finally
      (proc/destroy-tree server)
      (try @server (catch Exception _ nil))
      (doseq [file (reverse (file-seq temp))] (io/delete-file file true))
      (doseq [[label ok?] @checks]
        (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\nlearning assignment: %d/%d passed"
                         (- (count @checks) (count failed)) (count @checks)))
        (when (seq failed) (System/exit 1))))))
