#!/usr/bin/env bb
;; `north json children` and `north json child-settlement` are the two reads a
;; managed orchestrator turn-end reconciles against, under a 45s and a 5s
;; budget. Both stay functionally correct on a tiny fixture while a whole-database
;; scan exhausts those budgets on the live graph, so the canonical request shape
;; itself is the regression contract.
(require '[babashka.classpath :as classpath]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file
    (or (System/getenv "NORTH_TEST_ROOT")
        (str (.getParent (io/file (System/getProperty "babashka.file")))
             "/../..")))))

(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
                (System/getenv "BEAGLE_STORE_HOME")
                "/home/tom/code/beagle/main/store"))))
(def coordination-space "north-coordination")
(def telemetry-space "north-telemetry")

(classpath/add-classpath (str fram "/out"))
(require '[store.rpc :as wire]
         '[store.types :as t])

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

(defn read-exact! [input bytes offset length]
  (loop [position offset remaining length]
    (if (zero? remaining)
      true
      (let [read-count (.read input bytes position remaining)]
        (if (neg? read-count)
          false
          (recur (+ position read-count) (- remaining read-count)))))))

(defn read-request-frame! [input]
  (let [header (byte-array wire/rpc-v2-header-bytes)]
    (when-not (read-exact! input header 0 wire/rpc-v2-header-bytes)
      (throw (ex-info "FRAMRPC request ended inside its header"
                      {:type :rpc-truncated})))
    (let [buffer (doto (java.nio.ByteBuffer/wrap header)
                   (.order java.nio.ByteOrder/LITTLE_ENDIAN)
                   (.position 14))
          body-length (Integer/toUnsignedLong (.getInt buffer))]
      (when (> body-length wire/rpc-v2-max-body-bytes)
        (throw (ex-info "FRAMRPC request exceeds the body limit"
                        {:type :rpc-frame-too-large
                         :body-length body-length})))
      (let [body (byte-array (int body-length))
            frame (byte-array (+ wire/rpc-v2-header-bytes (int body-length)))]
        (when-not (read-exact! input body 0 (int body-length))
          (throw (ex-info "FRAMRPC request ended inside its body"
                          {:type :rpc-truncated})))
        (System/arraycopy header 0 frame 0 wire/rpc-v2-header-bytes)
        (System/arraycopy body 0 frame wire/rpc-v2-header-bytes
                          (int body-length))
        (wire/decode-rpc-frame-v2! frame)))))

(defn scan-pattern [request]
  (mapv wire/rpc-option-value!
        (wire/rpc-record-fields!
         (t/rpc-request-payload-value request)
         :rpc/triple-pattern 3)))

(defn query-plan-fields [request]
  (let [[plan _snapshot]
        (wire/rpc-record-fields!
         (t/rpc-request-payload-value request) :query/request 2)]
    (wire/rpc-record-fields! plan :query/plan 4)))

(defn query-find [request]
  (let [[find _strata _order _limit] (query-plan-fields request)
        [relation] (wire/rpc-record-fields! find :query/find-relation 1)]
    relation))

(defn query-head-width [request]
  (let [[_find strata _order _limit] (query-plan-fields request)
        [rules]
        (wire/rpc-record-fields!
         (first (wire/rpc-list-values! strata)) :query/stratum 1)
        [head _clauses]
        (wire/rpc-record-fields!
         (first (wire/rpc-list-values! rules)) :query/rule 2)
        [_relation terms]
        (wire/rpc-record-fields! head :query/head 2)]
    (count (wire/rpc-list-values! terms))))

(defn query-argument! [argument]
  (if (map? argument)
    (wire/rpc-query-variable! (:var argument))
    (wire/rpc-query-constant! argument)))

(defn query-relation! [{:keys [rel args neg]}]
  (wire/rpc-query-relation! rel (mapv query-argument! args) (boolean neg)))

(defn query-rule! [{:keys [head body]}]
  (wire/rpc-query-rule!
   (wire/rpc-query-head! (:rel head) (mapv query-argument! (:args head)))
   (mapv query-relation! body)))

(defn query-request! [{:keys [find rules]}]
  (wire/rpc-query-request!
   (wire/rpc-query-plan!
    (wire/rpc-query-find-relation! find)
    [(wire/rpc-query-stratum! (mapv query-rule! rules))])
   wire/query-current))

(defn page-observation [page]
  (when page
    {:limit (t/rpcpagerequest-limit page)
     :cursor (t/rpc-page-request-cursor-value page)}))

(defn request-observation [request]
  {:space (t/rpcrequest-space request)
   :op (t/rpcrequest-op request)
   :expected-version (t/rpcrequest-expected-version request)
   :page (page-observation (t/rpcrequest-page request))
   :timeout-ms (t/rpcrequest-timeout-ms request)
   :payload (t/rpc-request-payload-value request)})

(defn query-observation [space query]
  {:space space
   :op :rpc/query
   :expected-version nil
   :page {:limit 200 :cursor nil}
   :timeout-ms nil
   :payload (query-request! query)})

(defn exact-query? [request space query]
  (= (query-observation space query) (request-observation request)))

(defn exact-scan? [request space subject]
  (and (= space (t/rpcrequest-space request))
       (= :rpc/scan (t/rpcrequest-op request))
       (= [subject nil nil] (scan-pattern request))))

(defn response-for [frame response]
  (let [request (t/rpcframev2-request frame)
        operation (t/rpcrequest-op request)
        version (or (:version response) 0)
        error
        (cond
          (:row-limit? response)
          (wire/rpc-error! :query-row-limit false
                           "query exceeded its row bound" nil)

          (not (contains? #{:rpc/status :rpc/query :rpc/scan} operation))
          (wire/rpc-error! :rpc/unsupported-operation false
                           "fixture accepts only status, query, and scan" nil))
        page (when (and (nil? error) (contains? #{:rpc/query :rpc/scan} operation))
               (wire/rpc-page-response! 0 nil true))
        payload
        (when-not error
          (case operation
            :rpc/status
            (wire/rpc-status!
             :ready (count (:rows response)) :rpc/jvm
             (wire/rpc-record! :rpc/result-cache [0 0 0 0]))

            :rpc/query
            (if (:malformed? response)
              (wire/rpc-record! :rpc/not-query-rows [])
              (wire/rpc-query-rows!
               (mapv wire/rpc-query-row! (:rows response))))

            :rpc/scan
            (if (:malformed? response)
              (wire/rpc-record! :rpc/not-triples [])
              (let [[subject _predicate _value] (scan-pattern request)]
                (wire/rpc-triples!
                 (mapv (fn [[predicate value]]
                         (t/triple subject predicate value))
                       (:rows response)))))

            wire/rpc-unit))
        typed-response
        (wire/rpc-response!
         (t/rpcrequest-space request) operation version page error payload)]
    (wire/rpc-response-frame (t/rpcframev2-request-id frame) typed-response)))

(defn serve-peer! [server respond requests worker-error]
  (try
    (loop []
      (with-open [socket (.accept server)]
        (let [frame (read-request-frame! (.getInputStream socket))
              request (t/rpcframev2-request frame)
              output (.getOutputStream socket)]
          (swap! requests conj request)
          (.write output
                  (wire/encode-rpc-frame-v2!
                   (response-for frame (respond request))))
          (.flush output)))
      (recur))
    (catch java.net.SocketException _)
    (catch Throwable error
      (reset! worker-error error))))

;; The child-settlement projection is deliberately several bounded reads, so
;; the peer answers a sequence and records every typed request in order.
(defn invoke-peer
  [mode id respond extra-env]
  (let [server (java.net.ServerSocket. 0)
        requests (atom [])
        worker-error (atom nil)
        worker (future (serve-peer! server respond requests worker-error))
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
           {"BEAGLE_STORE_HOME" fram
            "BEAGLE_STORE_BIN" (str fram "/bin")
            "BEAGLE_STORE_OUT" (str fram "/out")
            "BEAGLE_STORE_SERVER_CONNECT" "127.0.0.1"
            "BEAGLE_STORE_SERVER_PORT" port
            "BEAGLE_STORE_SPACE_ID" coordination-space
            "NORTH_FRAMRPC_HOST" "127.0.0.1"
            "NORTH_FRAMRPC_READ_TIMEOUT_MS" "2000"
            "NORTH_PORT" port
            "NORTH_TELEMETRY_SPACE_ID" telemetry-space
            "NORTH_TELEMETRY_PARTITION" "0"
            "NORTH_VERB_SLOTS" "0"
            "NORTH_AGENTS_LIB" "1"
            "NORTH_BIN" (str root "/bin/north")
            "NORTH_HOME" root
            "NO_COLOR" "1"}
           routed-env)}
         (command-for mode id))
        _ (.close server)
        _ (try (deref worker 1000 nil) (catch Throwable _ nil))]
    {:result result :requests @requests :worker-error @worker-error}))

(defn ok [version rows] {:version version :rows rows})
(defn row-limit [version] {:version version :row-limit? true})

(defn children-query [parent]
  {:find "north_child"
   :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "part_of" (str "@" parent)]}]}]})

