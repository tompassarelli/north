#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[store.types :as t])

(def test-root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def store-root
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_HOME")
      "/home/tom/code/beagle/main/store"))
(when-not (.isFile (io/file store-root "bin/beagle-store-server"))
  (throw (ex-info "current Beagle store engine is required" {:store store-root})))
(def writer-path (str test-root "/cli/worktree-allocation-internal.clj"))

;; Load the writer's validators/publication functions without treating its CLI
;; usage error as a test failure. The executable path is still exercised below.
(try
  (binding [*command-line-args* []] (load-file writer-path))
  (catch clojure.lang.ExceptionInfo error
    (when-not (str/starts-with? (.getMessage error) "usage:") (throw error))))

;; The writer loaded the coordination facade, which loaded the STORE RPC client;
;; the transport seam below is the client's own injection point.
(require '[north.store-rpc-client :as rpc])

(def checks (atom []))
(defn check [label result] (swap! checks conj [label (boolean result)]))

;; Failure evidence is diffed between two runs, so every container in the
;; printed record is ordered: facts-of builds its map with zipmap over a set and
;; its values are sets, both of which print in hash order.
(defn diagnostic-child [result]
  (sorted-map :exit (:exit result) :out (:out result) :err (:err result)))
(defn diagnostic-snapshot [snapshot]
  (into (sorted-map)
        (map (fn [[predicate values]] [predicate (vec (sort-by pr-str values))]))
        snapshot))
(defn diagnostic-record! [record]
  (binding [*out* *err* *print-length* nil *print-level* nil]
    (prn (into (sorted-map) record))))

(defn injected-ambiguous-transact! [port actions readback!]
  (let [batch-attempts (atom 0)
        outcome
        (try
          {:value
           (with-redefs [rpc/subject-projection!
                         (fn [_ subject] (readback! subject))]
             (binding [rpc/*round-trip!*
                       (fn [client request]
                         (if (= :rpc/batch (t/rpcrequest-op request))
                           (if (= 1 (swap! batch-attempts inc))
                             (throw (java.net.SocketTimeoutException.
                                     "injected lost batch acknowledgement"))
                             (rpc/transport-round-trip! client request))
                           (rpc/transport-round-trip! client request)))]
               (north.coord/transact! port actions)))}
          (catch Throwable error {:error error}))]
    (assoc outcome :batch-attempts @batch-attempts)))

(defn injected-projection [subject occurrences]
  {:subject subject :served-version 4242 :occurrences occurrences})

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn await-coordinator! [port]
  (loop [attempt 0]
    (let [ready? (try
                   (= :ready (:state (north.coord/status port)))
                   (catch Exception _ false))]
      (cond
        ready? true
        (>= attempt 750) false
        :else (do (Thread/sleep 100) (recur (inc attempt)))))))

(defn registration [nonce suffix]
  (let [subject (str "@worktree-allocation:" nonce)
        run (str "@run:allocation-test-" suffix)
        observed "2026-07-22T00:00:00.000Z"]
    {"version" allocation-version
     "subject" subject
     "repositoryIdentity" (str "north:git-common-dir-sha256:v1:" (apply str (repeat 64 suffix)))
     "gitCommonDir" (str "/tmp/north-allocation-" suffix "/.git")
     "sourceRoot" (str "/tmp/north-allocation-" suffix)
     "repositoryLayout" "standalone"
     "worktree" (str "/tmp/north-allocation-" suffix "-lane")
     "durableRef" (str "refs/heads/lane-allocation-" suffix)
     "baseOid" (apply str (repeat 40 suffix))
     "headOid" (apply str (repeat 40 suffix))
     "run" run
     "agent" (str "@agent:allocation-" suffix)
     "thread" "@019f8a82-3dce-7418-b2c0-fc6184fc79c6"
     "concern" "@concern-1784735694797-a27c"
     "allocationNonce" nonce
     "lease" {"version" 1
              "holder" (str "@agent:allocation-" suffix)
              "issuedAt" observed
              "expiresAt" "2026-07-22T00:30:00.000Z"
              "enforcement" "phase-1-record-only"}
     "providerAuthorityProfile" {"version" 1 "phase" "requested"
                                 "provider" "auto" "target" "unresolved"
                                 "authMode" "unresolved" "profile" "unresolved"}
     "event" {"version" 1
              "id" (str "00000000-0000-4000-8000-00000000000" suffix)
              "type" "registered" "observedAt" observed
              "resourceState" "planned" "headOid" (apply str (repeat 40 suffix))
              "run" run}}))

(defn shell [log & args]
  (apply proc/shell {:out :string :err :string :continue true
                     :extra-env {"BEAGLE_STORE_LOG" (.getCanonicalPath (io/file log))}}
         args))

(let [port (free-port)
      temp (.toFile (java.nio.file.Files/createTempDirectory
                    "north-worktree-allocation-ledger"
                    (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file temp "coordination.storelog")
      daemon (do
               (proc/process
                {:dir store-root :out :string :err :string
                 :extra-env {"BEAGLE_STORE_SERVER_RUNTIME" "jvm-dev"
                             "BEAGLE_STORE_SERVER_QUIET" "1"
                             "BEAGLE_STORE_SERVER_XMX" "1g"}}
                (str store-root "/bin/beagle-store-server") "serve" (str port)
                (.getCanonicalPath log) "north-coordination"))
      first-registration (registration "11111111-1111-4111-8111-111111111111" "1")
      second-registration (registration "22222222-2222-4222-8222-222222222222" "2")
      third-registration (registration "33333333-3333-4333-8333-333333333333" "3")]
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check "throwaway current Beagle Store server is ready" (await-coordinator! port))

    (let [encoded (json/generate-string first-registration)
          committed (shell log "bb" writer-path (str port) "register" encoded)
          replayed (shell log "bb" writer-path (str port) "register" encoded)
          snapshot (facts-of port (get first-registration "subject"))
          allowed (set (concat registration-predicates
                               [marker-predicate "kind"]))]
      (check "atomic registration commits through the executable writer"
             (and (zero? (:exit committed))
                  (= #{"worktree_allocation"} (get snapshot "kind"))
                  (= 1 (count (get snapshot marker-predicate)))
                  (= allowed (set (keys snapshot)))))
      (check "exact replay is idempotent"
             (and (zero? (:exit replayed))
                  (str/includes? (:out replayed) "exact-replay")
                  (= 1 (count (get snapshot "worktree_allocation_event")))))
      (check "registration is content-free and owns every required physical axis"
             (let [flat (pr-str snapshot)]
               (and (= #{(get first-registration "gitCommonDir")}
                       (get snapshot "worktree_git_common_dir"))
                    (= #{(get first-registration "durableRef")}
                       (get snapshot "worktree_durable_ref"))
                    (= #{(get first-registration "run")}
                       (get snapshot "worktree_allocation_run"))
                    (= #{(get first-registration "concern")}
                       (get snapshot "worktree_allocation_concern"))
                    (not (re-find #"(?i)prompt|message|transcript|content" flat))))))

    (let [subject (get first-registration "subject")
          event {"version" 1
                 "id" "44444444-4444-4444-8444-444444444444"
                 "type" "quarantined"
                 "observedAt" "2026-07-22T00:01:00.000Z"
                 "resourceState" "quarantined"
                 "headOid" (apply str (repeat 40 "1"))
                 "run" "@run:allocation-test-1"
                 "error" {"code" "worktree_dirty" "phase" "admission_rollback"}
                 "recovery" {"action" "inspect-and-salvage"
                             "resource" "/tmp/north-allocation-1-lane"
                             "durableRef" "refs/heads/lane-allocation-1"}}
          result (shell log "bb" writer-path (str port) "event" subject
                        (json/generate-string event))
          snapshot (facts-of port subject)]
      (check "queryable quarantine carries exact structured error and recovery"
             (and (zero? (:exit result))
                  (contains? (get snapshot "worktree_allocation_event")
                             (canonical-json event)))))

    (let [left (proc/process {:out :string :err :string
                              :extra-env {"BEAGLE_STORE_LOG" (.getCanonicalPath log)}}
                             "bb" writer-path (str port) "register"
                             (json/generate-string second-registration))
          right (proc/process {:out :string :err :string
                               :extra-env {"BEAGLE_STORE_LOG" (.getCanonicalPath log)}}
                              "bb" writer-path (str port) "register"
                              (json/generate-string third-registration))
          left-result @left
          right-result @right
          ;; Bound unconditionally: inline in the `and` below, short-circuiting
          ;; threw away the very snapshot that says whether a failure lost facts
          ;; or never committed them.
          left-snapshot (facts-of port (get second-registration "subject"))
          right-snapshot (facts-of port (get third-registration "subject"))
          both-committed?
          (and (zero? (:exit left-result)) (zero? (:exit right-result))
               (= #{"worktree_allocation"} (get left-snapshot "kind"))
               (= #{"worktree_allocation"} (get right-snapshot "kind")))]
      (when-not both-committed?
        (diagnostic-record!
         {:check "concurrent allocation registrations both commit without lost facts"
          :left-child (diagnostic-child left-result)
          :left-snapshot (diagnostic-snapshot left-snapshot)
          :left-subject (get second-registration "subject")
          :right-child (diagnostic-child right-result)
          :right-snapshot (diagnostic-snapshot right-snapshot)
          :right-subject (get third-registration "subject")}))
      (check "concurrent allocation registrations both commit without lost facts"
             both-committed?))

    ;; The raced check above reproduces the flake only when the interleaving
    ;; cooperates. This one reproduces its exact disposition every run: the
    ;; reservation batch is the first mutation a registration issues, and its
    ;; acknowledgement is destroyed at the transport seam INSTEAD of being
    ;; delivered, so the write is provably absent while the writer can only
    ;; observe an answer it never received. Proving that absence and replanning
    ;; within the deadline is the whole difference from giving up.
    (let [ambiguous-registration
          (registration "99999999-9999-4999-8999-999999999999" "9")
          subject (get ambiguous-registration "subject")
          desired (registration-facts ambiguous-registration)
          first-batch? (atom true)
          outcome (try
                    (binding [rpc/*round-trip!*
                              (fn [client request]
                                (when (and (= :rpc/batch (t/rpcrequest-op request))
                                           (compare-and-set! first-batch? true false))
                                  (throw (java.net.SocketTimeoutException.
                                          "injected lost batch acknowledgement")))
                                (rpc/transport-round-trip! client request))]
                      (register! port ambiguous-registration))
                    (catch Exception error {:error error}))
          snapshot (facts-of port subject)
          ;; An injection that never fired would make this check pass whether or
          ;; not the ambiguity is handled, so spending it is part of the claim.
          replanned? (and (false? @first-batch?)
                          (:ok outcome)
                          (= "committed" (:result outcome))
                          (committed-registration? snapshot desired
                                                   (:manifest outcome)))]
      (when-not replanned?
        (diagnostic-record!
         {:check "sent-ambiguous registration batch is proven absent and replanned"
          :injection-spent? (false? @first-batch?)
          :outcome (if-let [error (:error outcome)]
                     (sorted-map :data (pr-str (ex-data error))
                                 :message (ex-message error))
                     (into (sorted-map) outcome))
          :snapshot (diagnostic-snapshot snapshot)
          :subject subject}))
      (check "sent-ambiguous registration batch is proven absent and replanned"
             replanned?))

    ;; Exercise every disposition of transact!'s exact-subject resolver without
    ;; a scheduler or a second writer. Each case loses the first batch answer;
    ;; the injected projection is therefore the only evidence allowed to decide
    ;; whether the same request committed, may be resent, or must stop.
    (let [actions-for
          (fn [subject]
            [{:op :assert :subject subject :predicate "kind"
              :value "ambiguity_resolved"}
             {:op :retract :subject subject :predicate "worktree_head_oid"
              :value "old"}])
          committed-subject "@worktree-allocation:resolver-committed"
          committed
          (injected-ambiguous-transact!
           port (actions-for committed-subject)
           #(injected-projection
             % {"kind" {"ambiguity_resolved" 1}}))
          retry-subject "@worktree-allocation:resolver-retry"
          seeded (north.coord/transact!
                  port [{:op :assert :subject retry-subject
                         :predicate "worktree_head_oid" :value "old"}])
          retried
          (injected-ambiguous-transact!
           port (actions-for retry-subject)
           #(injected-projection
             % {"worktree_head_oid" {"old" 1}}))
          retried-rows (set (north.coord/show-rows port retry-subject))
          mixed
          (injected-ambiguous-transact!
           port (actions-for "@worktree-allocation:resolver-mixed")
           #(injected-projection
             % {"kind" {"ambiguity_resolved" 1}
                "worktree_head_oid" {"old" 1}}))
          readback-error
          (fn [type]
            (injected-ambiguous-transact!
             port (actions-for (str "@worktree-allocation:resolver-" (name type)))
             (fn [_]
               (throw (ex-info "injected resolver readback refusal"
                               {:type type})))))]
      (check "all intended actions observed resolves the ambiguity as committed"
             (and (= 1 (:batch-attempts committed))
                  (= 4242 (get-in committed [:value :ok]))
                  (true? (get-in committed [:value :changed?]))
                  (nil? (:error committed))))
      (check "all action inverses observed proves noncommit and replans successfully"
             (and (:ok seeded)
                  (= 2 (:batch-attempts retried))
                  (:ok (:value retried))
                  (= #{["kind" "ambiguity_resolved"]} retried-rows)))
      (check "mixed intended and inverse actions fail closed as torn-subject"
             (let [data (some-> mixed :error ex-data)]
               (and (= 1 (:batch-attempts mixed))
                    (= :rpc/ambiguous-write (:type data))
                    (= :torn-subject (:resolution data)))))
      (doseq [[label type]
              [["unavailable resolver readback fails closed" :readback-unavailable]
               ["foreign resolver readback fails closed" :foreign-writer]
               ["durability-ambiguous resolver readback fails closed"
                :durability-ambiguous]]]
        (let [result (readback-error type)]
          (check label
                 (and (= 1 (:batch-attempts result))
                      (= type (some-> result :error ex-data :type)))))))

    (let [same-left (registration "66666666-6666-4666-8666-666666666666" "6")
          same-right (registration "77777777-7777-4777-8777-777777777777" "6")
          left (proc/process {:out :string :err :string
                              :extra-env {"BEAGLE_STORE_LOG" (.getCanonicalPath log)}}
                             "bb" writer-path (str port) "register"
                             (json/generate-string same-left))
          right (proc/process {:out :string :err :string
                               :extra-env {"BEAGLE_STORE_LOG" (.getCanonicalPath log)}}
                              "bb" writer-path (str port) "register"
                              (json/generate-string same-right))
          results [@left @right]
          winner (if (zero? (:exit (first results))) same-left same-right)
          loser (if (= winner same-left) same-right same-left)
          reservation-snapshot (facts-of port (reservation-subject winner))]
      (check "same-identity concurrent registration admits one nonce before Git"
             (and (= [0 1] (sort (map :exit results)))
                  (= #{(get winner "allocationNonce")}
                     (get reservation-snapshot "worktree_allocation_nonce"))
                  (= #{"worktree_allocation"}
                     (get (facts-of port (get winner "subject")) "kind"))
                  (every? empty? (vals (facts-of port (get loser "subject")))))))

    (let [failed-registration
          (registration "55555555-5555-4555-8555-555555555555" "5")
          subject (get failed-registration "subject")
          original-atomic-batch north.coord/assert-batch-after-read!
          calls (atom 0)
          rejected
          (try
            (with-redefs [north.coord/assert-batch-after-read!
                          (fn [& args]
                            (if (= 2 (swap! calls inc))
                              {:reject :injected_atomic_batch_refusal}
                              (apply original-atomic-batch args)))]
              (register! port failed-registration))
            nil
            (catch clojure.lang.ExceptionInfo error error))
          remaining (into {} (filter (comp seq val)) (facts-of port subject))
          refused-atomically? (and rejected (empty? remaining))]
      (check "injected atomic registration refusal leaves no unqueryable prefix"
             refused-atomically?))
    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (fs/delete-tree temp)))

  (doseq [[label passed?] @checks]
    (println (str (if passed? "✓ " "✗ ") label)))
  (when-let [failed (seq (remove second @checks))]
    (binding [*out* *err*]
      (println (str "FAILED: " (str/join ", " (map first failed)))))
    (System/exit 1)))
