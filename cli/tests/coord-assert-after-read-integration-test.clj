#!/usr/bin/env bb
;; Real Fram socket gate for north.coord/assert-after-read!.
;;
;; A mocked :reject cannot prove the global read→commit seam: ordinary Fram
;; :assert uses :base only as local OCC on declared-single predicates. This test
;; starts the real daemon containing :assert-at-version and proves that a write
;; to an unrelated fact invalidates the callback's snapshot, while an
;; uncontested retry commits a MULTI marker.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file *file*)) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                (str root "/../fram")))))
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw
   (ex-info
    "Fram checkout not found; set FRAM_PATH or clone it beside North"
    {:fram fram})))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/terminal-projection.clj"))
(load-file (str root "/cli/delivery-evidence-internal.clj"))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
      true)
    (catch Exception _ false)))

(defn eventually [f]
  (loop [remaining 200]
    (cond
      (try (f) (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(defn facts-of [port subject]
  (let [rows
        (:ok
         (north.coord/send-op
          port
          {:op :query
           :query
           {:find "assert_after_read_test"
            :rules
            [{:head {:rel "assert_after_read_test"
                     :args [{:var "p"} {:var "r"}]}
              :body [{:rel "triple"
                      :args [subject {:var "p"} {:var "r"}]}]}]}}))]
    (reduce (fn [facts [predicate value]]
              (update facts predicate (fnil conj #{}) value))
            {}
            rows)))

(let [port (free-port)
      dir (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-assert-after-read"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "facts.log")
      _ (spit log "")
      split-log (io/file dir "coordination.log")
      _ (spit split-log "")
      explicit-log-probe
      (proc/shell
       {:out :string :err :string :continue true
        :extra-env {"FRAM_LOG" (.getCanonicalPath log)}}
       "bb" "-e"
      (str "(load-file " (pr-str (str root "/cli/coord.clj")) ")"
            "(print (north.coord/expected-log))"))
      _ (io/delete-file split-log true)
      daemon
      ;; Pin the throwaway coordinator's OWN corpus: an inherited FRAM_LOG makes
      ;; it boot-fold the developer's live log (30s+ on a swarm corpus, so the
      ;; start check times out) and lets its telemetry land in live state.
      (proc/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"
                    "FRAM_LOG" (.getPath log)
                    "FRAM_TELEMETRY_LOG" (.getPath (io/file dir "telemetry.log"))
                    "FRAM_THREADS" (.getPath (io/file dir "threads"))}}
       "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
       (str port) (.getPath log))
      checks (atom [])
      check! (fn [label value]
               (swap! checks conj [label (boolean value)]))]
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check! "explicit FRAM_LOG wins over a sibling coordination.log"
            (and (zero? (:exit explicit-log-probe))
                 (= (.getCanonicalPath log)
                    (str/trim (:out explicit-log-probe)))))
    (check! "real Fram daemon starts"
            (eventually #(port-open? port)))

    (let [validations (atom 0)
          result
          (north.coord/assert-after-read!
           port "@run-revalidated" "run_bar_evidence" "record"
           (fn []
             ;; The first callback mutates an unrelated group after BASE was
             ;; captured. The real global assertion must reject that snapshot;
             ;; the second callback performs no write and may commit.
             (when (= 1 (swap! validations inc))
               (north.coord/append!
                port "@unrelated" "unrelated_predicate" "moved"))))]
      (check! "unrelated write forces real callback revalidation"
              (= 2 @validations))
      (check! "fresh uncontested retry commits a MULTI marker"
              (and (:ok result)
                   (= #{"record"}
                      (set (north.coord/many
                            port "@run-revalidated"
                            "run_bar_evidence"))))))

    (let [running? (atom true)
          churn-writes (atom 0)
          validations (atom 0)
          writer
          (future
            (while @running?
              (north.coord/append!
               port "@unrelated-churn" "noise"
               (str (swap! churn-writes inc)))
              (Thread/sleep 10)))
          started (System/nanoTime)
          results
          (try
            (Thread/sleep 50)
            (mapv
             (fn [index]
               (north.coord/assert-after-read!
                port (str "@run-churn-" index) "run_bar_evidence" "record"
                #(swap! validations inc)))
             (range 500))
            (finally
              (reset! running? false)
              @writer))
          elapsed-seconds (/ (- (System/nanoTime) started) 1e9)
          observed-rate (/ @churn-writes elapsed-seconds)]
      (println
       (format "  [METRIC] unrelated churn %.1f writes/s · %d writes · %d marker attempts · %d validations"
               observed-rate @churn-writes (count results) @validations))
      (check! "deadline retries sustain measured unrelated-writer churn"
              (and (>= @churn-writes 10)
                   (every? :ok results))))

    ;; Drive the production reserve! seam. Before each of its first twenty
    ;; marker assertions per run, publish one real unrelated coordinator fact.
    ;; That deterministically invalidates the captured global version while four
    ;; reservation futures contend concurrently.
    (let [forced-conflicts 20
          noise-writes (atom 0)
          marker-attempts (atom {})
          started (System/nanoTime)
          threads (mapv #(str "@thread:reservation-liveness-" %) (range 4))
          _ (doseq [thread threads]
              (north.coord/append! port thread "title" "reservation liveness")
              (north.coord/append! port thread "done_when" "coordinator passes"))
          original-send-op north.coord/send-op
          original-retry-deadline-ns north.coord/retry-deadline-ns
          original-retry-conflicts-until! north.coord/retry-conflicts-until!
          original-assert-after-read! north.coord/assert-after-read!
          deadline-observations (atom {})
          observe-deadline!
          (fn [stage deadline-ns]
            (swap! deadline-observations
                   update (.getId (Thread/currentThread))
                   (fnil conj []) [stage deadline-ns]))
          results
          (with-redefs
           [north.coord/retry-deadline-ns
            (fn
              ([]
               (let [deadline-ns (original-retry-deadline-ns)]
                 (observe-deadline! :created deadline-ns)
                 deadline-ns))
              ([timeout-ms]
               (original-retry-deadline-ns timeout-ms)))
            north.coord/retry-conflicts-until!
            (fn
              ([deadline-ns operation!]
               (observe-deadline! :retry deadline-ns)
               (original-retry-conflicts-until!
                deadline-ns Integer/MAX_VALUE operation!))
              ([deadline-ns attempts operation!]
               (observe-deadline! :retry deadline-ns)
               (original-retry-conflicts-until!
                deadline-ns attempts operation!)))
            north.coord/assert-after-read!
            (fn
              ([target-port te predicate value validate!]
               (original-assert-after-read!
                target-port te predicate value validate!))
              ([target-port te predicate value validate! attempts]
               (original-assert-after-read!
                target-port te predicate value validate! attempts))
              ([target-port te predicate value validate! attempts deadline-ns]
               (observe-deadline! :marker deadline-ns)
               (original-assert-after-read!
                target-port te predicate value validate! attempts deadline-ns)))
            north.coord/send-op
            (fn [target-port operation]
              (if (and (= :assert-batch-at-version (:op operation))
                       (str/starts-with? (str (:te operation))
                                         "@run:reservation-liveness-"))
                (let [run (:te operation)
                      attempts
                      (swap! marker-attempts update run (fnil inc 0))
                      attempt (get attempts run)]
                  (when (<= attempt forced-conflicts)
                    (original-send-op
                     target-port
                     {:op :assert
                      :te "@unrelated-reservation-churn"
                      :p "noise"
                      :r (str run "-" attempt)})
                    (swap! noise-writes inc))
                  (original-send-op target-port operation))
                (original-send-op target-port operation)))]
           (let [gate (promise)
                 reservations
                 (mapv
                  (fn [index]
                    (future
                      @gate
                      (let [run (str "@run:reservation-liveness-" index)]
                        (try
                          (north.delivery-evidence-internal/reserve!
                           port
                           {"run" run
                            "thread" (get threads index)
                            "reporter"
                            (str "agent:reservation-liveness-" index)
                            "capabilitySha256" (format "%064x" (inc index))})
                          (let [stored (facts-of port run)]
                            {:exact?
                             (and
                              (= (set north.terminal-projection/run-reservation-predicates)
                                 (set (keys stored)))
                              (= 7
                                 (count
                                  (select-keys
                                   stored
                                   north.terminal-projection/run-reservation-body-predicates)))
                              (every? #(= 1 (count %)) (vals stored)))
                             :valid?
                             (north.terminal-projection/run-reservation-valid?
                              stored)})
                          (catch Throwable error {:error error})))))
                  (range 4))]
             (deliver gate true)
             (mapv deref reservations)))]
      (let [attempt-counts
            (mapv #(get @marker-attempts
                        (str "@run:reservation-liveness-" %) 0)
                  (range 4))
            elapsed-ms (/ (- (System/nanoTime) started) 1000000.0)]
        (check! "production reserve! survives more than 16 conflicts per concurrent run"
                (and (= (* 4 forced-conflicts) @noise-writes)
                     (every? #(> % 16) attempt-counts)
                     (every? #(nil? (:error %)) results)))
        (check! "every production reservation has seven singleton body facts and one valid digest"
                (every? #(and (:exact? %) (:valid? %)) results))
        ;; Body and digest no longer have separate commit paths to share a
        ;; deadline BETWEEN: one publication deadline feeds exactly one retry
        ;; loop, and the single-fact marker CAS is gone from reserve! entirely.
        (check! "production reserve! spends one deadline on one publication loop"
                (and
                 (= 4 (count @deadline-observations))
                 (every?
                  (fn [observations]
                    (and (= 1 (count (filter #(= :created (first %)) observations)))
                         (= 1 (count (filter #(= :retry (first %)) observations)))
                         (zero? (count (filter #(= :marker (first %)) observations)))
                         (= 1 (count (set (map second observations))))))
                  (vals @deadline-observations))))
        (check! "concurrent reservation completion stays deadline-bounded"
                (< elapsed-ms
                   (+ north.coord/assert-after-read-deadline-ms 2000)))))

    ;; ALL-OR-ZERO. Every publication write for this run is preceded by a real
    ;; unrelated coordinator write, so no captured base can still be current
    ;; when the batch lands; injected monotonic time then exhausts the
    ;; publication deadline deterministically instead of racing a scheduler.
    ;; The old body-then-marker shape left seven unmarked body facts here.
    (let [run "@run:atomic-deadline"
          thread "@thread:atomic-deadline"
          original-send-op north.coord/send-op
          original-retry-deadline-ns north.coord/retry-deadline-ns
          now-ns (atom 0)
          publication-writes (atom 0)
          _ (do (north.coord/append! port thread "title" "atomic deadline")
                (north.coord/append! port thread "done_when" "coordinator passes"))
          error
          (binding
           [north.coord/*retry-monotonic-now-ns* #(deref now-ns)
            north.coord/*retry-sleep-ms!* #(swap! now-ns + (* % 1000000))
            north.coord/*retry-jitter-ms* (fn [_ cap-ms] cap-ms)]
           (with-redefs
            [north.coord/retry-deadline-ns
             (fn
               ([] (original-retry-deadline-ns 5))
               ([timeout-ms] (original-retry-deadline-ns timeout-ms)))
             north.coord/send-op
             (fn [target-port operation]
               (if (and (#{:assert :assert-at-version :assert-batch-at-version}
                         (:op operation))
                        (= run (:te operation)))
                 (do
                   (swap! publication-writes inc)
                   (original-send-op
                    target-port
                    {:op :assert :te "@unrelated-atomic-churn" :p "noise"
                     :r (str @publication-writes)})
                   (original-send-op target-port operation))
                 (original-send-op target-port operation)))]
             (try
               (north.delivery-evidence-internal/reserve!
                port {"run" run "thread" thread
                      "reporter" "@agent:atomic-deadline"
                      "capabilitySha256" (format "%064x" 11)})
               nil
               (catch clojure.lang.ExceptionInfo caught caught))))]
      (check! "an exhausted publication deadline is reported by name"
              (= "delivery evidence publication deadline exceeded"
                 (some-> error .getMessage)))
      (check! "an exhausted publication deadline leaves ZERO run facts"
              (and (pos? @publication-writes)
                   (empty? (facts-of port run)))))

    ;; READ-SET RACE. The thread contract moves exactly once, inside the window
    ;; between the reads and the publication. The reservation that lands must
    ;; digest the contract read at its WINNING base — never a stale baseline,
    ;; and never a partial body left behind by a rejected attempt.
    (let [run "@run:contract-drift"
          thread "@thread:contract-drift"
          original-send-op north.coord/send-op
          mutated (atom false)
          _ (do (north.coord/append! port thread "title" "contract drift")
                (north.coord/append! port thread "done_when" "original bar"))
          error
          (with-redefs
           [north.coord/send-op
            (fn [target-port operation]
              (if (and (#{:assert :assert-at-version :assert-batch-at-version}
                        (:op operation))
                       (= run (:te operation))
                       (compare-and-set! mutated false true))
                (do
                  (north.coord/append! port thread "done_when" "drifted bar")
                  (north.coord/retract! port thread "done_when" "original bar")
                  (original-send-op target-port operation))
                (original-send-op target-port operation)))]
            (try
              (north.delivery-evidence-internal/reserve!
               port {"run" run "thread" thread
                     "reporter" "@agent:contract-drift"
                     "capabilitySha256" (format "%064x" 12)})
              nil
              (catch Throwable caught caught)))
          stored (facts-of port run)]
      (check! "a contract mutation inside the publication window publishes all eight facts or none"
              (and (true? @mutated)
                   (nil? error)
                   (= (set north.terminal-projection/run-reservation-predicates)
                      (set (keys stored)))
                   (every? #(= 1 (count %)) (vals stored))
                   (north.terminal-projection/run-reservation-valid? stored)))
      (check! "the published baseline is the contract read at the winning base"
              (= ["drifted bar"]
                 (north.terminal-projection/run-reservation-done-when stored))))

    ;; COMPETING PUBLISHERS at one base, interleaved deterministically rather
    ;; than by luck: the loser is held at its first publication write until the
    ;; winner has published in full, so its base is provably stale.
    (let [run "@run:competing-publisher"
          thread "@thread:competing-publisher"
          winner "@agent:competing-winner"
          loser "@agent:competing-loser"
          winner-capability (format "%064x" 13)
          loser-capability (format "%064x" 14)
          original-send-op north.coord/send-op
          gate (promise)
          held (promise)
          first-writer (atom nil)
          _ (do (north.coord/append! port thread "title" "competing publisher")
                (north.coord/append! port thread "done_when" "coordinator passes"))
          loser-result
          (with-redefs
           [north.coord/send-op
            (fn [target-port operation]
              (if (and (#{:assert :assert-at-version :assert-batch-at-version}
                        (:op operation))
                       (= run (:te operation))
                       (compare-and-set! first-writer nil
                                         (.getId (Thread/currentThread))))
                (do (deliver held true)
                    @gate
                    (original-send-op target-port operation))
                (original-send-op target-port operation)))]
            (let [pending
                  (future
                    (try
                      (north.delivery-evidence-internal/reserve!
                       port {"run" run "thread" thread "reporter" loser
                             "capabilitySha256" loser-capability})
                      {:published true}
                      (catch Throwable caught
                        {:error (.getMessage caught)})))]
              (deref held 30000 :timeout)
              (north.delivery-evidence-internal/reserve!
               port {"run" run "thread" thread "reporter" winner
                     "capabilitySha256" winner-capability})
              (deliver gate true)
              (deref pending 60000 {:error "loser never settled"})))
          stored (facts-of port run)]
      (check! "a losing competing publisher lands no fact and refuses"
              (and (string? (:error loser-result))
                   (= (set north.terminal-projection/run-reservation-predicates)
                      (set (keys stored)))
                   (every? #(= 1 (count %)) (vals stored))))
      (check! "the surviving manifest is exactly the winning publisher's"
              (and (north.terminal-projection/run-reservation-valid? stored)
                   (= #{winner} (get stored "run_reservation_agent"))
                   (= #{winner-capability} (get stored "run_capability_sha256")))))

    ;; Injected monotonic time makes both sides of the bound exact rather than
    ;; relying on scheduler timing. Equal-jitter's upper selection must consume
    ;; the final remaining interval without asking the sleeper to overshoot it.
    (let [now-ns (atom 0)
          sleeps (atom [])
          jitter-bounds (atom [])
          result
          (binding
           [north.coord/*retry-monotonic-now-ns* #(deref now-ns)
            north.coord/*retry-sleep-ms!*
            (fn [delay-ms]
              (swap! sleeps conj delay-ms)
              (swap! now-ns + (* delay-ms 1000000)))
            north.coord/*retry-jitter-ms*
            (fn [floor-ms cap-ms]
              (swap! jitter-bounds conj [floor-ms cap-ms])
              cap-ms)]
           (north.coord/retry-conflicts-until!
            (north.coord/retry-deadline-ns 5)
            (constantly {:reject :conflict})))]
      (check! "injected deadline has deterministic lower and upper bounds"
              (and (= 5000000 @now-ns)
                   (= 5 (reduce + @sleeps))
                   (= :conflict (:reject result))
                   (true? (:deadline result))
                   (every?
                    (fn [[delay-ms [floor-ms cap-ms]]]
                      (<= floor-ms delay-ms cap-ms))
                    (map vector @sleeps @jitter-bounds)))))

    (let [now-ns (atom 0)
          result
          (binding
           [north.coord/*retry-monotonic-now-ns* #(deref now-ns)
            north.coord/*retry-sleep-ms!*
            #(swap! now-ns + (* % 1000000))
            north.coord/*retry-jitter-ms* (fn [floor-ms _] floor-ms)]
           (north.coord/retry-conflicts-until!
            (north.coord/retry-deadline-ns 5)
            #(if (< @now-ns 4000000)
               {:reject :conflict}
               {:ok true})))]
      (check! "retry may succeed immediately below the injected deadline"
              (and (:ok result) (= 4000000 @now-ns))))

    (let [validations (atom 0)
          result
          (north.coord/assert-after-read!
           port "@reserved-rejection" "name" "forbidden"
           #(swap! validations inc))]
      (check! "fixed rule/schema rejection is not retried"
              (and (= 1 @validations)
                   (vector? (:reject result)))))

    (let [validations (atom 0)
          now-ns (atom 0)
          result
          (binding
           [north.coord/*retry-monotonic-now-ns* #(deref now-ns)
            north.coord/*retry-sleep-ms!*
            #(swap! now-ns + (* % 1000000))
            north.coord/*retry-jitter-ms* (fn [_ cap-ms] cap-ms)]
           (north.coord/assert-after-read!
            port "@run-starved" "run_bar_evidence" "must-not-land"
            (fn []
              ;; Move a distinct unrelated fact on every bounded attempt.
              (let [attempt (swap! validations inc)]
                (north.coord/append!
                 port "@continuous-writer" "noise" (str attempt))))
            Integer/MAX_VALUE (north.coord/retry-deadline-ns 5)))]
      (check! "continuous graph movement exhausts the injected deadline"
              (and (= 3 @validations)
                   (= 5000000 @now-ns)
                   (= :conflict (:reject result))
                   (true? (:deadline result))))
      (check! "a rejected stale marker never lands"
              (empty?
               (set (north.coord/many
                     port "@run-starved" "run_bar_evidence")))))

    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks]
        (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed"
                         (- (count @checks) (count failed))
                         (count @checks)))
        (when (seq failed) (System/exit 1))))))
