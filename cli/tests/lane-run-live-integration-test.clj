#!/usr/bin/env bb
(require '[babashka.process :as process]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram (or (System/getenv "FRAM_HOME")
              (str (System/getProperty "user.home") "/code/fram/main")))
(def forbidden-ports #{7977 7978 17977 17978 27977 27978})
(def checks (atom []))

(defn check! [label pass?]
  (swap! checks conj [label (boolean pass?)]))

(defn free-port []
  (loop []
    (let [port (with-open [socket (java.net.ServerSocket. 0)]
                 (.getLocalPort socket))]
      (if (contains? forbidden-ports port) (recur) port))))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))

(defn eventually [predicate]
  (loop [attempt 0]
    (cond
      (predicate) true
      (>= attempt 200) false
      :else (do (Thread/sleep 25) (recur (inc attempt))))))

(defn run! [environment & arguments]
  (apply process/shell
         {:out :string :err :string :continue true :extra-env environment}
         "env" "-u" "FRAM_TELEMETRY_LOG" arguments))

(defn scan [environment slot0 slot1 slot2]
  (let [result (run! environment (str fram "/bin/fram") "scan" slot0 slot1 slot2)]
    (when-not (zero? (:exit result))
      (throw (ex-info "FRAMRPC scan failed" {:result result})))
    (mapv edn/read-string (remove str/blank? (str/split-lines (:out result))))))

(let [port (free-port)
      scratch (.toFile (java.nio.file.Files/createTempDirectory
                        "north-lane-run-live-"
                        (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file scratch "history.framlog"))
      space (str "north-lane-run-live-" (java.util.UUID/randomUUID))
      environment {"FRAM_HOME" fram
                   "FRAM_OUT" (str fram "/out")
                   "FRAM_PORT" (str port)
                   "NORTH_PORT" (str port)
                   "FRAM_SPACE_ID" space
                   "AGENT_ID" "dispatcher-fixture"}
      daemon
      (process/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_SPACE_ID" space}}
       "env" "-u" "FRAM_TELEMETRY_LOG"
       "clojure" "-M" "coord_daemon.clj" "serve" (str port) log space)]
  (try
    (check! "current-source coordinator starts via clojure -M"
            (eventually #(port-open? port)))
    (let [thread-write
          (run! environment (str fram "/bin/fram")
                "tell" "thread-task21" "title" "\"Fixture thread\"")]
      (check! "fixture thread is published through FRAMRPC"
              (zero? (:exit thread-write))))
    (let [started
          (run! environment (str root "/bin/north-lane-run")
                "start" "--thread" "thread-task21" "--arm" "graph"
                "--provider" "codex" "--account" "fixture"
                "--model" "fixture-model" "--task" "managed-style simulation"
                "--est-tokens" "240" "--est-wall-min" "7")
          run (str/trim (:out started))
          finished
          (run! environment (str root "/bin/north-lane-run")
                "finish" run "--outcome" "landed" "--retries" "0"
                "--tokens-in" "180" "--tokens-out" "40")
          run-subject (str "@" run)
          run-facts (scan environment run-subject "_" "_")
          estimate-links (scan environment "_" "estimate_of" run-subject)
          estimate-subject (ffirst estimate-links)
          estimate-facts (when estimate-subject
                           (scan environment (str estimate-subject) "_" "_"))]
      (check! "recorder start succeeds over the supported protocol"
              (and (zero? (:exit started))
                   (re-matches #"run:[0-9a-f-]{36}" run)))
      (check! "recorder finish succeeds over the supported protocol"
              (zero? (:exit finished)))
      (check! "run retains its ex-ante attributed estimate"
              (and (= 1 (count estimate-links))
                   (some #(= [estimate-subject "estimate_tokens" "240"] %) estimate-facts)
                   (some #(= [estimate-subject "estimate_wall_min" "7"] %) estimate-facts)
                   (some #(= [estimate-subject "estimate_by" "@agent:dispatcher-fixture"] %)
                         estimate-facts)))
      (check! "run retains terminal wall and token reconciliation"
              (and (some #(= [run-subject "run_outcome" "landed"] %) run-facts)
                   (some #(and (= run-subject (first %))
                               (= "run_wall_ms" (second %))
                               (re-matches #"[0-9]+" (str (nth % 2))))
                         run-facts)
                   (some #(= [run-subject "run_token_status" "exact"] %) run-facts)
                   (some #(= [run-subject "run_tokens_in" "180"] %) run-facts)
                   (some #(= [run-subject "run_tokens_out" "40"] %) run-facts))))
    (finally
      (try (process/destroy-tree daemon) (catch Throwable _ nil))
      (doseq [file (reverse (file-seq scratch))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label pass?] results]
    (println (format "  [%s] %s" (if pass? "PASS" "FAIL") label)))
  (println (format "\nlane-run live FRAMRPC: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
