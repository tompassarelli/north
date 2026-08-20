#!/usr/bin/env bb
;; Canonical offline concern recovery against one current Beagle Store STORE RPC server.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io])

(def root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def store
  (or (System/getenv "NORTH_TEST_STORE_ROOT")
      (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_PATH")
      "/home/tom/code/beagle/main/store"))
(cp/add-classpath (str root "/out:" store "/out"))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/concern-spool.clj"))
(load-file (str root "/cli/concern-spool-reconcile.clj"))

(def checks (atom []))
(defn check [label value]
  (let [passed (boolean value)]
    (swap! checks conj [label passed])
    (println (str "  " (if passed "PASS" "FAIL") " — " label))))

(defn temp-directory [prefix]
  (.toFile
   (java.nio.file.Files/createTempDirectory
    prefix (make-array java.nio.file.attribute.FileAttribute 0))))

(defn delete-tree! [directory]
  (doseq [entry (reverse (file-seq directory))]
    (io/delete-file entry true)))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn await-ready [port process]
  (loop [attempt 0]
    (let [status (try (north.coord/status port) (catch Throwable _ nil))]
      (cond
        (and (= :ready (:state status))
             (= "north-coordination" (:space-id status))) true
        (not (proc/alive? process)) false
        (>= attempt 800) false
        :else (do (Thread/sleep 25) (recur (inc attempt)))))))

(defn start-store [port log]
  (proc/process
   {:dir store
    :out :string
    :err :string
    :extra-env
    {"BEAGLE_STORE_SERVER_RUNTIME" "jvm-dev"
     "BEAGLE_STORE_SERVER_QUIET" "1"
     "BEAGLE_STORE_SERVER_XMX" "1g"}}
   (str store "/bin/beagle-store-server") "serve" (str port)
   (.getCanonicalPath log) "north-coordination"))

(def operation-sequence (atom 0))
(defn operation [log label]
  (let [sequence (swap! operation-sequence inc)
        operation-id (str (java.util.UUID/randomUUID))
        concern-id
        (str "@concern-" (+ (System/currentTimeMillis) sequence) "-"
             (format "%04x" sequence))]
    (north.concern-spool/build-operation
     {:operation-id operation-id
      :concern-id concern-id
      :target-log (.getCanonicalPath log)
      :created-at (str (java.time.Instant/now))
      :facts
      [{:predicate "title" :object (str "[north] " label)
        :cardinality "single"}
       {:predicate "agent" :object "@offline-fixture"
        :cardinality "single"}
       {:predicate "driver" :object "@offline-fixture"
        :cardinality "single"}
       {:predicate "repo" :object "north" :cardinality "single"}
       {:predicate "intent" :object label :cardinality "single"}
       {:predicate "touches" :object (str "cli/" label ".clj")
        :cardinality "multi"}
       {:predicate "attention_reconcile_pending" :object operation-id
        :cardinality "multi"}
       {:predicate "reached" :object "building" :cardinality "multi"}
       {:predicate "kind" :object "concern" :cardinality "single"}]})))

(defn publish! [spool item]
  (with-redefs [north.concern-spool/state-directory
                (fn [] (.toPath (io/file spool)))]
    (north.concern-spool/publish-operation! item)))

(def generous-limits
  {:max-items 32 :max-bytes (* 2 1024 1024) :max-millis 10000})

