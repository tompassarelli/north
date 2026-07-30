#!/usr/bin/env bb
;; Concern transitions must materialize durable attention, not merely return
;; plausible event maps.
(require '[babashka.process :as p]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root
  (-> (io/file test-script)
      .getParentFile .getParentFile .getParentFile .getCanonicalPath))
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

(def fram "/home/tom/code/fram/main")
(def port
  (or (some #(when (port-free? %) %) (range 7650 7680))
      (throw (ex-info "no test port available" {}))))
(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-concern-attention"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def log (io/file tmp "facts.log"))
(def telemetry (io/file tmp "telemetry.log"))
(spit log "")
(spit telemetry "")
(def canonical-log (.getCanonicalPath log))
(def canonical-telemetry-log (.getCanonicalPath telemetry))
(def isolated-env
  {"FRAM_LOG" canonical-log
   "FRAM_TELEMETRY_LOG" canonical-telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})
(def daemon
  (p/process {:dir fram
              :out :string
              :err :string
              :extra-env (assoc isolated-env "FRAM_REQUIRE_LOG_FENCE" "1")}
             "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
             (str port) canonical-log))

(defn cleanup []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (doseq [file (reverse (file-seq tmp))]
    (io/delete-file file true)))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))

(defn await-up []
  (loop [attempt 0]
    (cond
      (not (port-free? port)) true
      (>= attempt 300) false
      :else (do (Thread/sleep 250) (recur (inc attempt))))))

(defn fail-daemon-boot! []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (let [result (deref daemon 5000 nil)]
    (throw
     (ex-info
      "throwaway coordinator failed to start"
      {:exit (:exit result)
       :stdout (or (:out result) "<unavailable>")
       :stderr (or (:err result) "<unavailable>")}))))

(defn op [request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write
       writer
       (.getBytes
        (str (pr-str {:op :for-log
                      :expected-log canonical-log
                      :request request})
             "\n")))
      (.flush writer)
      (edn/read-string (.readLine reader)))))

(defn fact! [subject predicate object]
  (let [result (op {:op :assert :te subject :p predicate :r object})]
    (when-not (:ok result)
      (throw (ex-info "fixture fact write failed" result)))
    result))

(defn run-concern [& args]
  @(apply p/process
          {:dir root
           :out :string
           :err :string
           :extra-env isolated-env}
          "bb" "cli/concern-cli.clj" (str port) args))

(defn start-concern [& args]
  (apply p/process
         {:dir root
          :out :string
          :err :string
          :extra-env isolated-env}
         "bb" "cli/concern-cli.clj" (str port) args))

