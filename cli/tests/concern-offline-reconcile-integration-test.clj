#!/usr/bin/env bb
;; Offline concern reconciliation is a finite, exact-log recovery boundary.
(require '[babashka.process :as p]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def subject-root
  (.getCanonicalPath
   (io/file (or (System/getenv "NORTH_TEST_SUBJECT_ROOT") test-root))))
(def fram-root
  (or (System/getenv "NORTH_TEST_FRAM_ROOT")
      (let [home (System/getProperty "user.home")
            current (io/file home "code/north-data/fram-runtime/current")]
        (if (.isDirectory current)
          (.getCanonicalPath current)
          (str home "/code/fram/main")))))

(load-file (str test-root "/cli/coord.clj"))
(load-file (str test-root "/cli/concern-spool.clj"))

(def fails (atom 0))
(defn check [label observation]
  (println (str "  " (if observation "PASS" "FAIL") " — " label))
  (when-not observation (swap! fails inc))
  observation)

(defn temp-directory [prefix]
  (.toFile
   (java.nio.file.Files/createTempDirectory
    prefix
    (make-array java.nio.file.attribute.FileAttribute 0))))

(defn delete-tree! [file]
  (doseq [entry (reverse (file-seq file))]
    (io/delete-file entry true)))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn await-port [port process]
  (loop [attempt 0]
    (cond
      (>= attempt 100) false
      (not= ::running (deref process 0 ::running)) false
      :else
      (let [open?
            (try
              (with-open [socket (java.net.Socket.)]
                (.connect
                 socket
                 (java.net.InetSocketAddress. "127.0.0.1" (int port))
                 50)
                true)
              (catch Exception _ false))]
        (if open?
          true
          (do (Thread/sleep 50) (recur (inc attempt))))))))

(defn coordinator-op [port log request]
  (north.coord/send-op-for-log port (.getCanonicalPath log) request))

(defn sample-operation [log concern-id]
  (north.concern-spool/build-operation
   {:operation-id (str (java.util.UUID/randomUUID))
    :concern-id concern-id
    :target-log (.getCanonicalPath log)
    :created-at (str (java.time.Instant/now))
    :facts
    [{:predicate "title" :object "[north] reconcile fixture" :cardinality "single"}
     {:predicate "agent" :object "@offline-fixture" :cardinality "single"}
     {:predicate "driver" :object "@offline-fixture" :cardinality "single"}
     {:predicate "repo" :object "north" :cardinality "single"}
     {:predicate "intent" :object "reconcile fixture" :cardinality "single"}
     {:predicate "touches" :object "cli/reconcile.clj" :cardinality "multi"}
     {:predicate "reached" :object "building" :cardinality "multi"}
     {:predicate "kind" :object "concern" :cardinality "single"}]}))

(defn start-daemon [port log telemetry]
  (p/process
   {:dir fram-root
    :out :string
    :err :string
    :extra-env
    {"FRAM_LOG" (.getCanonicalPath log)
     "FRAM_TELEMETRY_LOG" (.getCanonicalPath telemetry)
     "NORTH_TELEMETRY_PARTITION" "0"
     "FRAM_REQUIRE_LOG_FENCE" "1"}}
   "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
   (str port) (.getCanonicalPath log)))

(defn run-reconcile-cli [root port log spool state]
  (deref
   (p/process
    {:dir root
     :out :string
     :err :string
     :extra-env
     {"NORTH_HOME" root
      "NORTH_BB" "bb"
      "NORTH_PORT" (str port)
      "FRAM_LOG" (.getCanonicalPath log)
      "NORTH_TELEMETRY_PARTITION" "0"
      "NORTH_CONCERN_SPOOL_DIR" (.getCanonicalPath spool)
      "NORTH_CONCERN_RECONCILE_DIR" (.getCanonicalPath state)}}
    (str root "/bin/concern") "reconcile-local")
   5000
   {:exit 124 :out "" :err "timeout"}))

(defn parent-red-probe []
  (let [tmp (temp-directory "north-concern-reconcile-parent-red")
        spool (doto (io/file tmp "spool") .mkdirs)
        state (io/file tmp "state")
        log (doto (io/file tmp "coordination.log") (spit ""))
        telemetry (doto (io/file tmp "telemetry.log") (spit ""))
        port (free-port)
        operation
        (sample-operation
         log
         (str "@concern-" (System/currentTimeMillis) "-a001"))
        daemon (start-daemon port log telemetry)]
    (try
      (check "strict scratch coordinator starts" (await-port port daemon))
      (with-redefs [north.concern-spool/state-directory
                    (fn [] (.toPath spool))]
        (north.concern-spool/publish-operation! operation))
      (let [result (run-reconcile-cli subject-root port log spool state)
            projection
            (coordinator-op
             port log {:op :show :te (:concern-id operation)})]
        (check "one local reconciliation commits the complete pending concern"
               (and (zero? (:exit result))
                    (= (frequencies
                        (mapv (juxt :predicate :object) (:facts operation)))
                       (frequencies (:rows projection))))))
      (finally
        (try (p/destroy-tree daemon) (catch Throwable _ nil))
        (delete-tree! tmp)))))