;; --- json children -----------------------------------------------------------

(let [parent "019fc807-fb95-749b-b620-9873d5495541"
      query (children-query parent)
      {:keys [result requests worker-error]}
      (invoke-peer :children parent
                   (constantly (ok 11 [["@019fc83d-0000-7000-8000-000000000002"]
                                       ["@019fc83d-0000-7000-8000-000000000001"]
                                       ["@019fc83d-0000-7000-8000-000000000001"]]))
                   {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "json children exits successfully" (zero? (:exit result)))
  (check! "json children issues exactly one bounded request" (= 1 (count requests)))
  (check! "json children sends one canonical part_of query"
          (and (nil? worker-error)
               (exact-query? (first requests) coordination-space query)))
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

(defn child-subject-query [coordinator]
  {:find "north_child"
   :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "coordinator" coordinator]}]}]})

(defn child-fact-query [coordinator]
  {:find "north_child_fact"
   :rules [{:head {:rel "north_child_fact"
                   :args [{:var "subject"} {:var "predicate"} {:var "value"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "coordinator" coordinator]}
                   {:rel "triple"
                    :args [{:var "subject"} {:var "predicate"} {:var "value"}]}]}]})

(def tagged-run-query
  {:find "north_child_run"
   :rules [{:head {:rel "north_child_run"
                   :args [{:var "subject"} {:var "agent"}]}
            :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]}
                   {:rel "triple"
                    :args [{:var "subject"} "agent" {:var "agent"}]}]}]})

