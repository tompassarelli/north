#!/usr/bin/env bb
;; Durable observer attention stays orthogonal to the listener's legacy
;; one-shot mail and explicit-watch contracts.
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
(def attention-cli (str root "/cli/attention-cli.clj"))
(def checks (atom []))
(def children (atom []))

(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)]))

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
      (predicate) true
      (>= attempt 400) false
      :else (do (Thread/sleep 25) (recur (inc attempt))))))

(defn await-daemon [predicate]
  (loop [attempt 0]
    (cond
      (predicate) true
      (>= attempt 300) false
      :else (do (Thread/sleep 250) (recur (inc attempt))))))

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

(defn values-of [port log subject predicate]
  (set
   (:values
    (coordinator-op
     port log {:op :resolved :te subject :p predicate}))))

(defn notification-ids [port log recipient]
  (->> (:ok
        (coordinator-op
         port log
         {:op :query
          :query
          {:find "notification"
           :rules
           [{:head {:rel "notification" :args [{:var "notification"}]}
             :body
             [{:rel "triple"
               :args [{:var "notification"} "kind" "notification"]}
              {:rel "triple"
               :args [{:var "notification"} "recipient" recipient]}]}]}}))
       (map first)
       set))

(defn isolated-env [port log]
  {"FRAM_LOG" log
   "FRAM_TELEMETRY_LOG"
   (.getCanonicalPath
    (io/file (.getParentFile (io/file log)) "telemetry.log"))
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})

(defn run-attention [port log & args]
  (apply
   proc/shell
   {:continue true
    :out :string
    :err :string
    :extra-env (isolated-env port log)}
   "bb" attention-cli (str port) args))

(defn start-listener! [port log output agent & flags]
  (let [command
        (into ["bb" listener-cli (str port) agent] flags)
        builder (ProcessBuilder. ^java.util.List command)
        _ (.directory builder (io/file root))
        _ (.redirectErrorStream builder true)
        _ (.redirectOutput builder (io/file output))
        _ (doseq [[name value] (isolated-env port log)]
            (.put (.environment builder) name value))
        process (.start builder)]
    (swap! children conj process)
    process))

(defn output-has? [file text]
  (and (.isFile (io/file file))
       (str/includes? (slurp file) text)))

