#!/usr/bin/env bb
(require '[fram.kernel :as kernel])

(def root
  (.getCanonicalPath
   (clojure.java.io/file
    (.getParent (clojure.java.io/file *file*))
    "../..")))

;; partitioned-main resolves coord.clj beside the executable named by this
;; property. Point it at a non-existent peer so loading defines the functions
;; without satisfying the executable entrypoint guard.
(System/setProperty "babashka.file" (str root "/cli/admission-test-loader.clj"))
(load-file (str root "/cli/partitioned-main.clj"))

(def pass (atom 0))
(def fail (atom 0))

(defn check [label condition]
  (if condition
    (do (swap! pass inc) (println "PASS" label))
    (do (swap! fail inc) (println "FAIL" label))))

(defn fact-index [rows]
  (kernel/build-index
   (mapv (fn [[subject predicate value]]
           (kernel/->Fact subject predicate value))
         rows)))

(def thread-facts
  [["@msa-thread" "title" "MSA-999 clock authority"]
   ["@msa-thread" "owner" "msa"]])

(def source-facts
  [["@legacy-open" "owner" "msa"]
   ["@legacy-open" "clocked_by" "user"]
   ["@legacy-open" "start_time" "2026-07-31T08:05:06"]
   ["@legacy-open" "session_of" "@msa-thread"]
   ["@legacy-open" "rate" "100"]
   ["@legacy-open" "kind" "client_session"]])

(def expected-candidate
  {:subject "@legacy-open"
   :thread "@msa-thread"
   :owner "msa"
   :actor "user"
   :start "2026-07-31T08:05:06"
   :rate "100"
   :kind "client_session"})

(defn snapshot [version rows]
  {:version version
   :facts (:facts
           (fram.fold/fold
            (mapv (fn [[tx [subject predicate value]]]
                    (fram.fold/->FactOp tx "assert" subject predicate value "test"))
                  (map-indexed (fn [index row] [(inc index) row]) rows))))})

(defn candidate
  ([telemetry] (candidate thread-facts telemetry))
  ([coordination telemetry]
   (bridge-candidate
    (fact-index coordination)
    (fact-index telemetry)
    "@legacy-open")))

(defn rejected? [coordination telemetry]
  (try
    (candidate coordination telemetry)
    false
    (catch Exception _ true)))

(check "complete exact tuple is admitted"
       (and (bridge-source? (fact-index source-facts) "@legacy-open")
            (= expected-candidate (candidate source-facts))))

(doseq [[label predicate]
        [["missing owner" "owner"]
         ["missing actor" "clocked_by"]
         ["missing start" "start_time"]
         ["missing session_of" "session_of"]
         ["missing rate" "rate"]
         ["missing kind" "kind"]]]
  (check label
         (let [telemetry
               (filterv #(not= predicate (second %)) source-facts)]
           (and (bridge-source? (fact-index telemetry) "@legacy-open")
                (rejected? thread-facts telemetry)))))

(doseq [[label predicate value]
        [["non-user actor" "clocked_by" "agent"]
         ["malformed start" "start_time" "not-an-instant"]
         ["noncanonical start" "start_time" "2026-07-31T08:05"]
         ["impossible start" "start_time" "2026-02-30T08:05:06"]
         ["malformed session_of" "session_of" "msa-thread"]
         ["zero rate" "rate" "0"]
         ["negative rate" "rate" "-1"]
         ["unparseable rate" "rate" "not-a-rate"]
         ["wrong kind" "kind" "session"]]]
  (check label
         (let [telemetry
               (mapv #(if (= predicate (second %))
                        [(first %) (second %) value]
                        %)
                     source-facts)]
           (and (bridge-source? (fact-index telemetry) "@legacy-open")
                (rejected? thread-facts telemetry)))))

(doseq [[label predicate]
        [["duplicate owner" "owner"]
         ["duplicate actor" "clocked_by"]
         ["duplicate start" "start_time"]
         ["duplicate session_of" "session_of"]
         ["duplicate rate" "rate"]
         ["duplicate kind" "kind"]]]
  (let [row (first (filter #(= predicate (second %)) source-facts))]
    (check label
           (let [telemetry (conj source-facts row)]
             (and (bridge-source? (fact-index telemetry) "@legacy-open")
                  (rejected? thread-facts telemetry))))))

(check "telemetry owner cannot retarget the coordination thread"
       (rejected?
        thread-facts
        (mapv #(if (= "owner" (second %))
                 [(first %) (second %) "acme"]
                 %)
              source-facts)))

(check "coordination thread requires one exact owner"
       (rejected?
        (conj thread-facts ["@msa-thread" "owner" "acme"])
        source-facts))

(check "session_of must resolve to a coordination thread"
       (rejected?
        [["@msa-thread" "owner" "msa"]]
        source-facts))

(def target-facts
  (mapv (fn [[_ predicate value]]
          ["@legacy-open" predicate value])
        source-facts))

(let [requests (atom [])
      coord (snapshot 41 thread-facts)
      result
      (with-redefs
       [coordination-snapshot (fn [_ _] coord)
        north.coord/send-op-for-log
        (fn [_ _ request]
          (swap! requests conj request)
          (if (= :version (:op request))
            {:version 41}
            {:ok 42 :batch true}))]
       (publish-bridge! (fact-index source-facts) "@legacy-open"))
      publication (first (filter #(= :assert-batch-at-version (:op %))
                                 @requests))]
  (check "good bridge publishes once" (= :bridged result))
  (check "good bridge publishes the complete tuple"
         (= [{:p "owner" :r "msa"}
             {:p "clocked_by" :r "user"}
             {:p "start_time" :r "2026-07-31T08:05:06"}
             {:p "session_of" :r "@msa-thread"}
             {:p "rate" :r "100"}
             {:p "kind" :r "client_session"}]
            (:facts publication))))

(let [requests (atom [])
      coord (snapshot 42 (into thread-facts target-facts))
      result
      (with-redefs
       [coordination-snapshot (fn [_ _] coord)
        north.coord/send-op-for-log
        (fn [_ _ request]
          (swap! requests conj request)
          (if (= :version (:op request))
            {:version 42}
            (throw (ex-info "unexpected mutation" {:request request}))))]
       (publish-bridge! (fact-index source-facts) "@legacy-open"))]
  (check "exact good bridge is idempotent" (= :already-bridged result))
  (check "idempotent bridge performs no mutation"
         (= [:version] (mapv :op @requests))))

(let [requests (atom [])
      retargeted (snapshot 43
                           [["@msa-thread" "title" "MSA-999 clock authority"]
                            ["@msa-thread" "owner" "acme"]])
      rejected
      (try
        (with-redefs
         [coordination-snapshot (fn [_ _] retargeted)
          north.coord/send-op-for-log
          (fn [_ _ request]
            (swap! requests conj request)
            (if (= :version (:op request))
              {:version 43}
              (throw (ex-info "unexpected mutation" {:request request}))))]
         (publish-bridge! (fact-index source-facts) "@legacy-open"))
        false
        (catch Exception _ true))]
  (check "fenced owner retarget is rejected" rejected)
  (check "owner retarget performs no mutation"
         (= [:version] (mapv :op @requests))))

(let [requests (atom [])
      phase (atom 0)
      before (snapshot 60 thread-facts)
      after (snapshot 61
                      [["@msa-thread" "title" "MSA-999 clock authority"]
                       ["@msa-thread" "owner" "acme"]])
      rejected
      (try
        (with-redefs
         [coordination-snapshot
          (fn [_ _] (if (zero? @phase) before after))
          north.coord/send-op-for-log
          (fn [_ _ request]
            (swap! requests conj request)
            (case (:op request)
              :version {:version (if (zero? @phase) 60 61)}
              :assert-batch-at-version
              (do (swap! phase inc) {:reject :conflict})))]
         (publish-bridge! (fact-index source-facts) "@legacy-open"))
        false
        (catch Exception _ true))
      writes (filterv #(= :assert-batch-at-version (:op %)) @requests)]
  (check "CAS retry revalidates a retargeted coordination owner" rejected)
  (check "retarget after conflict has no accepted second publication"
         (and (= 1 (count writes))
              (= 60 (:base (first writes))))))

(defn publication-rejected-without-mutation?
  [coordination telemetry]
  (let [requests (atom [])
        coord (snapshot 51 coordination)
        rejected
        (try
          (with-redefs
           [coordination-snapshot (fn [_ _] coord)
            north.coord/send-op-for-log
            (fn [_ _ request]
              (swap! requests conj request)
              (if (= :version (:op request))
                {:version 51}
                (throw (ex-info "unexpected mutation"
                                {:request request}))))]
           (publish-bridge! (fact-index telemetry) "@legacy-open"))
          false
          (catch Exception _ true))]
    (and rejected (= [:version] (mapv :op @requests)))))

(doseq [[label coordination telemetry]
        [["missing owner publishes nothing"
          thread-facts
          (filterv #(not= "owner" (second %)) source-facts)]
         ["missing actor publishes nothing"
          thread-facts
          (filterv #(not= "clocked_by" (second %)) source-facts)]
         ["missing start publishes nothing"
          thread-facts
          (filterv #(not= "start_time" (second %)) source-facts)]
         ["missing session_of publishes nothing"
          thread-facts
          (filterv #(not= "session_of" (second %)) source-facts)]
         ["missing rate publishes nothing"
          thread-facts
          (filterv #(not= "rate" (second %)) source-facts)]
         ["missing kind publishes nothing"
          thread-facts
          (filterv #(not= "kind" (second %)) source-facts)]
         ["malformed start publishes nothing"
          thread-facts
          (mapv #(if (= "start_time" (second %))
                   [(first %) (second %) "not-an-instant"]
                   %)
                source-facts)]
         ["malformed session_of publishes nothing"
          thread-facts
          (mapv #(if (= "session_of" (second %))
                   [(first %) (second %) "msa-thread"]
                   %)
                source-facts)]
         ["zero rate publishes nothing"
          thread-facts
          (mapv #(if (= "rate" (second %))
                   [(first %) (second %) "0"]
                   %)
                source-facts)]
         ["unparseable rate publishes nothing"
          thread-facts
          (mapv #(if (= "rate" (second %))
                   [(first %) (second %) "not-a-rate"]
                   %)
                source-facts)]
         ["duplicate tuple value publishes nothing"
          thread-facts
          (conj source-facts ["@legacy-open" "owner" "msa"])]
         ["telemetry owner conflict publishes nothing"
          thread-facts
          (mapv #(if (= "owner" (second %))
                   [(first %) (second %) "acme"]
                   %)
                source-facts)]
         ["coordination owner conflict publishes nothing"
          (conj thread-facts ["@msa-thread" "owner" "acme"])
          source-facts]
         ["unresolved coordination thread publishes nothing"
          [["@msa-thread" "owner" "msa"]]
          source-facts]]]
  (check label
         (publication-rejected-without-mutation?
          coordination telemetry)))

(println)
(println @pass "passed," @fail "failed")
(when (pos? @fail) (System/exit 1))
