#!/usr/bin/env bb
;; Concern transitions must materialize durable attention, not merely return
;; plausible event maps.
(require '[babashka.classpath :as cp]
         '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root
  (-> (io/file test-script)
      .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_PATH")
      (.getCanonicalPath
       (io/file (System/getProperty "user.home") "code" "store" "main"))))
(def runtime-classpath (str root "/out:" store "/out"))
(cp/add-classpath runtime-classpath)
(def source-path (str root "/cli/concern-cli.clj"))
(def source-text (slurp source-path))
(def main-offset (str/last-index-of source-text "\n(let [[ps verb"))
(when-not main-offset
  (throw (ex-info "concern CLI main form marker not found" {})))
(System/setProperty "babashka.file" source-path)
(load-string (subs source-text 0 main-offset))
(System/setProperty "babashka.file" test-script)

(def checks (atom []))
(defn check
  ([label value] (check label value nil))
  ([label value detail]
   (let [passed (boolean value)]
     (swap! checks conj [label passed])
     (println (str (if passed "PASS" "FAIL") " — " label))
     (when (and (not passed) detail)
       (println (str "       " (pr-str detail)))))))
(defn thrown [f]
  (try (f) nil (catch Exception error error)))

(defn port-free? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      false)
    (catch Exception _ true)))

(def port
  (or (some #(when (port-free? %) %) (range 7650 7680))
      (throw (ex-info "no test port available" {}))))
(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-concern-attention"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def log (io/file tmp "facts.storelog"))
(def telemetry (io/file tmp "telemetry.storelog"))
(def candidate-repo (.getCanonicalPath (io/file tmp "candidate-repo")))
(doseq [result
        [(shell/sh "git" "init" "-q" "-b" "feature" candidate-repo)
         (shell/sh "git" "-C" candidate-repo
                   "-c" "user.name=North Test"
                   "-c" "user.email=north-test@example.invalid"
                   "commit" "-q" "--allow-empty" "-m" "candidate fixture")]]
  (when-not (zero? (:exit result))
    (throw (ex-info "candidate Git fixture failed" {:result result}))))
(def canonical-log (.getCanonicalPath log))
(def canonical-telemetry-log (.getCanonicalPath telemetry))
(def isolated-env
  {"BEAGLE_STORE_LOG" canonical-log
   "BEAGLE_STORE_SPACE_ID" "north-coordination"
   "BEAGLE_STORE_TELEMETRY_LOG" canonical-telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})
(def daemon
  (p/process {:dir store
              :out :string
              :err :string
              :extra-env (assoc isolated-env
                                "BEAGLE_STORE_SERVER_QUIET" "1"
                                "BEAGLE_STORE_SERVER_XMX" "1g")}
             (str store "/bin/beagle-store-server") "serve" (str port)
             canonical-log "north-coordination"))

(defn cleanup []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (doseq [file (reverse (file-seq tmp))]
    (io/delete-file file true)))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))

(defn await-up []
  (loop [attempt 0]
    (let [status (try (north.coord/status! port) (catch Throwable _ nil))]
      (cond
        (and (= :ready (:state status))
             (= "north-coordination" (:space-id status))) true
        (>= attempt 800) false
        :else (do (Thread/sleep 25) (recur (inc attempt)))))))

(defn fail-daemon-boot! []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (let [result (deref daemon 5000 nil)]
    (throw
     (ex-info
      "throwaway coordinator failed to start"
      {:exit (:exit result)
       :stdout (or (:out result) "<unavailable>")
       :stderr (or (:err result) "<unavailable>")}))))

(defn fact! [subject predicate object]
  (let [result (north.coord/append! port subject predicate object)]
    (when (:reject result)
      (throw (ex-info "fixture fact write failed" result)))
    result))

(defn run-concern-in [directory & args]
  @(apply p/process
          {:dir directory
           :out :string
           :err :string
           :extra-env isolated-env}
          "bb" "-cp" runtime-classpath source-path (str port) args))

(defn run-concern [& args]
  (apply run-concern-in root args))

(defn start-concern [& args]
  (apply p/process
         {:dir root
          :out :string
          :err :string
          :extra-env isolated-env}
         "bb" "-cp" runtime-classpath
         "cli/concern-cli.clj" (str port) args))