(defn child-run-query [agent-id]
  {:find "north_child_run"
   :rules [{:head {:rel "north_child_run" :args [{:var "subject"}]}
            :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]}
                   {:rel "triple"
                    :args [{:var "subject"} "agent" agent-id]}]}]})

(defn settlement-responder
  [{:keys [child-facts tagged-runs child-show]}]
  (fn [request]
    (case (t/rpcrequest-op request)
      :rpc/scan
      (ok 31 (if (= (first (scan-pattern request)) (str "@" child-run))
               run-show-rows
               (or child-show [])))

      :rpc/query
      (case (query-find request)
        ("north_child_fact" "north_child") (child-facts request)
        "north_child_run" (tagged-runs request)
        {:version 31 :malformed? true})

      {:version 31 :malformed? true})))

(let [child-query (child-fact-query coordinator)
      {:keys [result requests worker-error]}
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
          (and (nil? worker-error)
               (every? #(contains? #{:rpc/query :rpc/scan}
                                    (t/rpcrequest-op %))
                       requests)))
  (check! "the child projection is one canonical coordinator join"
          (exact-query? (first requests) coordination-space child-query))
  (check! "the run projection is one canonical kind+agent join"
          (exact-query? (second requests) coordination-space tagged-run-query))
  (check! "run facts come back as exact subject scans"
          (exact-scan? (last requests) coordination-space (str "@" child-run)))
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
                        (if (= 3 (query-head-width request))
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
          (exact-query? (second requests) coordination-space
                        (child-subject-query coordinator))))

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
                       (if (= 2 (query-head-width request))
                         (row-limit 34)
                         (ok 34 [[(str "@" child-run)]])))})
                   {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "an over-broad tagged-run read falls back per child rather than refusing"
          (and (zero? (:exit result))
               (= [child-run] (distinct (map #(get % "subject") (get parsed "runs")))))))

(let [{:keys [result]}
      (invoke-peer :settlement coordinator
                   (constantly {:version 31 :malformed? true}) {})]
  (check! "an unreachable or malformed coordinator refuses loudly"
          (and (= 4 (:exit result))
               (re-find #"json child-settlement REFUSED" (:err result))
               (empty? (:out result)))))

;; Runs are telemetry-partitioned subjects, so the projection reads both named
;; spaces and merges their rows.
(let [{:keys [result requests]}
      (invoke-peer
       :settlement coordinator
       (settlement-responder
        {:child-facts (constantly (ok 35 child-fact-rows))
         :tagged-runs (constantly (ok 35 []))})
       {"NORTH_TELEMETRY_PARTITION" "1"
        "NORTH_TELEMETRY_PORT" "$SERVER_PORT"})]
  (check! "tagged runs are read from both canonical SpaceIds"
          (and (zero? (:exit result))
               (= [coordination-space coordination-space telemetry-space]
                  (mapv t/rpcrequest-space requests)))))

;; --- SDK end-to-end ----------------------------------------------------------

(let [{:keys [result]}
      (invoke-peer
       :sdk coordinator
       (settlement-responder
        {:child-facts (constantly (ok 36 child-fact-rows))
         :tagged-runs (constantly (ok 36 [[(str "@" child-run) "sdk-spawn-msddviyc-68c79f08-79b2-443c-b8f8-78c5118f162f"]]))})
       {})
      parsed (when (zero? (:exit result)) (json/parse-string (:out result)))]
  (check! "SDK settleChildren accepts the bounded FRAMRPC envelope inside its deadline"
          (= {"kind" "settled" "children" [(str "@" child)]} parsed)))

(let [failed (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "PASS" "FAIL") label))
  (println (format "json-children-indexed: %d / %d PASS"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (System/exit (if (seq failed) 1 0)))