(defn run-pass
  ([port log spool state]
   (run-pass port log spool state generous-limits))
  ([port log spool state limits]
   (with-redefs [north.coord/expected-log #(.getCanonicalPath (io/file log))]
     (north.concern-spool-reconcile/reconcile-pass!
      port
      {:spool-directory (.toPath (io/file spool))
       :state-directory (.toPath (io/file state))
       :limits limits}))))

(defn operation-file [spool item]
  (io/file spool (str (:operation-id item) ".op.edn")))
(defn settled-file [state item]
  (io/file state (str (:operation-id item) ".settled.edn")))
(defn conflict-file [state item]
  (io/file state (str (:operation-id item) ".conflict.edn")))

(defn expected-rows [item]
  (frequencies (map (juxt :predicate :object) (:facts item))))
(defn observed-rows [port item]
  (frequencies (:rows (north.coord/show-envelope port (:concern-id item)))))

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

(let [tmp (temp-directory "north-concern-reconcile-store-rpc")
      log (io/file tmp "coordination.storelog")
      spool (io/file tmp "spool")
      state (io/file tmp "state")
      port (free-port)
      daemon (start-store port log)]
  (try
    (let [ready? (await-ready port daemon)]
      (check "current Beagle Store serves the scratch SpaceId over STORE RPC" ready?)
      (when-not ready?
        (throw (ex-info "scratch Beagle Store server failed"
                        {:result (deref daemon 1000 nil)}))))

    (let [item (operation log "settle-once")
          _ (publish! spool item)
          first-pass (run-pass port log spool state)
          first-version (north.coord/cur-ver port)
          replay (run-pass port log spool state)
          replay-version (north.coord/cur-ver port)]
      (check "one pass settles the complete canonical concern projection"
             (and (= 1 (:settled first-pass))
                  (= (expected-rows item) (observed-rows port item))
                  (.isFile (settled-file state item))
                  (not (.exists (operation-file spool item)))))
      (check "settled replay performs no second STORE RPC mutation"
             (and (zero? (:processed replay))
                  (= first-version replay-version))))

    (let [item (operation log "lost-ack")
          actions
          (mapv (fn [{:keys [predicate object]}]
                  {:op :assert :subject (:concern-id item)
                   :predicate predicate :value object})
                (:facts item))
          _ (north.coord/transact! port actions)
          _ (publish! spool item)
          before (north.coord/cur-ver port)
          result (run-pass port log spool state)
          record (edn/read-string (slurp (settled-file state item)))]
      (check "an exact preexisting projection settles without another write"
             (and (= 1 (:settled result))
                  (= before (north.coord/cur-ver port))
                  (= "identical-preexisting" (:reason record)))))

    (let [wrong-log (io/file tmp "wrong.storelog")
          item (operation wrong-log "wrong-log")
          _ (publish! spool item)
          before (north.coord/cur-ver port)
          result (run-pass port log spool state)
          record (edn/read-string (slurp (conflict-file state item)))]
      (check "a different target STORELOG becomes a zero-mutation conflict"
             (and (= 1 (:conflicts result))
                  (= before (north.coord/cur-ver port))
                  (= "wrong-log" (:reason record))
                  (not (.exists (operation-file spool item))))))

    (let [item (operation log "post-commit-restart")
          _ (publish! spool item)
          interrupted
          (binding [north.concern-spool-reconcile/*reconcile-stage!*
                    (fn [stage _]
                      (when (= :post-commit-pre-ack stage)
                        (throw (ex-info "injected lost acknowledgement" {}))))]
            (run-pass port log spool state))
          after-commit (north.coord/cur-ver port)
          restarted (run-pass port log spool state)]
      (check "restart resolves a lost acknowledgement by exact readback"
             (and (= 1 (:deferred interrupted))
                  (= 1 (:settled restarted))
                  (= after-commit (north.coord/cur-ver port))
                  (= (expected-rows item) (observed-rows port item)))))
    (finally
      (try (proc/destroy-tree daemon) (catch Throwable _ nil))
      (delete-tree! tmp))))

(let [tmp (temp-directory "north-concern-reconcile-blackhole")
      log (io/file tmp "coordination.storelog")
      spool (io/file tmp "spool")
      state (io/file tmp "state")
      port (free-port)
      blackhole (start-blackhole port)
      item (operation log "transport-ambiguity")]
  (try
    (publish! spool item)
    (let [started (System/nanoTime)
          result
          (run-pass port log spool state
                    {:max-items 1 :max-bytes (* 1024 1024)
                     :max-millis 120})
          elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
      (let [ok (and (= 1 (:deferred result))
                    (< elapsed-ms 2000)
                    (.isFile (operation-file spool item))
                    (not (.exists (settled-file state item)))
                    (not (.exists (conflict-file state item))))]
        (when-not ok
          (println "    transport diagnostic"
                   (pr-str {:result result :elapsed-ms elapsed-ms})))
        (check "bounded transport ambiguity preserves the immutable operation"
               ok)))
    (finally
      (stop-blackhole! blackhole)
      (delete-tree! tmp))))

(let [failed (remove second @checks)]
  (println
   (format "\nconcern offline reconciliation: %d / %d PASS"
           (- (count @checks) (count failed)) (count @checks)))
  (System/exit (if (empty? failed) 0 1)))