(defn concern-id [result]
  (when-let [id (some-> (re-find #"(concern-\d+-[a-f0-9]+)" (:out result)) second)]
    (str "@" id)))

(defn concern-subjects []
  (->> (:ok
        (op {:op :query
             :query
             {:find "concern"
              :rules
              [{:head {:rel "concern" :args [{:var "c"}]}
                :body [{:rel "triple"
                        :args [{:var "c"} "kind" "concern"]}]}]}}))
       (map first)
       set))

(defn values-of [subject predicate]
  (->> (:ok
        (op {:op :query
             :query
             {:find "value"
              :rules
              [{:head {:rel "value" :args [{:var "value"}]}
                :body [{:rel "triple"
                        :args [subject predicate {:var "value"}]}]}]}}))
       (map first)
       set))

(defn notification-subjects []
  (->> (:ok
        (op {:op :query
             :query
             {:find "notification"
              :rules
              [{:head {:rel "notification" :args [{:var "n"}]}
                :body [{:rel "triple"
                        :args [{:var "n"} "kind" "notification"]}]}]}}))
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

(def original-send-op-for-log north.coord/send-op-for-log)
(defn isolated-send-op-for-log [requested-port _ operation]
  (original-send-op-for-log requested-port canonical-log operation))
(defn isolated-send-op [requested-port operation]
  (original-send-op-for-log requested-port canonical-log operation))

(with-redefs [north.coord/send-op isolated-send-op
              north.coord/send-op-for-log isolated-send-op-for-log]
 (try
  (let [started? (await-up)]
    (check "throwaway coordinator starts" started?)
    (when-not started?
      (fail-daemon-boot!)))
  (fact! "@thread-attention" "title" "Attention parent")
  (fact! "@thread-attention" "kind" "thread")
  (fact! "@wrong-kind" "title" "Not a thread")
  (fact! "@wrong-kind" "kind" "document")

  (let [request (atom nil)
        page
        (with-redefs
          [north.coord/query-page
           (fn [_ query limit after]
             (reset! request {:query query :limit limit :after after})
             {:ok [] :more true})]
          (pending-attention-event-intents port nil))
        errored
        (with-redefs
          [north.coord/query-page
           (fn [& _] {:error ["fixture failure"] :more false})]
          (thrown #(pending-attention-event-intents port nil)))
        malformed
        (with-redefs
          [north.coord/query-page
           (fn [& _] {:ok [["one-column-unscoped"]] :more false})]
          (thrown #(pending-attention-event-intents port nil)))]
    (check "terminal outbox reads one fixed page and reports remaining work"
           (and (= attention-event-reconcile-limit (:limit @request))
                (nil? (:after @request))
                (true? (:more page))
                (empty? (:intents page))))
    (check "terminal outbox query errors and malformed rows fail visibly"
           (and (= :concern-attention-outbox-query-failed
                   (:type (ex-data errored)))
                (= :malformed-concern-attention-outbox-page
                   (:type (ex-data malformed))))))

  (let [legacy (run-concern "declare" "agent-a" "/tmp/no-code"
                            "legacy declaration" "src/shared.clj")
        legacy-id (concern-id legacy)
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
        legacy-state (meta-of port legacy-id)
        entered-rows (notification-rows)]
    (check "legacy four-position declaration remains valid"
           (and (zero? (:exit legacy)) legacy-id))
    (check "wrong-kind about is rejected before any concern mutation"
           (and (= 2 (:exit invalid))
                (= before-invalid after-invalid)))
    (check "validated about is stored as the exact thread ref"
           (and (zero? (:exit anchored))
                (= "@thread-attention"
                   (:value (op {:op :resolved
                                :te anchored-id
                                :p "about"})))))
    (check "later declaration discovers the earlier active overlap"
           (and (= 1 (count (:overlaps discovered)))
                (= #{legacy-id anchored-id}
                   (set (:source-concerns overlap)))
                (= ["src/shared.clj"] (:shared overlap))))
    (check "entered overlap publishes two recipient-scoped notifications"
           (and (= 2 (count entered-rows))
                (= 2 (count (set (map :id entered-rows))))
                (= #{"@agent-a" "@agent-b"}
                   (set (map :recipient entered-rows)))
                (= 1 (count (set (map :event-key entered-rows))))
                (every? #(= "overlap-entered" (:attention-kind %))
                        entered-rows)
                (every? #(= "notify" (:delivery %)) entered-rows)
                (every? #(= "@thread-attention" (:about %)) entered-rows)
                (every? #(not (str/includes? (:body %) "src/shared.clj"))
                        entered-rows)))

    (let [forward (canonical-overlap legacy-state anchored-state
                                     #{"src/z.clj" "src/a.clj"} "path")
          reverse (canonical-overlap anchored-state legacy-state
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

    (let [likely (run-concern "status" anchored-id "likely-to-land")
          after-likely (notification-rows)
          repeated (run-concern "status" anchored-id "likely-to-land")
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
                  (= intents settled-after)
                  (empty? (:intents pending-after))
                  (= after-reconcile (notification-rows))
                  (= 2 (get current-kinds "overlap-entered"))
                  (= 1 (get current-kinds "likely-to-land"))
                  (= 1 (get current-kinds "overlap-left"))
                  (empty? (:overlaps (path-overlap-data port legacy-id))))
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
              (attention-events-for-transition abandoned abandoned [pair])))))

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
      (check "hidden reactor retirement boundary emits and settles overlap-left"
             (and (zero? (:exit seed))
                  (zero? (:exit retired))
                  (str/includes? (:out retired) ":status :committed")
                  (:abandoned state)
                  (= 1 (count left))
                  (= "@retire-peer" (:recipient (first left)))
                  (= intents settled)
                  (= 1 (count intents))))))

  (let [reactor-source (slurp (str root "/cli/north-reactor.clj"))]
    (check "reactor retirement calls the concern boundary and never appends terminal directly"
           (and
            (str/includes?
             reactor-source
             "(retire-stale-concern! c)")
            (not
             (re-find
              #"append!\s+port\s+c\s+\"reached\"\s+\"abandoned-stale\""
              reactor-source))
            (str/includes? reactor-source "\"@notification:\"")
            (str/includes? reactor-source "\"@subscription:\""))))

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
    (cleanup))))

(let [failed (remove second @checks)]
  (println
   (format "\nconcern attention seam: %d / %d PASS"
           (- (count @checks) (count failed))
           (count @checks)))
  (System/exit (if (empty? failed) 0 1)))