(when (= "parent-red" (first *command-line-args*))
  (parent-red-probe)
  (System/exit (if (zero? @fails) 0 1)))

(load-file (str test-root "/cli/concern-spool-reconcile.clj"))

(def generous-limits
  {:max-items 128
   :max-bytes (* 8 1024 1024)
   :max-millis 10000})

(def fixture-sequence (atom 0))

(defn fresh-concern-id []
  (let [value (swap! fixture-sequence inc)]
    (str "@concern-"
         (+ (System/currentTimeMillis) value)
         "-"
         (format "%04x" (mod value 65536)))))

(defn fixture-operation
  ([log label]
   (fixture-operation log label nil nil))
  ([log label about]
   (fixture-operation log label about nil))
  ([log label about about-binding-cid]
   (let [concern-id (fresh-concern-id)
         facts
         (vec
          (concat
           [{:predicate "title"
             :object (str "[north] " label)
             :cardinality "single"}
            {:predicate "agent"
             :object "@offline-fixture"
             :cardinality "single"}
            {:predicate "driver"
             :object "@offline-fixture"
             :cardinality "single"}
            {:predicate "repo" :object "north" :cardinality "single"}
            {:predicate "intent" :object label :cardinality "single"}]
           (when about
             [{:predicate "about" :object about :cardinality "single"}])
           [{:predicate "touches"
             :object (str "cli/" label ".clj")
             :cardinality "multi"}
            {:predicate "reached" :object "building" :cardinality "multi"}
            {:predicate "kind" :object "concern" :cardinality "single"}]))]
     (north.concern-spool/build-operation
      {:operation-id (str (java.util.UUID/randomUUID))
       :concern-id concern-id
       :target-log (.getCanonicalPath (io/file log))
       :created-at (str (java.time.Instant/now))
       :about about
       :about-binding-cid about-binding-cid
       :facts facts}))))

(defn reidentify-operation [operation operation-id]
  (north.concern-spool/build-operation
   {:operation-id operation-id
    :concern-id (:concern-id operation)
    :target-log (:target-log operation)
    :created-at (:created-at operation)
    :about (get-in operation [:precondition :about :subject])
    :about-binding-cid
    (get-in operation [:precondition :about :binding-cid])
    :facts
    (mapv #(select-keys % [:predicate :object :cardinality])
          (:facts operation))}))

(defn publish! [spool operation]
  (with-redefs [north.concern-spool/state-directory
                (fn [] (.toPath (io/file spool)))]
    (north.concern-spool/publish-operation! operation)))

(defn run-pass
  ([port spool state]
   (run-pass port spool state generous-limits))
  ([port spool state pass-limits]
   (north.concern-spool-reconcile/reconcile-pass!
    port
    {:spool-directory (.toPath (io/file spool))
     :state-directory (.toPath (io/file state))
     :limits pass-limits})))