(defn concern-id [result]
  (when-let [id (some-> (re-find #"(concern-\d+-[a-f0-9]+)" (:out result)) second)]
    (str "@" id)))

(defn concern-subjects []
  (->> (north.coord/query-rows!
        port
        {:find "concern"
         :rules
         [{:head {:rel "concern" :args [{:var "c"}]}
           :body [{:rel "triple"
                   :args [{:var "c"} "kind" "concern"]}]}]})
       (map first)
       set))

(defn values-of [subject predicate]
  (set (north.coord/many! port subject predicate)))

(defn notification-subjects []
  (->> (north.coord/query-rows!
        port
        {:find "notification"
         :rules
         [{:head {:rel "notification" :args [{:var "n"}]}
           :body [{:rel "triple"
                   :args [{:var "n"} "kind" "notification"]}]}]})
       (map first)
       set))

(defn notification-rows []
  (->> (notification-subjects)
       (map (fn [subject]
              {:id subject
               :recipient (first (values-of subject "recipient"))
               :event-key (first (values-of subject "event_key"))
               :attention-kind (first (values-of subject "attention_kind"))
               :delivery (first (values-of subject "delivery"))
               :about (first (values-of subject "about"))
               :body (first (values-of subject "body"))}))
       (sort-by :id)
       vec))

(try
  (let [started? (await-up)]
    (check "throwaway coordinator starts" started?)
    (when-not started?
      (fail-daemon-boot!)))
  (fact! "@thread-attention" "title" "Attention parent")
  (fact! "@thread-attention" "kind" "thread")
  (fact! "@wrong-kind" "title" "Not a thread")
  (fact! "@wrong-kind" "kind" "document")

  (let [requests (atom [])
        page
        (with-redefs
          [north.coord/bounded-query-in-domain!
           (fn [_ domain query limit]
             (swap! requests conj {:domain domain :query query :limit limit})
             {:rows [] :served-version 0})]
          (pending-attention-event-intents port nil))
        errored
        (with-redefs
          [indexed-predicate-rows
           (fn [& _]
             (throw (ex-info "fixture failure"
                             {:type :indexed-query-error})))]
          (thrown #(pending-attention-event-intents port nil)))
        malformed
        (with-redefs
          [indexed-predicate-rows
           (fn [& _] [["one-column-unscoped"]])]
          (thrown #(pending-attention-event-intents port nil)))]
    (check "terminal outbox uses one bounded live-intent index"
           (and (= 1 (count @requests))
                (every? #(= :coordination (:domain %)) @requests)
                (every? #(= automatic-index-row-limit (:limit %)) @requests)
                (false? (:more page))
                (empty? (:intents page))))
    (check "terminal outbox index errors and malformed rows fail visibly"
           (and (= :indexed-query-error
                   (:type (ex-data errored)))
                (= :malformed-concern-attention-index
                   (:type (ex-data malformed))))))

  (let [predicates (atom [])
        result
        (with-redefs
          [indexed-predicate-rows
           (fn [_ predicate]
             (swap! predicates conj predicate)
             [])
           all-concerns
           (fn [& _]
             (throw (ex-info "whole-corpus concern scan invoked" {})))
           concern-meta-index
           (fn [& _]
             (throw (ex-info "whole-corpus concern metadata invoked" {})))]
          (reconcile-attention! port))]
    (check "automatic attention reconciliation reads only live indexed outboxes"
           (and (= #{attention-event-intent-predicate
                     attention-reconcile-pending-predicate}
                   (set @predicates))
                (zero? (:overlaps result))
                (zero? (:deferred result)))))

  (let [base-declaration (run-concern "declare" "agent-a" "/tmp/no-code"
                                      "base declaration" "src/shared.clj")
        base-id (concern-id base-declaration)
        before-invalid (concern-subjects)
        invalid (run-concern "declare" "agent-invalid" "/tmp/no-code"
                             "invalid about" "src/other.clj"
                             "--about" "@wrong-kind")
        after-invalid (concern-subjects)
        anchored (run-concern "declare" "agent-b" "/tmp/no-code"
                              "anchored declaration" "src/shared.clj"
                              "--about" "@thread-attention")
        anchored-id (concern-id anchored)
        anchored-state (meta-of port anchored-id)
        discovered (path-overlap-data port anchored-id)
        overlap (first (:overlaps discovered))
        base-state (meta-of port base-id)
        entered-reconcile (run-concern "reconcile-attention" anchored-id)
        entered-rows (notification-rows)]
    (check "a declaration without an about binding remains valid"
           (and (zero? (:exit base-declaration)) base-id))
    (check "wrong-kind about is rejected before any concern mutation"
           (and (= 2 (:exit invalid))
                (= before-invalid after-invalid)))
    (check "validated about is stored as the exact thread ref"
           (and (zero? (:exit anchored))
                (= "@thread-attention"
                   (north.coord/resolved! port anchored-id "about"))))
    (check "later declaration discovers the earlier active overlap"
           (and (= 1 (count (:overlaps discovered)))
                (= #{base-id anchored-id}
                   (set (:source-concerns overlap)))
                (= ["src/shared.clj"] (:shared overlap))))
    (check "entered overlap reconciliation publishes two recipient-scoped notifications"
           (and (zero? (:exit entered-reconcile))
                (= 2 (count entered-rows))
                (= 2 (count (set (map :id entered-rows))))
                (= #{"@agent-a" "@agent-b"}
                   (set (map :recipient entered-rows)))
                (= 1 (count (set (map :event-key entered-rows))))
                (every? #(= "overlap-entered" (:attention-kind %))
                        entered-rows)
                (every? #(= "notify" (:delivery %)) entered-rows)
                (every? #(= "@thread-attention" (:about %)) entered-rows)
                (every? #(not (str/includes? (:body %) "src/shared.clj"))
                        entered-rows))
           {:reconcile entered-reconcile :rows entered-rows})

    (let [forward (canonical-overlap base-state anchored-state
                                     #{"src/z.clj" "src/a.clj"} "path")
          reverse (canonical-overlap anchored-state base-state
                                     ["src/a.clj" "src/z.clj"] "path")]
      (check "pair identity and evidence are invariant under A/B order reversal"
             (= forward reverse)))

    (let [entered (attention-events-for-transition nil anchored-state [overlap])
          replay (attention-events-for-transition nil anchored-state [overlap])
          likely-state (assoc anchored-state :status "likely-to-land")
          likely (attention-events-for-transition anchored-state likely-state
                                                  [overlap])
          landed-state (assoc anchored-state :status "landed")
          left (first
                (attention-events-for-transition
                 anchored-state landed-state [overlap]))
          encoded
          (attention-event-intent-value
           port anchored-id "landed" left)
          decoded (parse-attention-event-intent port anchored-id encoded)
          duplicate-status
          (attention-events-for-transition likely-state likely-state [overlap])]
      (check "overlap-entered emits stable per-recipient specs for both owners"
             (and (= 2 (count entered))
                  (= #{"@agent-a" "@agent-b"} (set (map :to entered)))
                  (= entered replay)
                  (= 1 (count (set (map :event-key entered))))
                  (every? #(= "@thread-attention" (:about %)) entered)))
      (check "likely-to-land emits one stable peer event and double-report emits none"
             (and (= 1 (count likely))
                  (= "@agent-a" (:to (first likely)))
                  (= likely
                     (attention-events-for-transition anchored-state likely-state
                                                      [overlap]))
                  (empty? duplicate-status)))
      (check "terminal outbox encoding is bounded canonical EDN"
             (and (= left (:event decoded))
                  (= "landed" (:trigger-status decoded))
                  (= encoded
                     (attention-event-intent-value
                      port anchored-id "landed" (:event decoded)))))
      (check "forged terminal recipient/source and oversized intent fail closed"
             (every?
              #(= :invalid-concern-attention-intent
                  (:type (ex-data %)))
              [(try
                 (attention-event-intent-value
                  port anchored-id "landed"
                  (assoc left :to "@agent-forged"))
                 (catch Exception error error))
               (try
                 (attention-event-intent-value
                  port anchored-id "landed"
                  (assoc left
                         :source-concerns
                         [anchored-id "@concern-forged"]))
                 (catch Exception error error))
               (try
                 (parse-attention-event-intent
                  port anchored-id (apply str (repeat 20000 "x")))
                 (catch Exception error error))])))

    (let [first-pass (start-concern "reconcile-attention" anchored-id)
          second-pass (start-concern "reconcile-attention" anchored-id)
          first-result @first-pass
          second-result @second-pass]
      (check "concurrent reconciliation converges without duplicate notifications"
             (and (zero? (:exit first-result))
                  (zero? (:exit second-result))
                  (= 2 (count (notification-rows))))))

    (let [likely (run-concern-in candidate-repo "status" anchored-id "likely-to-land")
          after-likely (notification-rows)
          repeated (run-concern-in candidate-repo "status" anchored-id "likely-to-land")
          after-repeat (notification-rows)
          likely-rows (filter #(= "likely-to-land" (:attention-kind %))
                              after-repeat)]
      (check "likely-to-land publishes one peer notification exactly once"
             (and (zero? (:exit likely))
                  (zero? (:exit repeated))
                  (= 3 (count after-likely))
                  (= 3 (count after-repeat))
                  (= 1 (count likely-rows))
                  (= "@agent-a" (:recipient (first likely-rows))))))

    (let [crash
          (try
            (with-redefs
              [reconcile-attention!
               (fn [& _]
                 (throw
                  (ex-info "injected post-commit publication crash"
                           {:type :injected-publication-crash})))]
              (terminal-concern-transition! port anchored-id "landed"))
            nil
            (catch Exception error error))
          after-commit-before-replay (notification-rows)
          intents (values-of anchored-id attention-event-intent-predicate)
          settled-before
          (values-of anchored-id attention-event-settled-predicate)
          pending-before (pending-attention-event-intents port anchored-id)
          reconciled (run-concern "reconcile-attention" anchored-id)
          after-reconcile (notification-rows)
          settled-after
          (values-of anchored-id attention-event-settled-predicate)
          pending-after (pending-attention-event-intents port anchored-id)
          repeated-reconcile
          (run-concern "reconcile-attention")
          current-kinds (frequencies (map :attention-kind after-reconcile))]
      (check "terminal state and replayable intent commit before publication"
             (and (= :injected-publication-crash (:type (ex-data crash)))
                  (= "landed" (:status (meta-of port anchored-id)))
                  (= 3 (count after-commit-before-replay))
                  (= 1 (count intents))
                  (empty? settled-before)
                  (= 1 (count (:intents pending-before))))
             {:crash-type (:type (ex-data crash))
              :crash-data (ex-data crash)
              :status (:status (meta-of port anchored-id))
              :notification-count (count after-commit-before-replay)
              :intent-count (count intents)
              :settled-before settled-before
              :pending-before pending-before})
      (check "terminal reconciliation publishes, settles, and dedupes the crash gap"
             (and (zero? (:exit reconciled))
                  (zero? (:exit repeated-reconcile))
                  (empty? (values-of anchored-id attention-event-intent-predicate))
                  (= intents settled-after)
                  (empty? (:intents pending-after))
                  (= after-reconcile (notification-rows))
                  (= 2 (get current-kinds "overlap-entered"))
                  (= 1 (get current-kinds "likely-to-land"))
                  (= 1 (get current-kinds "overlap-left"))
                  (empty? (:overlaps (path-overlap-data port base-id))))
             {:reconciled reconciled
              :repeated repeated-reconcile
              :intent-count (count intents)
              :settled-after settled-after
              :pending-after pending-after
              :notification-count (count after-reconcile)
              :current-kinds current-kinds
              :final-notification-count (count (notification-rows))}))

    (let [building {:id "@concern-a" :kind "concern" :agent "@agent-a"
                    :status "building" :abandoned false}
          abandoned (assoc building :abandoned true)
          peer {:id "@concern-b" :kind "concern" :agent "@agent-b"
                :status "building" :abandoned false}
          pair (canonical-overlap building peer ["src/shared.clj"] "path")]
      (check "an abandoned member suppresses subsequent pair events"
             (empty?
              (attention-events-for-transition abandoned abandoned [pair])))
      (check "an unroutable owner fact drops its notification, never the transition"
             (empty?
              (attention-events-for-transition
               building (assoc building :status "landed")
               [(canonical-overlap building (assoc peer :agent "not a ref")
                                   ["src/shared.clj"] "path")]))))

    ;; A concern seeded outside `declare` (raw fact writes) can carry a BARE owner
    ;; handle where the board's convention is a ref — and it is a PEER's fact, so
    ;; it must never fail this owner's declare or transition.
    (let [seed-id (str "@concern-" (System/currentTimeMillis) "-seed")
          _ (doseq [[predicate value]
                    [["kind" "concern"] ["agent" "agent-seeded"]
                     ["driver" "@agent-seeded"] ["repo" "/tmp/no-code"]
                     ["intent" "hand-seeded peer"] ["touches" "src/bare.clj"]
                     ["reached" "building"]]]
              (fact! seed-id predicate value))
          host (run-concern "declare" "agent-host" "/tmp/no-code"
                            "bare-owner host" "src/bare.clj")
          host-id (concern-id host)
          landed (try (terminal-concern-transition! port host-id "landed")
                      (catch Exception error {:status :threw
                                              :error (ex-message error)}))
          recipients (set (map :recipient (notification-rows)))]
      (check "a peer's bare owner handle canonicalizes instead of failing declare or the transition"
             (and (zero? (:exit host))
                  (= "@agent-seeded" (:agent (meta-of port seed-id)))
                  (= :committed (:status landed))
                  (contains? recipients "@agent-seeded"))
             {:host-exit (:exit host) :host-err (:err host)
              :landed landed :recipients recipients}))

    (check "declare stores one owner ref even when the caller already passed one"
           (let [reffed (run-concern "declare" "@agent-reffed" "/tmp/no-code"
                                     "ref-passing declaration" "src/reffed.clj")
                 reffed-id (concern-id reffed)]
             (= ["@agent-reffed" "@agent-reffed"]
                [(north.coord/resolved! port reffed-id "agent")
                 (north.coord/resolved! port reffed-id "driver")]))))

  (let [mint (- (System/currentTimeMillis) (* 25 60 60 1000))
        retiring (str "@concern-" mint "-retire")
        peer (str "@concern-" (inc mint) "-peer")]
    (doseq [[subject agent intent]
            [[retiring "@retire-owner" "stale concern"]
             [peer "@retire-peer" "live peer"]]]
      (fact! agent "display_name" (subs agent 1))
      (doseq [[predicate value]
              [["title" intent]
               ["kind" "concern"]
               ["agent" agent]
               ["repo" "/tmp/no-code"]
               ["intent" intent]
               ["touches" "src/retire-shared.clj"]
               ["reached" "building"]]]
        (fact! subject predicate value)))
    (let [seed (run-concern "reconcile-attention" retiring)
          retired (run-concern "retire-stale" retiring)
          state (meta-of port retiring)
          left
          (filter
           #(and (= "overlap-left" (:attention-kind %))
                 (= #{retiring peer}
                    (values-of (:id %) "source_concern")))
           (notification-rows))
          intents (values-of retiring attention-event-intent-predicate)
          settled (values-of retiring attention-event-settled-predicate)]
      (check "stale-concern janitor boundary emits and settles overlap-left"
             (and (zero? (:exit seed))
                  (zero? (:exit retired))
                  (str/includes? (:out retired) ":status :committed")
                  (:abandoned state)
                  (= 1 (count left))
                  (= "@retire-peer" (:recipient (first left)))
                  (empty? intents)
                  (= 1 (count settled))))))

  (let [maintenance-source
        (slurp (str root "/cli/coordination-maintenance-task-host.clj"))]
    (check "stale-concern janitor calls the concern boundary and never appends terminal directly"
           (and
            (str/includes?
             maintenance-source
             "(retire-stale-concern! c)")
            (not
             (re-find
              #"append!\s+port\s+c\s+\"reached\"\s+\"abandoned-stale\""
              maintenance-source)))))

  (let [projection (concern-projection port nil)
        rows (:concerns projection)]
    (check "concern projection remains exact version 1"
           (and (= #{:version :concerns} (set (keys projection)))
                (= 1 (:version projection))
                (every?
                 #(= #{:id :agent :repo :intent :maturity :classification
                       :online :retired :touches}
                     (set (keys %)))
                 rows))))

  ;; Exercise wrapper argument injection without a coordinator: a fake bb prints
  ;; the argv the wrapper would exec.
  (let [fake-bb (io/file tmp "fake-bb")
        _ (spit fake-bb "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\"\n")
        _ (.setExecutable fake-bb true)
        base-env {"NORTH_HOME" root
                  "NORTH_BB" (.getPath fake-bb)
                  "NORTH_PORT" "7650"}
        injected
        @(p/process {:dir root :out :string :err :string
                     :extra-env (assoc base-env
                                       "NORTH_THREAD_ID" "thread-attention")}
                    "bash" "bin/concern" "declare" "agent" "/tmp/no-code"
                    "intent" "src/file.clj")
        explicit
        @(p/process {:dir root :out :string :err :string
                     :extra-env (assoc base-env
                                       "NORTH_THREAD_ID" "thread-ambient")}
                    "bash" "bin/concern" "declare" "agent" "/tmp/no-code"
                    "intent" "src/file.clj"
                    "--about" "@thread-explicit")]
    (check "wrapper injects one ambient about only when the caller omitted it"
           (and (= 1 (count (re-seq #"(?m)^--about$" (:out injected))))
                (str/includes? (:out injected) "@thread-attention")
                (= 1 (count (re-seq #"(?m)^--about$" (:out explicit))))
                (str/includes? (:out explicit) "@thread-explicit")
                (not (str/includes? (:out explicit) "@thread-ambient")))))

  (finally
    (cleanup)))

(let [failed (remove second @checks)]
  (println
   (format "\nconcern attention seam: %d / %d PASS"
           (- (count @checks) (count failed))
           (count @checks)))
  (System/exit (if (empty? failed) 0 1)))
