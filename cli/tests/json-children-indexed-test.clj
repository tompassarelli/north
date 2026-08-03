#!/usr/bin/env bb
;; `north json children` and `north json child-settlement` are the two reads a
;; managed orchestrator turn-end reconciles against, under a 45s and a 5s
;; budget. Both stay functionally correct on a tiny fixture while a whole-corpus
;; :facts request exhausts those budgets on the live graph, so the wire shape
;; itself is the regression contract.
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

(def coordination-log "/tmp/north-json-children-indexed-coordination.log")
(def telemetry-log "/tmp/north-json-children-indexed-telemetry.log")

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn command-for [mode id]
  (case mode
    :children
    ["timeout" "--kill-after=1s" "10s"
     (str root "/bin/north") "json" "children" id]

    :settlement
    ["timeout" "--kill-after=1s" "10s"
     (str root "/bin/north") "json" "child-settlement" id]

    :sdk
    ["timeout" "--kill-after=1s" "20s" "bun" "-e"
     (str "import { settleChildren } from "
          (json/generate-string (str root "/sdk/src/children.ts"))
          "; console.log(JSON.stringify(settleChildren("
          (json/generate-string id)
          ")));")]))

;; The child-settlement projection is deliberately several indexed reads, so the
;; peer must answer a SEQUENCE and record every request in order — a one-shot
;; socket would make the second query look like an unreachable coordinator.
(defn invoke-peer
  [mode id respond extra-env]
  (let [server (java.net.ServerSocket. 0)
        requests (atom [])
        worker
        (future
          (try
            (loop []
              (with-open [socket (.accept server)
                          reader (io/reader (.getInputStream socket))
                          writer (io/writer (.getOutputStream socket))]
                (let [request (edn/read-string (.readLine reader))]
                  (swap! requests conj request)
                  (.write writer (str (pr-str (respond request)) "\n"))
                  (.flush writer)))
              (recur))
            (catch Throwable _ nil)))
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
            "FRAM_LOG" coordination-log
            "NORTH_PORT" port
            "NORTH_TELEMETRY_PARTITION" "0"
            "NORTH_VERB_SLOTS" "0"
            "NORTH_AGENTS_LIB" "1"
            "NORTH_BIN" (str root "/bin/north")
            "NORTH_HOME" root
            "NO_COLOR" "1"
            "WORLD_MANIFEST_PATH" "/tmp/north-json-children-indexed-no-manifest"}
           routed-env)}
         (command-for mode id))]
    (future-cancel worker)
    (.close server)
    {:result result :requests @requests}))

(defn query-body [request]
  (get-in request [:request :query :rules 0 :body]))

(defn ok [version rows] {:ok rows :version version :engine "index"})
(defn rows-response [version rows] {:version version :rows rows})
(defn row-limit [version]
  {:error ["indexed query exceeded its row bound"]
   :code :query-row-limit
   :version version
   :engine "index"})

;; --- json children -----------------------------------------------------------

(let [parent "019fc807-fb95-749b-b620-9873d5495541"
      {:keys [result requests]}
      (invoke-peer :children parent
                   (constantly (ok 11 [["@019fc83d-0000-7000-8000-000000000002"]
                                       ["@019fc83d-0000-7000-8000-000000000001"]
                                       ["@019fc83d-0000-7000-8000-000000000001"]]))
                   {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "json children exits successfully" (zero? (:exit result)))
  (check! "json children issues exactly one indexed request" (= 1 (count requests)))
  (check! "json children fences one indexed part_of reverse lookup"
          (= {:op :for-log
              :expected-log coordination-log
              :request {:op :query
                        :query {:find "north_child"
                                :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
                                         :body [{:rel "triple"
                                                 :args [{:var "subject"} "part_of" (str "@" parent)]}]}]}
                        :query-max-rows 4096
                        :query-max-response-bytes 1048576}}
             (first requests)))
  (check! "json children sorts and uniques into the JSON contract"
          (= ["019fc83d-0000-7000-8000-000000000001"
              "019fc83d-0000-7000-8000-000000000002"]
             parsed)))

