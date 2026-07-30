#!/usr/bin/env bb
;; Durable observer attention against a throwaway strict-log-fenced coordinator.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (str (System/getProperty "user.home") "/code/fram/main")))
(def attention-cli (str root "/cli/attention-cli.clj"))
(def attention-core (str root "/cli/attention.clj"))
(def checks (atom []))

(defn check [label ok? detail]
  (swap! checks conj {:label label :ok (boolean ok?) :detail detail}))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket
                (java.net.InetSocketAddress. "127.0.0.1" (int port))
                100)
      true)
    (catch Exception _ false)))

(defn await-daemon [port]
  (loop [attempt 0]
    (cond
      (port-open? port) true
      (>= attempt 300) false
      :else (do (Thread/sleep 250) (recur (inc attempt))))))

(defn coordinator-op [port log request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write writer
              (.getBytes
               (str (pr-str {:op :for-log
                             :expected-log log
                             :request request})
                    "\n")
               java.nio.charset.StandardCharsets/UTF_8))
      (.flush writer)
      (edn/read-string (.readLine reader)))))

(defn assert-fact! [port log subject predicate value]
  (let [result
        (coordinator-op
         port log {:op :assert :te subject :p predicate :r value})]
    (when (:reject result)
      (throw (ex-info "fixture assertion rejected" result)))
    result))

(defn retract-fact! [port log subject predicate value]
  (let [result
        (coordinator-op
         port log {:op :retract :te subject :p predicate :r value})]
    (when (:reject result)
      (throw (ex-info "fixture retraction rejected" result)))
    result))

(defn value-of [port log subject predicate]
  (:value
   (coordinator-op
    port log {:op :resolved :te subject :p predicate})))

(defn values-of [port log subject predicate]
  (set
   (:values
    (coordinator-op
     port log {:op :resolved :te subject :p predicate}))))

(defn current-version [port log]
  (:version (coordinator-op port log {:op :version})))

(defn base-env [port log agent-id]
  {"FRAM_LOG" log
   "FRAM_TELEMETRY_LOG"
   (.getCanonicalPath
    (io/file (.getParentFile (io/file log)) "telemetry.log"))
   "NORTH_PORT" (str port)
   "AGENT_ID" agent-id
   "NORTH_AUTHOR" "fallback-author"
   "NO_COLOR" "1"})

(defn run-attention [port log agent-id & args]
  (apply proc/shell
         {:continue true
          :out :string
          :err :string
          :extra-env (base-env port log agent-id)}
         "bb" attention-cli (str port) args))

(def publisher-expression
  (str
   "(require '[clojure.edn :as edn]) "
   "(let [[path port spec] *command-line-args*] "
   "(System/setProperty \"babashka.file\" path) "
   "(load-file path) "
   "(let [publisher (ns-resolve 'north.attention 'publish-notification!)] "
   "(println (publisher (parse-long port) (edn/read-string spec)))))"))

(defn run-publisher [port log spec]
  (proc/shell
   {:continue true
    :out :string
    :err :string
    :extra-env (base-env port log "agent-b")}
   "bb" "-e" publisher-expression "--"
   attention-core (str port) (pr-str spec)))

