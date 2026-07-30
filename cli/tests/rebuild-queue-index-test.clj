#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (-> (or (System/getProperty "babashka.file") *file*)
              io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(System/setProperty "north.rebuild-request-cli.lib" "1")
(System/setProperty "babashka.file" (str root "/cli/rebuild-request-cli.clj"))
(load-file (str root "/cli/rebuild-request-cli.clj"))

(alias 'rq 'north.rebuild-request)
(alias 'qs 'north.rebuild-request-state)
(alias 'bridge 'north.rebuild-queue-legacy)

(def checks (atom []))
(defn check [label ok & [detail]]
  (swap! checks conj [label (boolean ok) detail])
  (println (if ok (str "PASS " label) (str "FAIL " label)))
  (when (and (not ok) detail) (println (str "  " detail))))

(defn throws-type? [type f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (= type (:type (ex-data error))))))

(def temp-root
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-rebuild-queue-index-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def log-file (io/file temp-root "coordination.log"))
(spit log-file "")

(def store
  (atom {:version 0
         :queue-cell-version 0
         :queue-raw nil
         :queue-asserts 0
         :rows {}}))
(def base-barrier (atom nil))

(defn append-line! [record]
  (spit log-file (str (pr-str record) "\n") :append true))

(defn commit-record! [record]
  (locking store
    (let [tx (inc (:version @store))
          committed (merge {:tx tx :frame "rebuild-queue-index-test"} record)]
      (swap! store assoc :version tx)
      (append-line! committed)
      tx)))

(defn values-at [subject predicate]
  (get-in @store [:rows subject predicate] []))

(defn fake-show [_port subject]
  (if (= rq/queue-subject subject)
    (if-let [raw (:queue-raw @store)] [[rq/queue-predicate raw]] [])
    (->> (get-in @store [:rows subject] {})
         (mapcat (fn [[predicate values]]
                   (map (fn [value] [predicate value]) values)))
         vec)))

(defn fake-cur [_port _subject]
  (let [version (:version @store)
        latch @base-barrier]
    (when latch
      (.countDown ^java.util.concurrent.CountDownLatch latch)
      (.await ^java.util.concurrent.CountDownLatch latch))
    version))

(defn fake-send [_port operation]
  (case (:op operation)
    :assert
    (if (= rq/queue-subject (:te operation))
      (locking store
        (if (> (:queue-cell-version @store) (:base operation))
          {:reject :conflict :version (:version @store)}
          (let [tx (inc (:version @store))
                raw (:r operation)]
            (swap! store assoc
                   :version tx
                   :queue-cell-version tx
                   :queue-raw raw)
            (swap! store update :queue-asserts inc)
            (append-line! {:tx tx :op "assert"
                           :l rq/queue-subject :p rq/queue-predicate :r raw
                           :frame "rebuild-queue-index-test"})
            {:ok tx})))
      (throw (ex-info "unexpected fake assert" {:operation operation})))

    :assert-batch
    (locking store
      (let [subject (:te operation)
            tx (inc (:version @store))]
        (swap! store assoc :version tx)
        (doseq [{:keys [p r]} (:facts operation)]
          (swap! store assoc-in [:rows subject p] [r])
          (append-line! {:tx tx :op "assert" :l subject :p p :r r
                         :frame "rebuild-queue-index-test"}))
        {:ok tx}))

    (throw (ex-info "unexpected fake coordinator operation"
                    {:operation operation}))))

(defn fake-put [_port subject predicate value]
  (locking store
    (let [tx (inc (:version @store))]
      (swap! store assoc :version tx)
      (swap! store assoc-in [:rows subject predicate] [value])
      (append-line! {:tx tx :op "assert" :l subject :p predicate :r value
                     :frame "rebuild-queue-index-test"})
      {:ok tx})))

(defn request [id why at]
  (qs/new-request {:id id :requester "test-agent" :why why
                   :created-at-ms at}))

(def queue-snapshot! (ns-resolve 'north.rebuild-request 'queue-snapshot!))
(def enqueue! (ns-resolve 'north.rebuild-request 'enqueue-request!))
(def dequeue! (ns-resolve 'north.rebuild-request 'dequeue-request!))
(def settle-window! (ns-resolve 'north.rebuild-request 'settle-window-queue!))
(def decode-state (ns-resolve 'north.rebuild-request 'decode-queue-state))

(def fixed-now 2000000000000)
(def legacy-a "2000000000001-aaaa0001")
(def legacy-b "2000000000002-bbbb0002")
(def legacy-c "2000000000003-cccc0003")