(let [{:keys [result]}
      (invoke-peer :children "019fc849-0000-7000-8000-000000000000"
                   (constantly (ok 12 [])) {})]
  (check! "a childless parent is an authoritative empty array"
          (and (zero? (:exit result)) (= [] (json/parse-string (:out result))))))

(let [{:keys [result]}
      (invoke-peer :children "019fc849-0000-7000-8000-000000000001"
                   (constantly (ok 13 [["@a" "extra"]])) {})]
  (check! "a malformed child row refuses rather than answering short"
          (and (= 4 (:exit result))
               (re-find #"json children REFUSED" (:err result))
               (empty? (:out result)))))

(let [{:keys [result]}
      (invoke-peer :children "019fc849-0000-7000-8000-000000000002"
                   (constantly (row-limit 14)) {})]
  (check! "an over-broad parent refuses rather than truncating"
          (and (= 4 (:exit result))
               (re-find #"json children REFUSED" (:err result))
               (empty? (:out result)))))

;; --- json child-settlement ---------------------------------------------------

(def coordinator "lane-msddhsn6-56f4ff73-33e6-4bc3-a0eb-5e8e09dd6914")
(def child "agent:sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f")
(def child-run "run:sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f-3644")

(def child-fact-rows
  [[(str "@" child) "kind" "agent"]
   [(str "@" child) "coordinator" coordinator]
   [(str "@" child) "agent" "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]])

(def run-show-rows
  [["kind" "run"]
   ["agent" "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]
   ["process_outcome" "delivered"]
   ["run_committed" "2026-08-03T16:00:00Z"]])

(defn settlement-responder
  [{:keys [child-facts tagged-runs child-show]}]
  (fn [request]
    (let [inner (:request request)]
      (case (:op inner)
        :show (rows-response 31 (if (= (:te inner) (str "@" child-run))
                                  run-show-rows
                                  (or child-show [])))
        :query (let [body (query-body request)]
                 (if (= "coordinator" (second (:args (first body))))
                   (child-facts request)
                   (tagged-runs request)))
        {:error ["unexpected op"] :code :bad :version 31 :engine "index"}))))

(let [{:keys [result requests]}
      (invoke-peer
       :settlement coordinator
       (settlement-responder
        {:child-facts (constantly (ok 31 (into [[(str "@" coordinator) "kind" "lane"]]
                                               child-fact-rows)))
         :tagged-runs (constantly (ok 31 [[(str "@" child-run) "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]
                                          ["@run:other-lane-9999" "some-other-agent"]]))})
       {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "json child-settlement exits successfully" (zero? (:exit result)))
  (check! "json child-settlement never issues a whole-corpus request"
          (every? #(#{:query :show} (get-in % [:request :op])) requests))
  (check! "the child projection is one indexed coordinator join"
          (= {:op :query
              :query {:find "north_child_fact"
                      :rules [{:head {:rel "north_child_fact"
                                      :args [{:var "subject"} {:var "predicate"} {:var "value"}]}
                               :body [{:rel "triple"
                                       :args [{:var "subject"} "coordinator" coordinator]}
                                      {:rel "triple"
                                       :args [{:var "subject"} {:var "predicate"} {:var "value"}]}]}]}
              :query-max-rows 4096
              :query-max-response-bytes 1048576}
             (:request (first requests))))
  (check! "the run projection is one indexed kind+agent join"
          (= {:find "north_child_run"
              :rules [{:head {:rel "north_child_run" :args [{:var "subject"} {:var "agent"}]}
                       :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]}
                              {:rel "triple" :args [{:var "subject"} "agent" {:var "agent"}]}]}]}
             (get-in (second requests) [:request :query])))
  (check! "run facts come back as exact per-subject reads"
          (= {:op :show :te (str "@" child-run)}
             (:request (last requests))))
  (check! "the envelope keeps the closed north.child-settlement contract"
          (= ["protocol" "version" "coordinator" "children" "runs"] (keys parsed)))
  (check! "non-agent coordinator subjects stay out of the child set"
          (= [child] (distinct (map #(get % "subject") (get parsed "children")))))
  (check! "child rows sort by (subject, predicate, value)"
          (= [["agent" "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]
              ["coordinator" coordinator]
              ["kind" "agent"]]
             (map (juxt #(get % "predicate") #(get % "value")) (get parsed "children"))))
  (check! "only runs tagged to a direct child are projected"
          (= [child-run] (distinct (map #(get % "subject") (get parsed "runs")))))
  (check! "run rows sort by (subject, predicate, value)"
          (= ["agent" "kind" "process_outcome" "run_committed"]
             (map #(get % "predicate") (get parsed "runs")))))

(let [{:keys [result requests]}
      (invoke-peer
       :settlement coordinator
       (settlement-responder
        {:child-facts (fn [request]
                        (if (= 3 (count (get-in request [:request :query :rules 0 :head :args])))
                          (row-limit 32)
                          (ok 32 [[(str "@" child)] [(str "@" coordinator)]])))
         :tagged-runs (constantly (ok 32 []))
         :child-show [["kind" "agent"] ["coordinator" coordinator]]})
       {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "an overflowing join falls back to exact per-subject reads, never a refusal"
          (and (zero? (:exit result))
               (= [{"subject" child "predicate" "coordinator" "value" coordinator}
                   {"subject" child "predicate" "kind" "value" "agent"}]
                  (get parsed "children"))))
  (check! "the fallback re-asks for child subjects only"
          (= {:find "north_child"
              :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
                       :body [{:rel "triple"
                               :args [{:var "subject"} "coordinator" coordinator]}]}]}
             (get-in (second requests) [:request :query]))))

(let [{:keys [result]}
      (invoke-peer :settlement coordinator
                   (settlement-responder
                    {:child-facts (constantly (ok 33 [[(str "@" child) "kind"]]))
                     :tagged-runs (constantly (ok 33 []))})
                   {})]
  (check! "a malformed settlement row refuses rather than settling short"
          (and (= 4 (:exit result))
               (re-find #"json child-settlement REFUSED" (:err result))
               (empty? (:out result)))))

(let [{:keys [result]}
      (invoke-peer :settlement coordinator
                   (settlement-responder
                    {:child-facts (constantly (ok 34 child-fact-rows))
                     :tagged-runs
                     (fn [request]
                       (if (= 2 (count (get-in request [:request :query :rules 0 :head :args])))
                         (row-limit 34)
                         (ok 34 [[(str "@" child-run)]])))})
                   {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "an over-broad tagged-run read falls back per child rather than refusing"
          (and (zero? (:exit result))
               (= [child-run] (distinct (map #(get % "subject") (get parsed "runs")))))))

(let [{:keys [result]}
      (invoke-peer :settlement coordinator (constantly {:garbage true}) {})]
  (check! "an unreachable or malformed coordinator refuses loudly"
          (and (= 4 (:exit result))
               (re-find #"json child-settlement REFUSED" (:err result))
               (empty? (:out result)))))

;; Runs are telemetry-partitioned subjects; the corpus path this replaces read
;; the union of both origins, so the projection must too.
(let [{:keys [result requests]}
      (invoke-peer
       :settlement coordinator
       (settlement-responder
        {:child-facts (constantly (ok 35 child-fact-rows))
         :tagged-runs (constantly (ok 35 []))})
       {"NORTH_TELEMETRY_PARTITION" "1"
        "NORTH_TELEMETRY_PORT" "$SERVER_PORT"
        "FRAM_TELEMETRY_LOG" telemetry-log})]
  (check! "tagged runs are read from both the coordination and telemetry origins"
          (and (zero? (:exit result))
               (= [coordination-log coordination-log telemetry-log]
                  (map :expected-log requests)))))

;; --- SDK end-to-end ----------------------------------------------------------

(let [{:keys [result]}
      (invoke-peer
       :sdk coordinator
       (settlement-responder
        {:child-facts (constantly (ok 36 child-fact-rows))
         :tagged-runs (constantly (ok 36 [[(str "@" child-run) "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]]))})
       {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "SDK settleChildren accepts the indexed envelope inside its deadline"
          (= {"kind" "settled" "children" [(str "@" child)]} parsed)))

(let [failed (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "PASS" "FAIL") label))
  (println (format "json-children-indexed: %d / %d PASS"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (System/exit (if (seq failed) 1 0)))
