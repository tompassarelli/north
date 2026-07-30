#!/usr/bin/env bb
;; Native `north listen` reachability is a renewable, generation-fenced lease.
;; Durable armed state alone is never a routing authority.
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
(def listener-cli (str root "/cli/north-listen.clj"))
(def message-cli (str root "/cli/msg-cli.clj"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(def peek-cli (str root "/cli/inbox-peek.clj"))
(def checks (atom []))
(def children (atom []))

(defn check [label value]
  (swap! checks conj [label (boolean value)])
  (println (if value (str "PASS " label) (str "FAIL " label))))

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

(defn eventually [predicate]
  (loop [attempt 0]
    (cond
      (try (boolean (predicate)) (catch Exception _ false)) true
      (>= attempt 400) false
      :else (do (Thread/sleep 25) (recur (inc attempt))))))

(defn coordinator-op [port log request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write
       writer
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

(defn resolved [port log subject predicate]
  (:value
   (coordinator-op
    port log {:op :resolved :te subject :p predicate})))

(defn values-of [port log subject predicate]
  (set
   (:values
    (coordinator-op
     port log {:op :resolved :te subject :p predicate}))))

(defn decode-lease [value]
  (when (string? value)
    (let [[holder expiry epoch] (str/split value #"\|" -1)]
      (when (and (not (str/blank? holder))
                 (re-matches #"[0-9]+" expiry)
                 (re-matches #"[0-9]+" epoch))
        {:holder holder
         :exp (parse-long expiry)
         :epoch (parse-long epoch)}))))

(defn listener-snapshot [port log agent]
  (let [node (str "@agent:" agent)]
    {:kind (resolved port log node "kind")
     :state (resolved port log node "live_input_state")
     :generation (resolved port log node "live_input_epoch")
     :lease (decode-lease
             (resolved port log
                       (str "@lease:listener:" agent) "lease"))}))

(defn matching-live-generation? [snapshot]
  (let [lease (:lease snapshot)]
    (and (= "session" (:kind snapshot))
         (= "armed" (:state snapshot))
         (map? lease)
         (= (:generation snapshot) (:holder lease))
         (> (:exp lease) (System/currentTimeMillis)))))

(defn isolated-env [port log]
  {"FRAM_LOG" log
   "FRAM_TELEMETRY_LOG"
   (.getCanonicalPath
    (io/file (.getParentFile (io/file log)) "telemetry.log"))
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)
   "NORTH_LISTENER_LEASE_TTL_MS" "600"
   "NORTH_LISTEN_INITIAL_BACKOFF_MS" "20"
   "NORTH_LISTEN_MAX_BACKOFF_MS" "50"})

(defn start-listener! [port log output agent & flags]
  (let [command (into ["bb" listener-cli (str port) agent] flags)
        builder (ProcessBuilder. ^java.util.List command)
        _ (.directory builder (io/file root))
        _ (.redirectErrorStream builder true)
        _ (.redirectOutput builder (io/file output))
        _ (doseq [[name value] (isolated-env port log)]
            (.put (.environment builder) name value))
        process (.start builder)]
    (swap! children conj process)
    process))

(defn stop-child! [process force?]
  (when process
    (when (.isAlive ^Process process)
      (if force?
        (.destroyForcibly ^Process process)
        (.destroy ^Process process))
      (.waitFor ^Process process 3 java.util.concurrent.TimeUnit/SECONDS))
    (swap! children
           (fn [current]
             (vec (remove #(identical? % process) current))))))

(defn run-message [port log & args]
  (apply proc/sh
         {:continue true
          :out :string
          :err :string
          :extra-env (isolated-env port log)}
         "bb" message-cli (str port) args))

(defn run-presence [port log & args]
  (apply proc/sh
         {:continue true
          :out :string
          :err :string
          :extra-env (isolated-env port log)}
         "bb" presence-cli (str port) args))

(defn run-peek [port log runtime agent]
  (proc/sh
   {:continue true
    :out :string
    :err :string
    :extra-env
    (assoc (isolated-env port log)
           "XDG_RUNTIME_DIR" (.getCanonicalPath (io/file runtime)))}
   "bb" peek-cli (str port) agent))

(defn output-has? [file text]
  (and (.isFile (io/file file))
       (str/includes? (slurp file) text)))

(let [port (free-port)
      tmp
      (.toFile
       (java.nio.file.Files/createTempDirectory
        "north-native-listener-liveness"
        (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "facts.log")
      _ (spit facts "")
      log (.getCanonicalPath facts)
      telemetry (io/file tmp "telemetry.log")
      _ (spit telemetry "")
      daemon
      (proc/process
       {:dir fram
        :out :string
        :err :string
        :extra-env
        (merge
         (isolated-env port log)
         {"FRAM_REQUIRE_LOG_FENCE" "1"
          "FRAM_SINGLE_VALUED"
          (str "kind live_input_state live_input_epoch "
               "from to subject body sent_at acked_at")})}
       "bb" "-cp" "out" "coord_daemon.clj"
       "serve-flat" (str port) log)]
  (try
    (check "throwaway Fram coordinator starts"
           (eventually #(port-open? port)))
    (let [agent "lease-only-session"
          runtime (doto (io/file tmp "lease-only-runtime") .mkdirs)]
      (check "lease-only native session registers without a listener"
             (and
              (zero?
               (:exit
                (run-presence
                 port log "register" agent "/tmp/lease-only-session" agent)))
              (nil? (:lease (listener-snapshot port log agent)))))
      (let [sent
            (run-message
             port log "send" "test-sender" agent
             "lease-only delivery"
             "PostToolUse consumes this durable mail without a listener")
            message
            (some->> (:out sent)
                     (re-find #"sent (@msg:[^ ]+) ->")
                     second)]
        (check "direct send admits a live renewable session lease"
               (and (zero? (:exit sent)) message))
        (let [peek (run-peek port log runtime agent)]
          (check "lease-only mail is printed and acknowledged by inbox peek"
                 (and
                  (zero? (:exit peek))
                  (str/includes? (:out peek) "lease-only delivery")
                  (= #{agent} (values-of port log message "acked_by"))
                  (nil? (:lease (listener-snapshot port log agent))))))))

    (let [agent "wrong-holder-session"
          lease
          (str "somebody-else|"
               (+ (System/currentTimeMillis) 60000)
               "|1")]
      (assert-fact!
       port log (str "@lease:session:" agent) "lease" lease)
      (let [rejected
            (run-message
             port log "send" "test-sender" agent
             "wrong-holder rejection"
             "a future expiry does not confer another holder's authority")]
        (check "direct admission rejects a future lease held by another control"
               (and
                (= 2 (:exit rejected))
                (str/includes?
                 (str (:out rejected) (:err rejected))
                 "has no live presence")))))

    (let [agent "native-listener-generation"
          node (str "@agent:" agent)
          crash-output (io/file tmp "crash-listener.log")]
      (assert-fact! port log node "kind" "session")
      (let [crashed (start-listener! port log crash-output agent)]
        (check "native listener publishes only a matching live generation"
               (eventually
                #(matching-live-generation?
                  (listener-snapshot port log agent))))
        (stop-child! crashed true)
        (let [stale (listener-snapshot port log agent)]
          (check "SIGKILL leaves descriptive armed state for the lease to bound"
                 (and (= "armed" (:state stale))
                      (map? (:lease stale))))
          (check "crashed listener becomes unreachable after the bounded TTL"
                 (eventually
                  #(let [lease (:lease (listener-snapshot port log agent))]
                     (and lease
                          (<= (:exp lease)
                              (System/currentTimeMillis))))))
          (let [rejected
                (run-message
                 port log "send" "test-sender" agent
                 "must reject stale generation" "no listener owns this route")]
            (check "direct send rejects a crashed stale armed generation"
                   (and (= 2 (:exit rejected))
                        (str/includes?
                         (str (:out rejected) (:err rejected))
                         "has no live presence"))))
          (let [successor-output (io/file tmp "successor-listener.log")
                successor
                (start-listener!
                 port log successor-output agent "--once" "--ack")]
            (check "successor arms with a fresh generation after expiry"
                   (eventually
                    #(let [fresh (listener-snapshot port log agent)]
                       (and (matching-live-generation? fresh)
                            (not= (:holder (:lease stale))
                                  (:holder (:lease fresh)))))))
            (let [fresh (listener-snapshot port log agent)
                  stale-freeze
                  (coordinator-op
                   port log
                   {:op :assert-with-fence
                    :res (str "listener:" agent)
                    :holder (:holder (:lease stale))
                    :epoch (:epoch (:lease stale))
                    :te node
                    :p "live_input_state"
                    :r "frozen"})]
              (check "predecessor fence cannot freeze its successor"
                     (and (= :fence-lost (:reject stale-freeze))
                          (= "armed"
                             (:state
                              (listener-snapshot port log agent)))
                          (= (:holder (:lease fresh))
                             (:generation
                              (listener-snapshot port log agent))))))
            (let [sent
                  (run-message
                   port log "send" "test-sender" agent
                   "generation delivery" "listener received the exact mail")
                  message
                  (some->> (:out sent)
                           (re-find #"sent (@msg:[^ ]+) ->")
                           second)]
              (check "direct send admits the exact armed successor"
                     (and (zero? (:exit sent)) message))
              (check "listener receives, acknowledges, and exits one-shot"
                     (eventually
                      #(and (not (.isAlive ^Process successor))
                            message
                            (= #{agent}
                               (values-of port log message "acked_by"))
                            (output-has? successor-output
                                         "listener received the exact mail"))))
              (check "clean one-shot exit freezes and releases its generation"
                     (eventually
                      #(let [snapshot (listener-snapshot port log agent)]
                         (and (= "frozen" (:state snapshot))
                              (nil? (:lease snapshot)))))))))))

    (let [agent "managed-listener-route"
          node (str "@agent:" agent)
          generation "00000000-0000-4000-8000-000000000299"
          output (io/file tmp "managed-listener.log")]
      (assert-fact! port log node "kind" "lane")
      (assert-fact! port log node "live_input_state" "armed")
      (assert-fact! port log node "live_input_epoch" generation)
      (let [managed (start-listener! port log output agent)]
        (check "managed listener still establishes its ordinary subscription"
               (eventually #(output-has? output "listening")))
        (let [snapshot (listener-snapshot port log agent)]
          (check "north listen never mutates managed SDK route authority"
                 (and (= "armed" (:state snapshot))
                      (= generation (:generation snapshot))
                      (nil? (:lease snapshot)))))
        (stop-child! managed true)))
    (finally
      (doseq [child @children] (stop-child! child true))
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [failed (remove second @checks)]
  (println (str "native listener liveness integration: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