(defn stop-child! [process]
  (when process
    (when (.isAlive ^Process process)
      (.destroyForcibly ^Process process)
      (.waitFor ^Process process 2 java.util.concurrent.TimeUnit/SECONDS))
    (swap! children
           (fn [current]
             (vec (remove #(identical? % process) current))))))

(let [port (free-port)
      tmp
      (.toFile
       (java.nio.file.Files/createTempDirectory
        "north-attention-listener"
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
          (str
           "title display_name kind entity_kind subscriber about delivery "
           "start_version cursor_version start_offset cursor_offset "
           "cursor_anchor end_version end_offset end_anchor created_at ended_at "
           "from subject body "
           "sent_at to target recipient attention_kind source_version subscription "
           "event_key acked_at read_at")})}
       "bb" "-cp" "out" "coord_daemon.clj"
       "serve-flat" (str port) log)]
  (try
    (let [started? (await-daemon #(port-open? port))]
      (check "throwaway Fram coordinator starts" started?)
      (when started?
        (let [agent "attention-listener"
              agent-node (str "@agent:" agent)
              role "@role:attention-reviewer"
              followed "@thread:followed"
              follow-log (io/file tmp "follow-listener.log")]
          (assert-fact! port log role "title" "Attention reviewer")
          (assert-fact! port log agent-node "holds" role)
          (assert-fact! port log followed "title" "Followed thread")
          (assert-fact! port log followed "kind" "thread")
          (let [follow
                (run-attention
                 port log "follow" followed "--as" role "--delivery" "inbox")]
            (check "stable role follow is accepted"
                   (and (zero? (:exit follow))
                        (str/includes? (:out follow) "following"))))
          (let [listener
                (start-listener!
                 port log follow-log agent "--once" "--ack" "--scoped")]
            (check "scoped one-shot listener arms with the followed thread"
                   (eventually #(output-has? follow-log "1 followed thread")))
            (assert-fact! port log followed "progress" "observer update")
            (check "followed update materializes exactly one durable notification"
                   (eventually #(= 1 (count (notification-ids port log role)))))
            (check "followed update does not consume --once"
                   (.isAlive ^Process listener))
            (let [notification "@notification:concern-owner-notice"]
              (assert-fact! port log (str "@" agent)
                            "display_name" "Current concern owner")
              (doseq [[predicate value]
                      [["from" "north"]
                       ["subject" "Concern overlap entered"]
                       ["body" "@concern:a ↔ @concern:b"]
                       ["sent_at" "2026-07-30T00:00:00Z"]
                       ["recipient" (str "@" agent)]
                       ["attention_kind" "overlap-entered"]
                       ["delivery" "notify"]
                       ["kind" "notification"]
                       ["entity_kind" "notification"]]]
                (assert-fact! port log notification predicate value))
              (assert-fact! port log notification "target" agent)
              (check "raw concern owner notice reaches the current listener"
                     (eventually
                      #(and (.isAlive ^Process listener)
                            (output-has? follow-log
                                         "Concern overlap entered")))))
            (let [message "@msg:attention-listener-terminal"]
              (doseq [[predicate value]
                      [["from" "coordinator"]
                       ["subject" "direct interrupt"]
                       ["body" "wake the listener"]
                       ["sent_at" "2026-07-30T00:00:00Z"]]]
                (assert-fact! port log message predicate value))
              ;; The direct-address trigger remains last.
              (assert-fact! port log message "to" agent)
              (check "direct mail still acks and consumes --once"
                     (eventually
                      #(and (not (.isAlive ^Process listener))
                            (= #{agent}
                               (values-of port log message "acked_by"))
                            (output-has? follow-log "direct interrupt")))))))

        (let [agent "dynamic-follower"
              agent-node (str "@agent:" agent)
              role "@role:dynamic-reviewer"
              followed "@thread:dynamic-follow"
              dynamic-log (io/file tmp "dynamic-listener.log")]
          (assert-fact! port log role "title" "Dynamic reviewer")
          (assert-fact! port log agent-node "holds" role)
          (assert-fact! port log followed "title" "Dynamic follow target")
          (assert-fact! port log followed "kind" "thread")
          (let [listener
                (start-listener!
                 port log dynamic-log agent "--once" "--ack" "--scoped")]
            (check "scoped listener arms before a dynamic follow exists"
                   (eventually
                    #(output-has? dynamic-log "0 followed thread")))
            (let [follow
                  (run-attention
                   port log "follow" followed "--as" role
                   "--delivery" "notify")]
              (check "follow-after-arm triggers a scoped re-scope"
                     (and (zero? (:exit follow))
                          (eventually
                           #(output-has? dynamic-log
                                         "attention scope: 1 followed")))))
            (assert-fact! port log followed "progress" "dynamic update")
            (check "dynamic follow update notifies once without consuming --once"
                   (eventually
                    #(and (= 1 (count (notification-ids port log role)))
                          (= 1 (count
                                (re-seq
                                 #"(?m)^◉  NOTICE "
                                 (slurp dynamic-log))))
                          (.isAlive ^Process listener))))
            (let [unfollow
                  (run-attention port log "unfollow" followed "--as" role)]
              (check "unfollow-after-arm triggers terminal catch-up and re-scope"
                     (and (zero? (:exit unfollow))
                          (eventually
                           #(output-has? dynamic-log
                                         "attention scope: 0 followed")))))
            (assert-fact! port log followed "progress" "after dynamic unfollow")
            (run-attention port log "notifications" "--as" role)
            (check "dynamic unfollow suppresses later updates and never consumes --once"
                   (and (= 1 (count (notification-ids port log role)))
                        (.isAlive ^Process listener)))
            (stop-child! listener)))

        (let [agent "legacy-watcher"
              agent-node (str "@agent:" agent)
              watched "@thread:legacy-watch"
              legacy-log (io/file tmp "legacy-listener.log")]
          (assert-fact! port log watched "title" "Legacy watched thread")
          (assert-fact! port log agent-node "watches" watched)
          (let [listener
                (start-listener!
                 port log legacy-log agent "--once" "--ack" "--scoped")]
            (check "legacy watcher arms independently of attention follows"
                   (eventually #(output-has? legacy-log "1 watched + 0 followed")))
            (assert-fact! port log watched "progress" "legacy update")
            (check "legacy watched assert retains one-shot exit semantics"
                   (eventually
                    #(and (not (.isAlive ^Process listener))
                          (output-has? legacy-log "◆  THREAD"))))))))
    (finally
      (doseq [child @children] (stop-child! child))
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println
   (format "\nattention listener integration: %d / %d PASS"
           passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