(defn legacy-request-record [id why at]
  {:op "assert"
   :l (str rq/subject-prefix id)
   :p "rebuild_request"
   :r (cheshire.core/generate-string
       (sorted-map "version" qs/protocol-version
                   "requester" "legacy-agent"
                   "why" why
                   "createdAtMs" at
                   "urgent" false))})

(defn queue-state []
  (some-> (:queue-raw @store) decode-state))

(try
  (with-redefs
    [north.coord/expected-log (fn [] (.getCanonicalPath log-file))
     north.coord/show-rows fake-show
     north.coord/cur-ver-for-subject fake-cur
     north.coord/send-op fake-send
     north.coord/resolved (fn [& _] "single")
     north.coord/put! fake-put
     north.coord/many (fn [_port subject predicate]
                        (values-at subject predicate))
     rq/coordination-on? (constantly true)
     rq/window-seconds (constantly 3600)
     rq/now-ms (constantly fixed-now)
     rq/ensure-schema! (fn [_] nil)
     north.coord/indexed-query
     (fn [& _] (throw (ex-info "corpus query used" {:type :corpus-query-used})))]

    (let [plan (rq/plan-window 7977)]
      (check "bootstrap is scan-free and leaves an empty queue idle"
             (and (= :idle (:action plan))
                  (= "bootstrap" (get-in plan [:queue-read :mode]))
                  (zero? (get-in plan [:queue-read :corpus-queries]))
                  (:caught-up (:queue-read plan))))
      (check "bootstrap publishes one exact singleton"
             (= 1 (:queue-asserts @store))))

    (let [before (:queue-asserts @store)
          plan (rq/plan-window 7977)]
      (check "the immediate steady pass consumes only its self tail"
             (and (= :idle (:action plan))
                  (= "self-tail" (get-in plan [:queue-read :mode]))
                  (= before (:queue-asserts @store))
                  (zero? (get-in plan [:queue-read :corpus-queries])))))

    (commit-record! {:op "assert" :l "@unrelated" :p "note" :r "advance"})
    (let [before (:queue-asserts @store)
          plan (rq/plan-window 7977)]
      (check "unrelated tail traffic advances the durable cursor once"
             (and (= :idle (:action plan))
                  (= "incremental" (get-in plan [:queue-read :mode]))
                  (= (inc before) (:queue-asserts @store))
                  (pos? (get-in plan [:queue-read :bytes-read]))
                  (zero? (get-in plan [:queue-read :relevant-events])))))

    (commit-record! (legacy-request-record legacy-a "legacy after seed" (- fixed-now 1000)))
    (let [plan (rq/plan-window 7977)
          ids (mapv :id (:open plan))]
      (check "a legacy-only request appended after singleton creation is planned once"
             (and (= :fire (:action plan))
                  (= [legacy-a] ids)
                  (= 1 (get-in plan [:queue-read :relevant-events]))
                  (zero? (get-in plan [:queue-read :corpus-queries])))))

    (commit-record! (legacy-request-record legacy-a "legacy after seed" (- fixed-now 1000)))
    (check "a replayed legacy request remains one active membership"
           (= [legacy-a] (mapv :id (:requests (queue-state)))))

    (commit-record!
     {:op "assert"
      :l (str rq/subject-prefix legacy-a)
      :p "rebuild_request_satisfied"
      :r "{\"generation\":\"/nix/store/legacy\",\"atMs\":2000000000004}"})
    (let [plan (rq/plan-window 7977)]
      (check "a legacy-only satisfaction appended after seed prunes before planning"
             (and (= :idle (:action plan))
                  (empty? (:requests (queue-state)))
                  (pos? (get-in plan [:queue-read :relevant-events])))))

    (commit-record! (legacy-request-record legacy-b "wait after fired" (- fixed-now 500)))
    (commit-record! {:op "assert"
                     :l "@rebuild-window:1999999999999-abcd0001"
                     :p "window_action" :r "fired"})
    (let [plan (rq/plan-window 7977)]
      (check "a legacy fired-window tail preserves the coalescing timestamp"
             (and (= :waiting (:action plan))
                  (= :window-not-due (:reason plan))
                  (= 1999999999999 (:last-fired-ms (queue-state)))
                  (= 2 (get-in plan [:queue-read :relevant-events])))))

    (let [a (request "2000000000100-aaaa0100" "race-a" fixed-now)
          b (request "2000000000101-bbbb0101" "race-b" (inc fixed-now))
          latch (java.util.concurrent.CountDownLatch. 2)]
      (reset! base-barrier latch)
      (let [fa (future (enqueue! 7977 a))
            fb (future (enqueue! 7977 b))]
        @fa @fb)
      (reset! base-barrier nil)
      (let [ids (set (map :id (:requests (queue-state))))]
        (check "two same-base admissions retry to the set union"
               (and (contains? ids (:id a)) (contains? ids (:id b))))))

    (let [settled-id "2000000000100-aaaa0100"
          admitted (request "2000000000102-cccc0102" "race-new" (+ fixed-now 2))
          latch (java.util.concurrent.CountDownLatch. 2)]
      (reset! base-barrier latch)
      (let [remove-future (future (dequeue! 7977 settled-id))
            add-future (future (enqueue! 7977 admitted))]
        @remove-future @add-future)
      (reset! base-barrier nil)
      (let [ids (set (map :id (:requests (queue-state))))]
        (check "settlement racing admission preserves the new id exactly once"
               (and (not (contains? ids settled-id))
                    (= 1 (count (filter #{(:id admitted)} ids)))))))

    (let [left "2000000000101-bbbb0101"
          right "2000000000102-cccc0102"
          first-window "2000000000200-dddd0200"
          second-window "2000000000300-eeee0300"
          latch (java.util.concurrent.CountDownLatch. 2)]
      (reset! base-barrier latch)
      (let [one (future (settle-window! 7977 first-window [left]))
            two (future (settle-window! 7977 second-window [right]))]
        @one @two)
      (reset! base-barrier nil)
      (check "atomic disjoint window settlements remove both and retain maximum fired time"
             (and (not-any? #{left right} (map :id (:requests (queue-state))))
                  (= 2000000000300 (:last-fired-ms (queue-state))))))

    (let [crash-id "2000000000400-ffff0400"]
      (commit-record! (legacy-request-record crash-id "crash after canonical fact"
                                             (+ fixed-now 400)))
      (let [plan (rq/plan-window 7977)]
        (check "a crash after the canonical request fact cannot lose the accepted ask"
               (= 1 (count (filter #{crash-id} (mapv :id (:open plan))))))))

    (let [partial-id "2000000000500-abcd0500"
          line (pr-str (merge {:tx (inc (:version @store))
                               :frame "rebuild-queue-index-test"}
                              (legacy-request-record partial-id "partial" (+ fixed-now 500))))
          before (:queue-asserts @store)]
      (spit log-file line :append true)
      (check "a partial trailing event blocks planning and remains unconsumed"
             (and (throws-type? :legacy-log-partial #(rq/plan-window 7977))
                  (= before (:queue-asserts @store))))
      (spit log-file "\n" :append true)
      (swap! store update :version inc)
      (check "the completed trailing event is imported on the next pass"
             (= 1
                (count
                 (filter #{partial-id}
                         (mapv :id (:open (rq/plan-window 7977))))))))

    (let [rollback-id "2000000000600-abcd0600"]
      (with-redefs [rq/mint-id (constantly rollback-id)]
        (rq/record-request! 7977
                            {:requester "candidate-agent"
                             :why "rollback-visible pending request"}))
      (let [pending (rq/decode-request
                     7977 (str rq/subject-prefix rollback-id))]
        (check "candidate admission retains the legacy request ledger"
               (= "rollback-visible pending request" (:why pending))))
      (rq/mark-satisfied! 7977 rollback-id
                          {:intent "00000000-0000-0000-0000-000000000000"
                           :generation "/nix/store/candidate"})
      (let [settled (rq/decode-request
                     7977 (str rq/subject-prefix rollback-id))]
        (check "candidate settlement remains visible to the legacy exact reader"
               (= "/nix/store/candidate"
                  (get-in settled [:satisfied :generation])))))

    (spit log-file "{not-edn}\n" :append true)
    (swap! store update :version inc)
    (check "a malformed complete legacy event fails closed"
           (throws-type? :legacy-log-malformed #(rq/plan-window 7977))))

  (let [oversize (io/file temp-root "oversize.log")]
    (spit oversize (apply str (repeat (inc bridge/max-batch-bytes) "x")))
    (check "one bridge read refuses a record larger than its fixed buffer"
           (throws-type?
            :legacy-log-line-too-long
            #(bridge/read-batch (.getCanonicalPath oversize)
                                0 (.length oversize)))))

  (finally
    (doseq [file (reverse (file-seq temp-root))]
      (try (io/delete-file file true) (catch Throwable _ nil)))))

(let [failed (remove second @checks)]
  (println (str "rebuild queue index: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
