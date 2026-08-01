#!/usr/bin/env bb
;; `north json show` must issue one exact :show request. A whole-corpus :facts
;; request can remain functionally correct on tiny fixtures while exhausting
;; the managed-startup acknowledgement budget on the live graph, so the wire
;; shape itself is the regression contract.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file
    (or (System/getenv "NORTH_TEST_ROOT")
        (str (.getParent (io/file (System/getProperty "babashka.file")))
             "/../..")))))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn command-for [mode id]
  (case mode
    :cli
    ["timeout" "--kill-after=1s" "5s"
     (str root "/bin/north") "json" "show" id]

    :plain
    ["timeout" "--kill-after=1s" "5s"
     (str root "/bin/north") "show" id]

    :sdk
    ["timeout" "--kill-after=1s" "5s" "bun" "-e"
     (str "import { getThreadFacts } from "
          (json/generate-string (str root "/sdk/src/north-client.ts"))
          "; console.log(JSON.stringify(getThreadFacts("
          (json/generate-string id)
          ", { timeoutMs: 2000 })));")]

    :admission
    ["timeout" "--kill-after=1s" "10s" "bb" "-e"
     (str "(System/setProperty \"north.agents.lib\" \"1\") "
          "(load-file " (pr-str (str root "/cli/agents-cli.clj")) ") "
          "(prn (select-keys (read-delegate-thread! " (pr-str id) ") "
          "[:id :title :committed?]))")]))

(defn invoke-peer
  [mode id response extra-env]
  (let [server (java.net.ServerSocket. 0)
        request (promise)
        worker
        (future
          (with-open [socket (.accept server)
                      reader (io/reader (.getInputStream socket))
                      writer (io/writer (.getOutputStream socket))]
            (deliver request (edn/read-string (.readLine reader)))
            (.write writer (str (pr-str response) "\n"))
            (.flush writer)))
        fram (.getCanonicalPath (io/file root "../../fram/main"))
        port (str (.getLocalPort server))
        routed-env
        (into {}
              (map (fn [[key value]]
                     [key (if (= "$SERVER_PORT" value) port value)]))
              extra-env)
        result
        (apply
         proc/shell
         {:continue true
          :out :string
          :err :string
          :extra-env
          (merge
           {"FRAM_HOME" fram
            "FRAM_LOG" "/tmp/north-json-show-indexed-coordination.log"
            "NORTH_PORT" port
            "NORTH_TELEMETRY_PARTITION" "0"
            "NORTH_VERB_SLOTS" "0"
            "NORTH_AGENTS_LIB" "1"
            "NORTH_BIN" (str root "/bin/north")
            "NORTH_HOME" root
            "NO_COLOR" "1"
            "WORLD_MANIFEST_PATH" "/tmp/north-json-show-indexed-no-manifest"}
           routed-env)}
         (command-for mode id))
        received (deref request 1000 ::timeout)]
    (.close server)
    (try (deref worker 1000 nil) (catch Throwable _ nil))
    {:result result :request received}))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d834"
      rows [["kind" "thread"] ["title" "managed \"admission\""]]
      {:keys [result request]} (invoke-peer :cli id {:version 17 :rows rows} {})
      parsed (when (zero? (:exit result))
               (json/parse-string (:out result) true))]
  (check! "exact JSON show exits successfully" (zero? (:exit result)))
  (check! "wrapper sends one fenced exact-subject :show request"
          (= {:op :for-log
              :expected-log "/tmp/north-json-show-indexed-coordination.log"
              :request {:op :show :te (str "@" id)}}
             request))
  (check! "exact rows retain the canonical JSON fact contract"
          (= [{:predicate "kind" :value "thread"}
              {:predicate "title" :value "managed \"admission\""}]
             parsed)))

(let [{:keys [result]} (invoke-peer
                        :cli
                        "019fb39e-94a9-7627-adc1-6b4dac07d835"
                        {:version 18 :rows []}
                        {})]
  (check! "authoritative absence remains an empty JSON array"
          (and (zero? (:exit result))
               (= [] (json/parse-string (:out result) true)))))

(let [telemetry-log "/tmp/north-json-show-indexed-telemetry.log"
      {:keys [result request]}
      (invoke-peer
       :cli
       "run:indexed"
       {:version 19 :rows [["kind" "run"]]}
       {"NORTH_TELEMETRY_PARTITION" "1"
        "NORTH_TELEMETRY_PORT" "$SERVER_PORT"
        "FRAM_TELEMETRY_LOG" telemetry-log})]
  (check! "telemetry subjects retain Stage-A routing"
          (and (zero? (:exit result))
               (= {:op :for-log
                   :expected-log telemetry-log
                   :request {:op :show :te "@run:indexed"}}
                  request))))

(let [directory
      (.toFile
       (java.nio.file.Files/createTempDirectory
        "north-subject-read-"
        (make-array java.nio.file.attribute.FileAttribute 0)))
      telemetry-log (.getCanonicalPath (io/file directory "telemetry.fifo"))]
  (try
    (proc/shell "mkfifo" telemetry-log)
    (let [{:keys [result request]}
          (invoke-peer
           :plain
           "run:bounded"
           {:version 20 :rows [["kind" "run"] ["run_task" "bounded read"]]}
           {"NORTH_TELEMETRY_PARTITION" "1"
            "NORTH_TELEMETRY_PORT" "$SERVER_PORT"
            "FRAM_TELEMETRY_LOG" telemetry-log})]
      (check! "plain telemetry show renders exact rows without opening the origin log"
              (and (zero? (:exit result))
                   (= "  kind  run\n  run_task  bounded read\n" (:out result))))
      (check! "plain telemetry show retains the fenced exact-subject request"
              (= {:op :for-log
                  :expected-log telemetry-log
                  :request {:op :show :te "@run:bounded"}}
                 request)))
    (finally
      (io/delete-file telemetry-log true)
      (io/delete-file directory true))))

(let [{:keys [result]}
      (invoke-peer :cli
                   "019fb39e-94a9-7627-adc1-6b4dac07d836"
                   {:version 21 :rows [["malformed"]]}
                   {})]
  (check! "malformed coordinator envelopes fail closed"
          (and (= 4 (:exit result))
               (re-find #"json show REFUSED" (:err result))
               (empty? (:out result)))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d837"
      rows [["kind" "thread"] ["title" "SDK admission"]]
      {:keys [result request]} (invoke-peer :sdk id {:version 22 :rows rows} {})]
  (check! "SDK getThreadFacts consumes the exact wrapper projection"
          (and (zero? (:exit result))
               (= [{:predicate "kind" :value "thread"}
                   {:predicate "title" :value "SDK admission"}]
                  (json/parse-string (:out result) true))))
  (check! "SDK admission emits :show rather than :facts"
          (= :show (get-in request [:request :op]))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d838"
      rows [["title" "Delegate admission"]
            ["kind" "thread"]
            ["committed" "2026-07-30"]
            ["done_when" "probe exits 0"]]
      {:keys [result request]}
      (invoke-peer :admission id {:version 23 :rows rows} {})
      parsed (when (zero? (:exit result))
               (edn/read-string (:out result)))]
  (check! "delegate intake accepts a title-bearing exact projection"
          (and (zero? (:exit result))
               (= {:id id :title "Delegate admission" :committed? true}
                  parsed)))
  (check! "delegate intake reaches the same indexed :show boundary"
          (= {:op :show :te (str "@" id)}
             (:request request))))

(let [failed (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "PASS" "FAIL") label))
  (println (format "json-show-indexed: %d / %d PASS"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (System/exit (if (seq failed) 1 0)))