(defn operation-files [directory]
  (if-not (.isDirectory (io/file directory))
    []
    (->> (.listFiles (io/file directory))
         (filter #(str/ends-with? (.getName %) ".op.edn"))
         (sort-by #(.getName %))
         vec)))

(defn record-files [directory suffix]
  (if-not (.isDirectory (io/file directory))
    []
    (->> (.listFiles (io/file directory))
         (filter #(str/ends-with? (.getName %) suffix))
         (sort-by #(.getName %))
         vec)))

(defn operation-record [state operation suffix]
  (io/file
   state
   (str (:operation-id operation) suffix)))

(defn settled-record [state operation]
  (operation-record state operation ".settled.edn"))

(defn conflict-record [state operation]
  (operation-record state operation ".conflict.edn"))

(defn read-record [file]
  (edn/read-string (slurp file)))

(defn version [port log]
  (:version (coordinator-op port log {:op :version})))

(defn show [port log operation]
  (coordinator-op
   port log {:op :show :te (:concern-id operation)}))

(defn exact-projection? [operation rows]
  (= (frequencies
      (mapv (juxt :predicate :object) (:facts operation)))
     (frequencies rows)))

(defn assert-operation! [port log operation]
  (coordinator-op
   port
   log
   {:op :assert-batch
    :te (:concern-id operation)
    :facts
    (mapv
     (fn [{:keys [predicate object]}]
       {:p predicate :r object})
     (:facts operation))}))

(defn assert-facts! [port log subject facts]
  (coordinator-op
   port log {:op :assert-batch :te subject :facts facts}))

(defn kind-binding-cid [port log subject]
  (:claim-cid
   (coordinator-op
    port log {:op :claim-read :te subject :p "kind"})))

(defn start-blackhole [port]
  (let [server (java.net.ServerSocket. port)
        sockets (atom [])
        acceptor
        (future
          (try
            (loop []
              (let [socket (.accept server)]
                (swap! sockets conj socket)
                (future
                  (try
                    (.read (.getInputStream socket))
                    (Thread/sleep 5000)
                    (catch Throwable _ nil)))
                (recur)))
            (catch Throwable _ nil)))]
    {:server server :sockets sockets :acceptor acceptor}))

(defn stop-blackhole! [{:keys [server sockets acceptor]}]
  (try (.close server) (catch Throwable _ nil))
  (doseq [socket @sockets]
    (try (.close socket) (catch Throwable _ nil)))
  (future-cancel acceptor))

(defn crash-restart-probe!
  [port log spool state stage label]
  (let [operation (fixture-operation log label)
        _ (publish! spool operation)
        version-before (version port log)
        injected
        (binding
         [north.concern-spool-reconcile/*reconcile-stage!*
          (fn [observed-stage _]
            (when (= stage observed-stage)
              (throw
               (ex-info
                (str "injected " (name stage) " crash")
                {:stage stage}))))]
          (run-pass port spool state))
        version-after-crash (version port log)
        no-record?
        (and (not (.exists (settled-record state operation)))
             (not (.exists (conflict-record state operation))))
        pending?
        (.isFile
         (io/file spool (str (:operation-id operation) ".op.edn")))
        restarted (run-pass port spool state)
        version-after-restart (version port log)
        replayed (run-pass port spool state)
        version-after-replay (version port log)
        projection (:rows (show port log operation))]
    (check (str label " crash retains the pending operation without a marker")
           (and (= 1 (:deferred injected))
                no-record?
                pending?))
    (check (str label " restart converges to one exact settled projection")
           (and (= 1 (:settled restarted))
                (.isFile (settled-record state operation))
                (not (.exists (conflict-record state operation)))
                (not (.isFile
                      (io/file spool
                               (str (:operation-id operation) ".op.edn"))))
                (exact-projection? operation projection)))
    (check (str label " replay cannot duplicate the recovered commit")
           (and (zero? (:processed replayed))
                (= version-after-restart version-after-replay)
                (if (= stage :pre-commit)
                  (and (= version-before version-after-crash)
                       (> version-after-restart version-after-crash))
                  (and (> version-after-crash version-before)
                       (= version-after-crash version-after-restart)))))))

(defn primary-probes []
  (let [tmp (temp-directory "north-concern-reconcile")
        spool (doto (io/file tmp "spool") .mkdirs)
        state (io/file tmp "state")
        log (doto (io/file tmp "coordination.log") (spit ""))
        telemetry (doto (io/file tmp "telemetry.log") (spit ""))
        wrong-log (doto (io/file tmp "wrong.log") (spit "wrong-log-sentinel\n"))
        port (free-port)
        daemon (start-daemon port log telemetry)]
    (try
      (check "focused strict scratch coordinator starts"
             (await-port port daemon))

      ;; A normal restartable operation lands once, records its immutable result,
      ;; and leaves the bounded active queue.
      (let [operation (fixture-operation log "double-reconcile")
            _ (publish! spool operation)
            first-pass (run-pass port spool state)
            first-version (version port log)
            second-pass (run-pass port spool state)
            second-version (version port log)]
        (check "double reconcile commits one complete concern exactly once"
               (and (= 1 (:settled first-pass))
                    (zero? (:conflicts first-pass))
                    (exact-projection?
                     operation (:rows (show port log operation)))
                    (= first-version second-version)
                    (zero? (:processed second-pass))))
        (check "durable settlement retires the active recovery operation"
               (and (.isFile (settled-record state operation))
                    (not (.isFile
                          (io/file
                           spool
                           (str (:operation-id operation) ".op.edn"))))
                    (= "north-concern-reconciliation-committed-v1"
                       (:commit (read-record
                                 (settled-record state operation)))))))

      ;; An identical projection may predate the local receipt when an ack was
      ;; lost. It settles without another coordinator mutation.
      (let [operation (fixture-operation log "identical-preexisting")
            ack (assert-operation! port log operation)
            _ (publish! spool operation)
            before (version port log)
            result (run-pass port spool state)
            after (version port log)
            record (read-record (settled-record state operation))]
        (check "identical preexisting projection settles idempotently"
               (and (true? (:batch ack))
                    (= before after)
                    (= 1 (:settled result))
                    (= "identical-preexisting" (:reason record))
                    (exact-projection?
                     operation (:rows (show port log operation))))))

      ;; Any nonempty non-exact subject gets an immutable conflict. The
      ;; reconciler never tries to fill a partial spine.
      (let [operation (fixture-operation log "differing")
            _ (assert-facts!
               port log (:concern-id operation)
               [{:p "title" :r "existing different title"}])
            _ (publish! spool operation)
            before (version port log)
            result (run-pass port spool state)
            after (version port log)
            record (read-record (conflict-record state operation))]
        (check "differing or partial projection conflicts with zero target mutation"
               (and (= before after)
                    (= 1 (:conflicts result))
                    (= "projection-differs" (:reason record))
                    (= [["title" "existing different title"]]
                       (:rows (show port log operation)))
                    (not (.isFile
                          (io/file
                           spool
                           (str (:operation-id operation) ".op.edn")))))))

      ;; The Phase 1 envelope is cryptographically self-consistent even when a
      ;; hand-built payload violates the fixed concern spine. Phase 2 rejects
      ;; that shape locally instead of discovering it after a write.
      (let [seed (fixture-operation log "invalid-single")
            facts
            (vec
             (concat
              (butlast (:facts seed))
              [{:predicate "title"
                :object "second conflicting title"
                :cardinality "single"}
               (last (:facts seed))]))
            operation
            (north.concern-spool/build-operation
             {:operation-id (str (java.util.UUID/randomUUID))
              :concern-id (:concern-id seed)
              :target-log (.getCanonicalPath log)
              :created-at (str (java.time.Instant/now))
              :facts
              (mapv #(select-keys % [:predicate :object :cardinality])
                    facts)})
            _ (publish! spool operation)
            before (version port log)
            result (run-pass port spool state)
            after (version port log)
            record (read-record (conflict-record state operation))]
        (check "self-consistent invalid cardinality conflicts before target mutation"
               (and (= before after)
                    (= 1 (:conflicts result))
                    (= "invalid-operation" (:reason record))
                    (empty? (:rows (show port log operation))))))

      ;; A ref can be retargeted to another perfectly valid titled thread. The
      ;; immutable kind-fact CID distinguishes that from the original binding.
      (let [original "@thread:original-binding"
            retarget "@thread:retarget-binding"
            _ (assert-facts!
               port log original
               [{:p "title" :r "original valid thread"}
                {:p "kind" :r "thread"}])
            _ (assert-facts!
               port log retarget
               [{:p "title" :r "different valid thread"}
                {:p "kind" :r "thread"}])
            original-cid (kind-binding-cid port log original)
            operation
            (fixture-operation
             log "about-binding" retarget original-cid)
            _ (publish! spool operation)
            before (version port log)
            result (run-pass port spool state)
            after (version port log)
            record (read-record (conflict-record state operation))]
        (check "valid-thread alias retarget conflicts before concern mutation"
               (and (= before after)
                    (= 1 (:conflicts result))
                    (= "about-binding-changed" (:reason record))
                    (= #{"thread"}
                       (set
                        (:values
                         (coordinator-op
                          port log
                          {:op :resolved
                           :te retarget
                           :p "kind"}))))
                    (empty? (:rows (show port log operation))))))

      ;; The target-log fence rejects the nested read before a write is possible.
      (let [operation (fixture-operation wrong-log "wrong-log")
            _ (publish! spool operation)
            served-before (version port log)
            wrong-before (slurp wrong-log)
            result (run-pass port spool state)
            served-after (version port log)
            record (read-record (conflict-record state operation))]
        (check "wrong-log intent becomes an explicit immutable zero-mutation conflict"
               (and (= served-before served-after)
                    (= wrong-before (slurp wrong-log))
                    (= 1 (:conflicts result))
                    (= "wrong-log" (:reason record))
                    (empty?
                     (:rows
                      (coordinator-op
                       port log
                       {:op :show :te (:concern-id operation)}))))))

      ;; Phase 1's publication lock must survive concurrent producers; Phase 2
      ;; consumes their canonical creation order with operation id as tie-break.
      (let [parallel-spool (doto (io/file tmp "parallel-spool") .mkdirs)
            parallel-state (io/file tmp "parallel-state")
            operations
            (vec
             (repeatedly
              16
              #(fixture-operation log (str "parallel-" (swap! fixture-sequence inc)))))
            receipts
            (with-redefs
             [north.concern-spool/state-directory
              (fn [] (.toPath parallel-spool))]
              (->> operations
                   (mapv
                    (fn [operation]
                      (future
                        (north.concern-spool/publish-operation!
                         operation
                         (+ (System/nanoTime) (* 1000000 5000))))))
                   (mapv deref)))
            expected-files
            (->> operations
                 (sort-by (juxt :created-at :operation-id))
                 (map #(str (:operation-id %) ".op.edn"))
                 vec)
            published-complete?
            (and (= 16 (count (operation-files parallel-spool)))
                 (every?
                  #(north.concern-spool/read-operation-file!
                    (io/file
                     parallel-spool
                     (str (:operation-id %) ".op.edn")))
                  receipts))
            result (run-pass port parallel-spool parallel-state)
            observed-files (mapv :file (:outcomes result))]
        (check "16 parallel publishers create 16 complete immutable operations"
               (and (= 16 (count receipts))
                    (= 16 (count (set (map :operation-id receipts))))
                    published-complete?))
        (check "one bounded pass consumes parallel operations in canonical creation order"
               (and (= expected-files observed-files)
                    (= 16 (:settled result))
                    (empty? (operation-files parallel-spool))
                    (every?
                     #(exact-projection? % (:rows (show port log %)))
                     operations))))

      ;; Already-settled names do not starve later work at a one-item boundary.
      (let [item-spool (doto (io/file tmp "item-spool") .mkdirs)
            item-state (io/file tmp "item-state")
            left (fixture-operation log "item-bound-left")
            right (fixture-operation log "item-bound-right")
            _ (publish! item-spool left)
            _ (publish! item-spool right)
            one-item
            {:max-items 1 :max-bytes (* 1024 1024) :max-millis 5000}
            first-pass (run-pass port item-spool item-state one-item)
            second-pass (run-pass port item-spool item-state one-item)]
        (check "durable cursor advances a deterministic one-item boundary"
               (and (= 1 (:processed first-pass))
                    (= 1 (:settled first-pass))
                    (= 1 (:remaining first-pass))
                    (= 1 (:processed second-pass))
                    (= 1 (:settled second-pass))
                    (.isFile (settled-record item-state left))
                    (.isFile (settled-record item-state right)))))

      ;; A marker name is never authority by itself. A corrupt marker is
      ;; surfaced, and the durable cursor lets later work proceed next pass.
      (let [marker-spool (doto (io/file tmp "marker-spool") .mkdirs)
            marker-state (doto (io/file tmp "marker-state") .mkdirs)
            operations
            (->> [(fixture-operation log "corrupt-marker")
                  (fixture-operation log "after-corrupt-marker")]
                 (sort-by (juxt :created-at :operation-id))
                 vec)
            corrupt (first operations)
            later (second operations)
            _ (doseq [operation operations]
                (publish! marker-spool operation))
            _ (spit (conflict-record marker-state corrupt) "{:truncated true}\n")
            one-item
            {:max-items 1 :max-bytes (* 1024 1024) :max-millis 5000}
            first-pass (run-pass port marker-spool marker-state one-item)
            second-pass (run-pass port marker-spool marker-state one-item)]
        (check "corrupt marker cannot suppress operation validation"
               (and (= 1 (:deferred first-pass))
                    (empty? (:rows (show port log corrupt)))))
        (check "permanently deferred marker prefix cannot starve the ordered tail"
               (and (= 1 (:settled second-pass))
                    (exact-projection?
                     later (:rows (show port log later))))))

      ;; An operation larger than the configured byte slice consumes a bounded
      ;; deferred turn and advances the cursor instead of pinning the queue.
      (let [byte-spool (doto (io/file tmp "byte-spool") .mkdirs)
            byte-state (io/file tmp "byte-state")
            big
            (reidentify-operation
             (fixture-operation log (apply str (repeat 5000 "x")))
             "00000000-0000-0000-0000-000000000001")
            small
            (reidentify-operation
             (fixture-operation log "small-after-oversize")
             "ffffffff-ffff-ffff-ffff-fffffffffff2")
            _ (publish! byte-spool big)
            small-receipt (publish! byte-spool small)
            small-bytes
            (.length
             (io/file byte-spool
                      (str (:operation-id small-receipt) ".op.edn")))
            byte-limits
            {:max-items 1
             :max-bytes (inc small-bytes)
             :max-millis 5000}
            first-pass (run-pass port byte-spool byte-state byte-limits)
            second-pass (run-pass port byte-spool byte-state byte-limits)]
        (check "over-byte ordered head is deferred without mutation"
               (and (= 1 (:deferred first-pass))
                    (= "operation-exceeds-remaining-pass-byte-budget"
                       (:reason (first (:outcomes first-pass))))
                    (empty? (:rows (show port log big)))))
        (check "over-byte head cannot starve a later operation that fits"
               (and (= 1 (:settled second-pass))
                    (exact-projection?
                     small (:rows (show port log small))))))

      ;; The lock spans the whole pass. A peer gets a bounded busy result and
      ;; never enters the operation loop.
      (let [lock-spool (doto (io/file tmp "lock-spool") .mkdirs)
            lock-state (io/file tmp "lock-state")
            entered (promise)
            release (promise)
            holder
            (future
              (binding
               [north.concern-spool-reconcile/*reconcile-stage!*
                (fn [stage _]
                  (when (= :lock-acquired stage)
                    (deliver entered true)
                    @release))]
                (run-pass port lock-spool lock-state)))
            acquired? (deref entered 2000 false)
            contender (run-pass port lock-spool lock-state)
            _ (deliver release true)
            holder-result (deref holder 5000 {:status :timeout})]
        (check "exactly one reconciler owns the process lock"
               (and acquired?
                    (= :busy (:status contender))
                    (= :complete (:status holder-result)))))

      ;; A real process death after marker rename cannot run the cursor write or
      ;; directory fsync. Restart validates the operation/marker binding and
      ;; completes that fsync before treating the operation as terminal.
      (let [operation (fixture-operation log "marker-rename-crash")
            _ (publish! spool operation)
            crashed?
            (try
              (binding
               [north.concern-spool-reconcile/*reconcile-stage!*
                (fn [stage context]
                  (when (and (= :record-renamed stage)
                             (= (:operation-id operation)
                                (get-in context
                                        [:record :operation-id])))
                    (throw (Error. "injected marker rename crash"))))]
                (run-pass port spool state))
              false
              (catch Error _ true))
            version-after-crash (version port log)
            marker-visible? (.isFile (settled-record state operation))
            restarted (run-pass port spool state)
            version-after-restart (version port log)]
        (check "post-rename process death leaves a visible complete marker"
               (and crashed? marker-visible?))
        (check "restart validates, re-fsyncs, and skips the bound marker"
               (and (pos? (:already-settled restarted))
                    (empty? (operation-files spool))
                    (= version-after-crash version-after-restart)
                    (exact-projection?
                     operation (:rows (show port log operation))))))

      (crash-restart-probe!
       port log spool state :pre-commit "pre-commit")
      (crash-restart-probe!
       port log spool state :post-commit-pre-ack "post-commit-pre-ack")
      (crash-restart-probe!
       port log spool state :post-readback-pre-settlement
       "post-readback-pre-settlement")

      (finally
        (try (p/destroy-tree daemon) (catch Throwable _ nil))
        (delete-tree! tmp)))))

(defn bounded-transport-probe []
  (let [tmp (temp-directory "north-concern-reconcile-bounded")
        spool (doto (io/file tmp "spool") .mkdirs)
        state (io/file tmp "state")
        log (doto (io/file tmp "coordination.log") (spit ""))
        port (free-port)
        server (start-blackhole port)
        operation (fixture-operation log "bounded-blackhole")]
    (try
      (publish! spool operation)
      (let [started (System/nanoTime)
            result
            (run-pass
             port spool state
             {:max-items 1
              :max-bytes (* 1024 1024)
              :max-millis 120})
            elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
        (check "time bound cuts off an unreadable coordinator without losing intent"
               (and (= 1 (:deferred result))
                    (< elapsed-ms 1000)
                    (.isFile
                     (io/file
                      spool
                      (str (:operation-id operation) ".op.edn")))
                    (not (.exists (settled-record state operation)))
                    (not (.exists (conflict-record state operation))))))
      (finally
        (stop-blackhole! server)
        (delete-tree! tmp)))))

(defn load-concern-cli-prefix! []
  (let [test-script (System/getProperty "babashka.file")
        source-path (str test-root "/cli/concern-cli.clj")
        source-text (slurp source-path)
        main-offset (str/last-index-of source-text "\n(let [[ps verb")]
    (when-not main-offset
      (throw (ex-info "concern CLI main form marker not found" {})))
    (System/setProperty "babashka.file" source-path)
    (load-string (subs source-text 0 main-offset))
    (System/setProperty "babashka.file" test-script)))

(defn retirement-capacity-probe []
  (let [tmp (temp-directory "north-concern-retirement-capacity")
        spool (doto (io/file tmp "spool") .mkdirs)
        log (doto (io/file tmp "coordination.log") (spit ""))
        first-operation (fixture-operation log "capacity-first")
        second-operation (fixture-operation log "capacity-second")
        configured
        {:max-record-bytes (* 64 1024)
         :max-files 1
         :max-total-bytes (* 1024 1024)}]
    (try
      (binding [north.concern-spool/*limits-override* configured]
        (publish! spool first-operation)
        (let [full?
              (try
                (publish! spool second-operation)
                false
                (catch clojure.lang.ExceptionInfo error
                  (= :concern-spool-full (:type (ex-data error)))))]
          (check "active spool capacity remains fail-closed" full?))
        (north.concern-spool/retire-operation!
         (.toPath spool)
         (:operation-id first-operation)
         (:sha256 first-operation)
         (+ (System/nanoTime) (* 1000000 1000)))
        (let [receipt (publish! spool second-operation)]
          (check "terminal retirement immediately restores publication capacity"
                 (and (= (:operation-id second-operation)
                         (:operation-id receipt))
                      (= 1 (count (operation-files spool)))))))
      (finally
        (delete-tree! tmp)))))

(defn terminal-marker-priority-probe []
  (let [tmp (temp-directory "north-concern-terminal-priority")
        spool (doto (io/file tmp "spool") .mkdirs)
        state (doto (io/file tmp "state") .mkdirs)
        log (doto (io/file tmp "coordination.log") (spit ""))
        pending (fixture-operation log "priority-pending")
        _ (Thread/sleep 2)
        terminal (fixture-operation log "priority-terminal")]
    (try
      (publish! spool pending)
      (publish! spool terminal)
      (#'north.concern-spool-reconcile/publish-record!
       (.toPath state)
       (#'north.concern-spool-reconcile/build-record
        {:record-type "settled"
         :operation-id (:operation-id terminal)
         :concern-id (:concern-id terminal)
         :target-log (:target-log terminal)
         :operation-sha256 (:sha256 terminal)
         :reason "fixture-terminal"
         :observed-version 0
         :observed-projection-sha256 ""}))
      (let [result
            (run-pass
             7977 spool state
             {:max-items 1 :max-bytes (* 1024 1024) :max-millis 1000})]
        (check "durable terminal markers reclaim capacity before transport work"
               (and (= 1 (:retired result))
                    (= 1 (:remaining result))
                    (= (str (:operation-id terminal) ".op.edn")
                       (:file (first (:outcomes result))))
                    (.isFile
                     (io/file spool
                              (str (:operation-id pending) ".op.edn")))
                    (not (.exists
                          (io/file spool
                                   (str (:operation-id terminal)
                                        ".op.edn")))))))
      (finally
        (delete-tree! tmp)))))

(defn single-read-snapshot-probe []
  (let [log (doto (io/file "/tmp/north-single-read-snapshot.log") (spit ""))
        declaration (fixture-operation log "single-read-declaration")
        transition
        (north.concern-spool/build-operation
         {:operation-type north.concern-spool/transition-operation-type
          :operation-id "00000000-0000-0000-0000-000000000005"
          :concern-id (:concern-id declaration)
          :target-log (.getCanonicalPath log)
          :created-at "2026-07-31T08:00:03Z"
          :facts
          [{:predicate "reached"
            :object "likely-to-land"
            :cardinality "multi"}]})
        calls (atom [])
        fake-send
        (fn [_port _target-log request]
          (swap! calls conj request)
          (if (= :show (:op request))
            {:version 73 :rows []}
            (throw
             (ex-info "snapshot must not perform a separate version read"
                      {:request request}))))
        [declaration-snapshot transition-snapshot]
        (with-redefs [north.coord/send-op-for-log fake-send]
          [(#'north.concern-spool-reconcile/read-snapshot-at-base
            7977 declaration)
           (#'north.concern-spool-reconcile/transition-snapshot-at-base
            7977 transition)])]
    (check "fenced show supplies the snapshot rows and their exact global base"
           (and (= 73 (:base declaration-snapshot))
                (= 73 (:base transition-snapshot))
                (= [:show :show] (mapv :op @calls))))))

(defn transition-snapshot-fast-path-probe []
  (let [log (io/file "/tmp/north-transition-snapshot-fast-path.log")
        concern "@concern-1785506000003-d004"
        operation
        (north.concern-spool/build-operation
         {:operation-type north.concern-spool/transition-operation-type
          :operation-id "00000000-0000-0000-0000-000000000004"
          :concern-id concern
          :target-log (.getCanonicalPath log)
          :created-at "2026-07-31T08:00:02Z"
          :facts
          [{:predicate "reached"
            :object "likely-to-land"
            :cardinality "multi"}]})
        planner @(resolve 'concern-transition-plan!)
        no-global-query
        (fn [_]
          (throw (ex-info "global concern index must not be queried" {})))
        [missing nonterminal]
        (with-redefs-fn
         {(resolve 'concern-meta-index) no-global-query}
         (fn []
           [(planner 7977 operation {:base 41 :rows []})
            (planner
             7977
             operation
             {:base 42
              :rows
              [["kind" "concern"]
               ["agent" "@offline-fixture"]
               ["repo" "north"]
               ["intent" "fast replay"]
               ["reached" "building"]]})]))]
    (check "missing transition target conflicts from its exact fenced snapshot"
           (and (:local-conflict missing)
                (= "concern-missing-or-invalid" (:reason missing))
                (= 41 (:observed-version missing))))
    (check "nonterminal transition replay needs no whole-board query"
           (= [{:p "reached" :r "likely-to-land"}]
              (:facts nonterminal)))))

(defn terminal-replay-probe []
  (load-concern-cli-prefix!)
  (let [tmp (temp-directory "north-concern-terminal-replay")
        spool (doto (io/file tmp "spool") .mkdirs)
        state (io/file tmp "state")
        log (doto (io/file tmp "coordination.log") (spit ""))
        target-log (.getCanonicalPath log)
        concern "@concern-1785506000000-a001"
        peer "@concern-1785506000001-b002"
        raced "@concern-1785506000002-c003"
        transition
        (north.concern-spool/build-operation
         {:operation-type north.concern-spool/transition-operation-type
          :operation-id "ffffffff-ffff-ffff-ffff-fffffffffff1"
          :concern-id concern
          :target-log target-log
          :created-at "2026-07-31T08:00:00Z"
          :facts
          [{:predicate "reached" :object "landed" :cardinality "multi"}]})
        raced-transition
        (north.concern-spool/build-operation
         {:operation-type north.concern-spool/transition-operation-type
          :operation-id "00000000-0000-0000-0000-000000000002"
          :concern-id raced
          :target-log target-log
          :created-at "2026-07-31T08:00:01Z"
          :facts
          [{:predicate "reached" :object "landed" :cardinality "multi"}]})
        states
        {concern {:id concern :kind "concern" :agent "@agent-a"
                  :about nil :repo "north" :intent "terminal replay"
                  :status "building" :abandoned false
                  :touches #{"cli/shared.clj"}}
         peer {:id peer :kind "concern" :agent "@agent-b"
               :about nil :repo "north" :intent "peer"
               :status "building" :abandoned false
               :touches #{"cli/shared.clj"}}
         raced {:id raced :kind "concern" :agent "@agent-c"
                :about nil :repo "north" :intent "lost race"
                :status "building" :abandoned true
                :touches #{"cli/raced.clj"}}}
        rows
        (atom
         {concern [["kind" "concern"] ["agent" "@agent-a"]
                   ["repo" "north"] ["intent" "terminal replay"]
                   ["touches" "cli/shared.clj"] ["reached" "building"]]
          raced [["kind" "concern"] ["agent" "@agent-c"]
                 ["repo" "north"] ["intent" "lost race"]
                 ["touches" "cli/raced.clj"]
                 ["reached" "building"] ["reached" "abandoned-stale"]]})
        version* (atom 20)
        batches (atom [])
        fake-send
        (fn [_port exact-log request]
          (when-not (= target-log exact-log)
            (throw (ex-info "wrong test log" {:exact-log exact-log})))
          (case (:op request)
            :version {:version @version*}
            :show {:version @version*
                   :rows (vec (get @rows (:te request) []))}
            :assert-batch-at-version
            (if (not= @version* (:base request))
              {:reject :conflict}
              (let [facts (:facts request)
                    subject (:te request)
                    existing (set (get @rows subject []))
                    written
                    (->> facts
                         (mapv (juxt :p :r))
                         (remove existing)
                         vec)
                    idempotent
                    (->> facts
                         (mapv (juxt :p :r))
                         (filter existing)
                         vec)
                    next-version (swap! version* inc)]
                (swap! batches conj {:subject subject :facts facts})
                (swap! rows update subject
                       (fn [current]
                         (vec (distinct (concat current written)))))
                {:ok next-version
                 :written (mapv pr-str written)
                 :idempotent (mapv pr-str idempotent)
                 :batch true}))
            {:error "unexpected fake coordinator operation"}))]
    (try
      (publish! spool transition)
      (publish! spool raced-transition)
      (let [before-version @version*
            result
            (with-redefs-fn
             {#'north.coord/send-op-for-log fake-send
              (resolve 'concern-meta-index) (fn [_] {})
              (resolve 'concern-meta)
              (fn [_ subject] (get states subject))
              (resolve 'many)
              (fn [_ subject predicate]
                (if (= "reached" predicate)
                  (mapv second
                        (filter #(= "reached" (first %))
                                (get @rows subject [])))
                  []))
              (resolve 'overlaps-for)
              (fn [_ subject]
                {:mine (get states subject)
                 :overlaps
                 (if (= concern subject)
                   [{:pair-key
                     "concern-overlap:concern-1785506000000-a001:concern-1785506000001-b002"
                     :source-concerns [concern peer]
                     :left (get states concern)
                     :right (get states peer)
                     :shared ["cli/shared.clj"]
                     :evidence "path"}]
                   [])})
              (resolve 'attention-event-intent-value)
              (fn [_ subject status event]
                (pr-str ["test-intent" subject status (:event-key event)]))}
             (fn []
               (binding
                [north.concern-spool-reconcile/*transition-plan!*
                 @(resolve 'concern-transition-plan!)]
                 (run-pass 7977 spool state))))
            outcome-files (mapv :file (:outcomes result))
            terminal-batch (first @batches)
            conflict (read-record (conflict-record state raced-transition))
            replay-version @version*
            replay
            (with-redefs [north.coord/send-op-for-log fake-send]
              (binding
               [north.concern-spool-reconcile/*transition-plan!*
                @(resolve 'concern-transition-plan!)]
                (run-pass 7977 spool state)))]
        (check "terminal replay processes queued operations in deterministic order"
               (= ["ffffffff-ffff-ffff-ffff-fffffffffff1.op.edn"
                   "00000000-0000-0000-0000-000000000002.op.edn"]
                  outcome-files))
        (check "spooled terminal transition reconciles with its warning intent atomically"
               (and (= 1 (:settled result))
                    (= 1 (:conflicts result))
                    (> @version* before-version)
                    (= concern (:subject terminal-batch))
                    (= ["attention_event_intent" "reached"]
                       (mapv :p (:facts terminal-batch)))
                    (contains? (set (get @rows concern)) ["reached" "landed"])
                    (.isFile (settled-record state transition))))
        (check "a terminal transition that lost its race surfaces an immutable conflict"
               (and (= "transition-overtaken" (:reason conflict))
                    (.isFile (conflict-record state raced-transition))
                    (not (contains? (set (get @rows raced))
                                    ["reached" "landed"]))))
        (check "settled and conflicted transitions leave no active replay work"
               (and (= replay-version @version*)
                    (zero? (:processed replay))
                    (empty? (operation-files spool)))))
      (finally
        (delete-tree! tmp)))))

(when (= "terminal-offline" (first *command-line-args*))
  (retirement-capacity-probe)
  (terminal-marker-priority-probe)
  (single-read-snapshot-probe)
  (terminal-replay-probe)
  (transition-snapshot-fast-path-probe)
  (System/exit (if (zero? @fails) 0 1)))

(primary-probes)
(bounded-transport-probe)
(retirement-capacity-probe)
(terminal-marker-priority-probe)
(single-read-snapshot-probe)
(terminal-replay-probe)
(transition-snapshot-fast-path-probe)

(if (zero? @fails)
  (do
    (println "\nconcern offline reconciliation O2/O3: ALL PASS")
    (System/exit 0))
  (do
    (println (str "\nconcern offline reconciliation O2/O3: "
                  @fails " FAIL"))
    (System/exit 1)))