(defn subscription-id [result]
  (second (re-find #"via (@subscription:[^ \n]+)" (:out result))))

(defn notification-ids [text]
  (set (re-seq #"@notification:[0-9a-f]{64}" text)))

(defn utf8-size [value]
  (alength
   (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn log-events [log]
  (->> (str/split-lines (slurp log))
       (remove str/blank?)
       (mapv edn/read-string)))

(defn notification-predicates [log notification]
  (->> (log-events log)
       (filter #(and (= notification (:l %)) (= "assert" (:op %))))
       (mapv :p)))

(defn notification-read-transactions [log notification]
  (->> (log-events log)
       (filter #(and (= notification (:l %))
                     (= "assert" (:op %))
                     (#{"read_by" "read_at"} (:p %))))
       (map :tx)
       set))

(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw
   (ex-info "Fram checkout not found; set FRAM_TEST_CHECKOUT"
            {:fram fram})))

(let [port (free-port)
      temp
      (.toFile
       (java.nio.file.Files/createTempDirectory
        "north-attention" (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file temp "coordination.log")
      _ (spit facts "")
      log (.getCanonicalPath facts)
      telemetry (io/file temp "telemetry.log")
      _telemetry (spit telemetry "")
      telemetry-log (.getCanonicalPath telemetry)
      single-valued
      (str "title display_name kind entity_kind subscriber about delivery "
           "start_version cursor_version start_offset cursor_offset cursor_anchor "
           "created_at ended_at end_version end_offset end_anchor from subject body "
           "sent_at to target recipient attention_kind source_version subscription "
           "event_key read_at")
      daemon
      (proc/process
       {:dir fram
        :out :string
        :err :string
        :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"
                    "FRAM_TELEMETRY_LOG" telemetry-log
                    "FRAM_SINGLE_VALUED" single-valued}}
       "bb" "-cp" "out" "coord_daemon.clj"
       "serve-flat" (str port) log)]
  (try
    (let [started? (await-daemon port)]
      (check "throwaway strict coordinator starts" started? (str "port=" port))
      (when started?
        (let [role "@role:reviewer"
              role-2 "@role:observer"
              agent-a "@agent:agent-a"
              agent-b "@agent:agent-b"
              thread "@thread:attention"
              _ (doseq [[subject predicate value]
                        [[role "kind" "role"]
                         [role-2 "kind" "role"]
                         [agent-a "kind" "agent"]
                         [agent-b "kind" "agent"]
                         [agent-a "holds" role]
                         [thread "title" "Attention contract"]
                         [thread "kind" "thread"]]]
                  (assert-fact! port log subject predicate value))
              before-follow (current-version port log)
              first-follow (run-attention port log "agent-a" "follow" thread)
              subscription (subscription-id first-follow)
              repeated-follow (run-attention port log "agent-a" "follow" thread)
              repeated-subscription (subscription-id repeated-follow)]
          (check
           "default principal is the unique held role and follow is idempotent"
           (and (zero? (:exit first-follow))
                (zero? (:exit repeated-follow))
                subscription
                (= subscription repeated-subscription)
                (= role (value-of port log subscription "subscriber"))
                (= thread (value-of port log subscription "about"))
                (= "subscription" (value-of port log subscription "kind"))
                (= "subscription" (value-of port log subscription "entity_kind"))
                (= (str before-follow)
                   (value-of port log subscription "start_version"))
                (= (value-of port log subscription "start_offset")
                   (value-of port log subscription "cursor_offset"))
                (not (str/blank?
                      (value-of port log subscription "cursor_anchor")))
                (= "reviewer" (value-of port log subscription "target"))
                (nil? (value-of port log subscription "to")))
           (pr-str {:first first-follow :repeat repeated-follow
                    :subscription subscription}))

          (let [before-invalid-options (.length facts)
                invalid-follow-option
                (run-attention
                 port log "agent-a" "follow" thread "--mark-read")
                invalid-notification-option
                (run-attention
                 port log "agent-a" "notifications"
                 "--delivery" "notify")]
            (check
             "verb-specific option validation rejects ignored intent before mutation"
             (and (= 2 (:exit invalid-follow-option))
                  (= 2 (:exit invalid-notification-option))
                  (str/includes?
                   (:err invalid-follow-option)
                   "follow does not accept --mark-read")
                  (str/includes?
                   (:err invalid-notification-option)
                   "notifications does not accept --delivery")
                  (= before-invalid-options (.length facts)))
             (pr-str
              {:follow invalid-follow-option
               :notifications invalid-notification-option})))

          ;; No listener is running: the later read must replay this durable change.
          (assert-fact! port log thread "progress" "offline progress")
          (let [first-read
                (run-attention port log "agent-a" "notifications")
                ids (notification-ids (:out first-read))
                notification (first ids)
                cursor-after-first
                (value-of port log subscription "cursor_offset")
                repeated-read
                (run-attention port log "agent-a" "notifications")
                repeated-ids (notification-ids (:out repeated-read))
                cursor-after-repeat
                (value-of port log subscription "cursor_offset")]
            (check
             "offline replay materializes exactly one deterministic unread notification"
             (and (zero? (:exit first-read))
                  (= 1 (count ids))
                  (= ids repeated-ids)
                  (= cursor-after-first cursor-after-repeat)
                  (= role (value-of port log notification "recipient"))
                  (= thread (value-of port log notification "about"))
                  (= "progress" (value-of port log notification "attention_kind"))
                  (= "notification" (value-of port log notification "kind"))
                  (= "notification" (value-of port log notification "entity_kind"))
                  (= "target" (last (notification-predicates log notification)))
                  (= "reviewer" (value-of port log notification "target"))
                  (nil? (value-of port log notification "to")))
             (pr-str {:first first-read :repeat repeated-read :ids ids}))

            (let [marked
                  (run-attention
                   port log "agent-a" "notifications" "--mark-read")
                  unread-after
                  (run-attention port log "agent-a" "notifications")]
              (check
               "mark-read acknowledges exactly the displayed notification"
               (and (= ids (notification-ids (:out marked)))
                    (empty? (notification-ids (:out unread-after)))
                    (= #{role} (values-of port log notification "read_by"))
                    (not (str/blank?
                          (value-of port log notification "read_at")))
                    (= 1
                       (count
                        (notification-read-transactions log notification))))
               (pr-str {:marked marked :after unread-after}))))

          (retract-fact! port log agent-a "holds" role)
          (assert-fact! port log agent-b "holds" role)
          (let [following-after-handoff
                (run-attention port log "agent-b" "following")]
            (check
             "role principal and its subscription survive agent replacement"
             (and (zero? (:exit following-after-handoff))
                  (str/includes? (:out following-after-handoff) subscription))
             (pr-str following-after-handoff)))

          ;; Physical append order, not :tx magnitude, is replay authority. The
          ;; first semantic record crosses the 64 KiB reader boundary and the
          ;; second shares its transaction/kind, proving streaming grouping.
          (let [chunk-spanning-progress
                (str "later physical append with lower tx "
                     (apply str (repeat 70000 "x")))]
            (spit log
                  (str
                   (pr-str {:tx 999999 :op "assert" :l thread :p "progress"
                            :r "merge replay is not semantic"
                            :frame "merge"})
                   "\n"
                   (pr-str {:tx 1 :op "assert" :l thread :p "progress"
                            :r chunk-spanning-progress
                            :frame "test"})
                   "\n"
                   (pr-str {:tx 1 :op "assert" :l thread :p "progress"
                            :r "same transaction after the chunk boundary"
                            :frame "test"})
                   "\n")
                  :append true))
          (let [lower-tx-read
                (run-attention port log "agent-b" "notifications")
                lower-tx-ids (notification-ids (:out lower-tx-read))
                lower-tx-notification (first lower-tx-ids)
                lower-tx-body
                (value-of port log lower-tx-notification "body")
                lower-tx-repeat
                (run-attention port log "agent-b" "notifications")]
            (check
             "chunked replay preserves a lower-tx group and bounds its deterministic body"
             (and (zero? (:exit lower-tx-read))
                  (zero? (:exit lower-tx-repeat))
                  (= 1 (count lower-tx-ids))
                  (= lower-tx-ids
                     (notification-ids (:out lower-tx-repeat)))
                  (not (str/includes? (:out lower-tx-read)
                                      "merge replay is not semantic"))
                  (str/includes? lower-tx-body "total-events=2")
                  (str/includes? lower-tx-body "event-digest=")
                  (<= (utf8-size lower-tx-body) 65536)
                  (= "1" (value-of port log lower-tx-notification
                                   "source_version")))
            (pr-str lower-tx-read))
            (run-attention port log "agent-b" "notifications" "--mark-read"))

          ;; Imported tails may reuse a display tx. An intervening physical row
          ;; still separates two occurrences with identical semantic content.
          (spit log
                (str
                 (pr-str {:tx 7 :op "assert" :l thread :p "progress"
                          :r "same tx reused after an intervening row"
                          :frame "import"})
                 "\n"
                 (pr-str {:tx 8 :op "assert" :l "@thread:separator"
                          :p "progress" :r "physical separator"
                          :frame "import"})
                 "\n"
                 (pr-str {:tx 7 :op "assert" :l thread :p "progress"
                          :r "same tx reused after an intervening row"
                          :frame "import"})
                 "\n")
                :append true)
          (let [reused-tx-read
                (run-attention port log "agent-b" "notifications")
                reused-tx-ids (notification-ids (:out reused-tx-read))
                reused-tx-repeat
                (run-attention port log "agent-b" "notifications")]
            (check
             "reused tx separated by another physical row remains two occurrences"
             (and (zero? (:exit reused-tx-read))
                  (zero? (:exit reused-tx-repeat))
                  (= 2 (count reused-tx-ids))
                  (= reused-tx-ids
                     (notification-ids (:out reused-tx-repeat)))
                  (every?
                   #(= "7" (value-of port log % "source_version"))
                   reused-tx-ids))
             (pr-str
              {:first reused-tx-read :repeat reused-tx-repeat}))
            (run-attention
             port log "agent-b" "notifications" "--mark-read"))

          ;; Content alone is not an event identity. A later physical occurrence
          ;; of the same assert must not collide with the first occurrence.
          (assert-fact! port log thread "progress" "repeatable transition")
          (let [first-assert-read
                (run-attention port log "agent-b" "notifications")
                first-assert-ids (notification-ids (:out first-assert-read))
                _ (run-attention
                   port log "agent-b" "notifications" "--mark-read")
                _ (retract-fact!
                   port log thread "progress" "repeatable transition")
                retract-read
                (run-attention port log "agent-b" "notifications")
                retract-ids (notification-ids (:out retract-read))
                _ (run-attention
                   port log "agent-b" "notifications" "--mark-read")
                _ (assert-fact!
                   port log thread "progress" "repeatable transition")
                second-assert-read
                (run-attention port log "agent-b" "notifications")
                second-assert-ids
                (notification-ids (:out second-assert-read))
                cursor-after-second
                (value-of port log subscription "cursor_offset")
                repeat-read
                (run-attention port log "agent-b" "notifications")
                cursor-after-repeat
                (value-of port log subscription "cursor_offset")]
            (check
             "identical later transition has a distinct occurrence id and repeat sync dedupes"
             (and (zero? (:exit first-assert-read))
                  (zero? (:exit retract-read))
                  (zero? (:exit second-assert-read))
                  (zero? (:exit repeat-read))
                  (= 1 (count first-assert-ids))
                  (= 1 (count retract-ids))
                  (= 1 (count second-assert-ids))
                  (not= first-assert-ids second-assert-ids)
                  (= second-assert-ids
                     (notification-ids (:out repeat-read)))
                  (= cursor-after-second cursor-after-repeat)
                  (str/includes?
                   (:out second-assert-read) "repeatable transition"))
             (pr-str
              {:first first-assert-read
               :retract retract-read
               :second second-assert-read
               :repeat repeat-read}))
            (run-attention
             port log "agent-b" "notifications" "--mark-read"))

          (assert-fact! port log thread "progress" "pending before unfollow")
          (let [unfollow
                (run-attention port log "agent-b" "unfollow" thread)]
            (assert-fact! port log thread "progress" "after unfollow")
            (let [pending
                  (run-attention port log "agent-b" "notifications")]
              (check
               "unfollow replays its captured boundary and suppresses later events"
               (and (zero? (:exit unfollow))
                    (= "subscription"
                       (value-of port log subscription "kind"))
                    (not (str/blank?
                          (value-of port log subscription "ended_at")))
                    (not (str/blank?
                          (value-of port log subscription "end_offset")))
                    (nil? (value-of port log subscription "target"))
                    (= 1 (count (notification-ids (:out pending))))
                    (str/includes? (:out pending) "pending before unfollow")
                    (not (str/includes? (:out pending) "after unfollow"))
                    (= (value-of port log subscription "end_version")
                       (value-of port log subscription "cursor_version"))
                    (= "retract"
                       (:op
                        (last
                         (filter
                          #(and (= subscription (:l %))
                                (= "target" (:p %)))
                          (log-events log))))))
               (pr-str {:unfollow unfollow :pending pending})))
            ;; Simulate a death after the authoritative end batch but before its
            ;; transport cleanup. Repeating unfollow must heal the stale target.
            (assert-fact! port log subscription "target" "reviewer")
            (let [healed
                  (run-attention
                   port log "agent-b" "unfollow" thread)]
              (check
               "unfollow retry heals an ended subscription target"
               (and (zero? (:exit healed))
                    (str/includes? (:out healed) "not following")
                    (nil? (value-of port log subscription "target")))
               (pr-str healed)))
            (run-attention port log "agent-b" "notifications" "--mark-read"))

          (let [spec {:event-key "attention-integration-publisher-v1"
                      :to role
                      :about thread
                      :attention-kind "changed"
                      :subject "Manual attention"
                      :body "Review the durable event."
                      :source-version (current-version port log)
                      :source-concerns ["@concern:attention"]}
                first-publish (run-publisher port log spec)
                repeat-publish (run-publisher port log spec)
                second-recipient
                (run-publisher port log (assoc spec :to role-2))
                collision
                (run-publisher
                 port log (assoc spec :body "Different immutable body"))
                control-key
                (run-publisher
                 port log (assoc spec :event-key "invalid\ncontrol"))
                control-recipient
                (run-publisher
                 port log (assoc spec :to "@invalid\nrecipient"))
                published-id
                (first (notification-ids (:out first-publish)))]
            (check
             "publisher fans one semantic key out per recipient and collision-fails per recipient"
             (and (zero? (:exit first-publish))
                  (zero? (:exit repeat-publish))
                  (zero? (:exit second-recipient))
                  (= (notification-ids (:out first-publish))
                     (notification-ids (:out repeat-publish)))
                  (= 1 (count (notification-ids (:out first-publish))))
                  (= 1 (count (notification-ids (:out second-recipient))))
                  (not=
                   (notification-ids (:out first-publish))
                   (notification-ids (:out second-recipient)))
                  (not (zero? (:exit collision)))
                  (str/includes? (:err collision)
                                 "collides with different facts")
                  (not (zero? (:exit control-key)))
                  (str/includes? (:err control-key)
                                 "contains control characters")
                  (not (zero? (:exit control-recipient)))
                  (str/includes? (:err control-recipient)
                                 "recipient must be an entity reference")
                  (= "target"
                     (last (notification-predicates log published-id)))
                  (= "reviewer" (value-of port log published-id "target"))
                  (nil? (value-of port log published-id "to"))
                  (= role (value-of port log published-id "recipient")))
             (pr-str {:first first-publish :repeat repeat-publish
                      :second second-recipient :collision collision
                      :control-key control-key
                      :control-recipient control-recipient}))

          (let [zero-role (run-attention port log "agent-without-role" "following")
                _ (assert-fact! port log "@agent:ambiguous" "kind" "agent")
                _ (assert-fact! port log "@agent:ambiguous" "holds" role)
                _ (assert-fact! port log "@agent:ambiguous" "holds" role-2)
                many-role (run-attention port log "ambiguous" "following")]
            (check
             "default principal fails for zero or multiple held roles"
             (and (not (zero? (:exit zero-role)))
                  (not (zero? (:exit many-role)))
                  (str/includes? (:err zero-role) "pass --as")
                  (str/includes? (:err many-role) "pass --as"))
             (pr-str {:zero zero-role :many many-role})))))))
    (finally
      (try (proc/destroy-tree daemon) (catch Exception _ nil))
      (try @daemon (catch Exception _ nil))
      (doseq [file (reverse (file-seq temp))]
        (io/delete-file file true)))))

(doseq [{:keys [label ok detail]} @checks]
  (println (format "  [%s] %s%s"
                   (if ok "PASS" "FAIL")
                   label
                   (if ok "" (str "\n    " detail)))))
(let [failed (remove :ok @checks)]
  (println (format "\n%d/%d passed"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (when (seq failed)
    (System/exit 1)))
