#!/usr/bin/env bb
;; Directed-attention producer and public-wrapper contract against canonical
;; FRAMRPC. Message publication atomicity has its own transaction-level tests.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      "/home/tom/code/beagle/main/branch-core"))
(def runtime-classpath (str root "/out:" fram "/out"))
(cp/add-classpath runtime-classpath)
(def msg-cli (str root "/cli/msg-cli.clj"))
(def listener-cli (str root "/cli/north-listen.clj"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(def north-wrapper (str root "/bin/north"))
(def checks (atom []))

(load-file (str root "/cli/coord.clj"))

(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn eventually [predicate]
  (loop [attempt 0]
    (cond
      (predicate) true
      (>= attempt 300) false
      :else (do (Thread/sleep 25) (recur (inc attempt))))))

(defn await-daemon-boot [predicate]
  ;; Fram cold starts can exceed 20 seconds under ordinary host contention.
  ;; Match the established msg-cli integration-test budget instead of turning
  ;; a slow fixture into a product failure.
  (loop [attempt 0]
    (cond
      (predicate) true
      (>= attempt 300) false
      :else (do (Thread/sleep 250) (recur (inc attempt))))))

(defn assert-fact! [port subject predicate value]
  (let [result (north.coord/append! port subject predicate value)]
    (when (:reject result)
      (throw (ex-info "fixture assertion rejected" result)))
    result))

(defn values-of [port subject predicate]
  (set (north.coord/many port subject predicate)))

(defn value-of [port subject predicate]
  (north.coord/resolved port subject predicate))

(defn isolated-env [port log]
  {"FRAM_LOG" log
   "FRAM_TELEMETRY_LOG"
   (.getCanonicalPath
    (io/file (.getParentFile (io/file log)) "telemetry.framlog"))
   "FRAM_SPACE_ID" "north-coordination"
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})

(defn run-cli [path port log & args]
  (apply proc/shell
         {:continue true
          :out :string
          :err :string
          :extra-env
          (merge
           (isolated-env port log)
           {"NORTH_PORT" (str port)
            "AGENT_TOPOLOGY" "orchestrator"
            "NO_COLOR" "1"})}
         "bb" "-cp" runtime-classpath path (str port) args))

(defn run-msg [port log & args]
  (apply run-cli msg-cli port log args))

(defn message-id [result]
  (second (re-find #"sent (@msg:[^ ]+) ->" (:out result))))

(defn graph-message-ids [port]
  (->> (north.coord/query-rows
        port
        {:find "message"
         :rules
         [{:head {:rel "message" :args [{:var "message"}]}
           :body [{:rel "triple"
                   :args [{:var "message"} "kind" "message"]}]}]})
       (map first)
       set))

(defn with-daemon [prefix body]
  (let [port (free-port)
        tmp (.toFile
             (java.nio.file.Files/createTempDirectory
              prefix (make-array java.nio.file.attribute.FileAttribute 0)))
        facts (io/file tmp "facts.framlog")
        log (.getCanonicalPath facts)
        daemon
        (proc/process
         {:dir fram
          :out :string
          :err :string
          :extra-env
          (merge
           (isolated-env port log)
           {"FRAM_SERVER_RUNTIME" "jvm-dev"
            "FRAM_SERVER_QUIET" "1"
            "FRAM_SERVER_XMX" "1g"
            "FRAM_SINGLE_VALUED"
            (str "title from subject body sent_at to attention_kind "
                 "delivery_class requires_ack about agent dir session_id started_at")})}
         (str fram "/bin/fram-server") "serve" (str port) log
         "north-coordination")]
    (try
      (let [started?
            (await-daemon-boot
             #(let [status (try (north.coord/status port)
                                (catch Throwable _ nil))]
                (and (= :ready (:state status))
                     (= "north-coordination" (:space-id status)))))]
        (check "throwaway Fram coordinator starts" started?)
        (when started?
          (body port log tmp)))
      (finally
        (try (proc/destroy-tree daemon) (catch Exception _ nil))
        (try @daemon (catch Exception _ nil))
        (doseq [file (reverse (file-seq tmp))]
          (io/delete-file file true))))))

(defn run-mention! []
  (with-daemon
   "north-directed-mention"
   (fn [port log _tmp]
     (let [thread "@thread:directed-attention"
           _ (assert-fact! port thread "title" "Directed attention")
           wrong-kind "@concern:not-a-thread"
           _wrong-kind-title
           (assert-fact! port wrong-kind "title" "Not a thread")
           _wrong-kind-kind
           (assert-fact! port wrong-kind "kind" "concern")
           result
           (run-msg port log "mention" "requester" "offline-reviewer"
                    "--about" thread "Please review this.")
           message (message-id result)]
       (check "mention accepts an offline stable address"
              (and (zero? (:exit result))
                   (= (str "sent " message " -> offline-reviewer\n")
                      (:out result))))
       (check "mention envelope carries durable attention metadata and about"
              (and message
                   (= "requester" (value-of port message "from"))
                   (= "offline-reviewer" (value-of port message "to"))
                   (= "mention" (value-of port message "subject"))
                   (= "mention" (value-of port message "attention_kind"))
                   (= "inbox" (value-of port message "delivery_class"))
                   (= "true" (value-of port message "requires_ack"))
                   (= thread (value-of port message "about"))))
       (let [inbox (run-msg port log "inbox" "offline-reviewer")]
         (check "offline mention remains pending in the recipient inbox"
                (and (zero? (:exit inbox))
                     (str/includes? (:out inbox) (subs message 5))
                     (str/includes? (:out inbox) "mention")
                     (empty? (values-of port message "acked_by")))))
       (let [before (graph-message-ids port)
             malformed
             [(run-msg port log "mention" "requester" "offline-reviewer"
                       "--unknown" "body")
              (run-msg port log "mention" "requester" "offline-reviewer"
                       "--about" thread "--about" thread "body")
              (run-msg port log "mention" "requester" "offline-reviewer"
                       "--about" "--missing")
              (run-msg port log "mention" "requester" "offline-reviewer"
                       "--about" "@thread:absent" "body")
              (run-msg port log "mention" "requester" "offline-reviewer"
                       "--about" wrong-kind "body")]]
         (check "malformed options and non-thread about reject before publication"
                (and (every? #(= 2 (:exit %)) malformed)
                     (every? #(str/includes? (:err %) "REJECTED: message")
                             malformed)
                     (= before (graph-message-ids port)))))))))

(defn run-interrupt! []
  (with-daemon
   "north-directed-interrupt"
   (fn [port log tmp]
     (let [before (graph-message-ids port)
           absent
           (run-msg port log "interrupt" "director" "live-reviewer"
                    "Please stop and look.")]
       (check "interrupt rejects an absent recipient without message facts"
              (and (= 2 (:exit absent))
                   (str/includes? (:err absent) "has no live presence")
                   (= before (graph-message-ids port)))))
     (let [registered
           (run-cli presence-cli port log
                    "register" "live-reviewer" "/tmp/live-reviewer" "test-session")
           listener-log (io/file tmp "listener.log")
           listener
           (proc/process
            {:out listener-log
             :err listener-log
            :extra-env {"FRAM_LOG" log
                        "FRAM_SPACE_ID" "north-coordination"
                        "NO_COLOR" "1"}}
            "bb" "-cp" runtime-classpath listener-cli
            (str port) "live-reviewer"
            "--once" "--ack")]
       (try
         (check "recipient has a live presence lease"
                (zero? (:exit registered)))
         (check "recipient listener is armed"
                (eventually
                 #(and (.exists listener-log)
                       (str/includes? (slurp listener-log) "listening"))))
         (let [result
               (run-msg port log "interrupt" "director" "live-reviewer"
                        "Please stop and look.")
               message (message-id result)]
           (check "interrupt publishes the urgent live-only envelope"
                  (and (zero? (:exit result))
                       message
                       (= "URGENT" (value-of port message "subject"))
                       (= "interrupt"
                          (value-of port message "attention_kind"))
                       (= "interrupt"
                          (value-of port message "delivery_class"))
                       (= "true"
                          (value-of port message "requires_ack"))))
           (check "interrupt reaches and is acknowledged by the live recipient"
                  (and
                   (eventually
                    #(and (.exists listener-log)
                          (str/includes? (slurp listener-log) "URGENT")
                          (str/includes? (slurp listener-log)
                                         "Please stop and look.")))
                   (eventually
                    #(= #{"live-reviewer"}
                        (values-of port message "acked_by"))))))
         (finally
           (try (proc/destroy-tree listener) (catch Exception _ nil))
           (try @listener (catch Exception _ nil))))))))

(defn wrapper-result [tmp env & args]
  (apply
   proc/shell
   {:continue true
    :out :string
    :err :string
    :extra-env
    (merge
     {"HOME" (.getCanonicalPath tmp)
      "XDG_CONFIG_HOME" (str (.getCanonicalPath tmp) "/config")
      "FRAM_HOME" "/test/fram"
      "FRAM_BIN" "/test/fram/bin"
      "FRAM_OUT" "/test/fram/out"
      "NORTH_BB" "/run/current-system/sw/bin/echo"
      "NORTH_PORT" "47891"}
     env)}
   north-wrapper args))

(defn run-wrapper! []
  (let [tmp (.toFile
             (java.nio.file.Files/createTempDirectory
              "north-directed-wrapper"
              (make-array java.nio.file.attribute.FileAttribute 0)))]
    (try
      (let [agent
            (wrapper-result
             tmp {"AGENT_ID" "agent-first"
                  "NORTH_AGENT_ID" "north-second"
                  "NORTH_AUTHOR" "author-third"}
             "mention" "reviewer" "--about" "@thread:x" "body")
            north-agent
            (wrapper-result
             tmp {"AGENT_ID" ""
                  "NORTH_AGENT_ID" "north-second"
                  "NORTH_AUTHOR" "author-third"}
             "interrupt" "reviewer" "body")
            author
            (wrapper-result
             tmp {"AGENT_ID" ""
                  "NORTH_AGENT_ID" ""
                  "NORTH_AUTHOR" "author-third"}
             "mention" "reviewer" "body")]
        (check "mention and interrupt wrapper routes preserve arguments"
               (and
                (zero? (:exit agent))
                (str/includes?
                 (:out agent)
                 (str msg-cli " 47891 mention agent-first reviewer "
                      "--about @thread:x body"))
                (zero? (:exit north-agent))
                (str/includes?
                 (:out north-agent)
                 (str msg-cli " 47891 interrupt north-second reviewer body"))
                (zero? (:exit author))
                (str/includes?
                 (:out author)
                 (str msg-cli " 47891 mention author-third reviewer body"))))
        (check "wrapper sender precedence is AGENT_ID then NORTH_AGENT_ID then NORTH_AUTHOR"
               (and
                (not (str/includes? (:out agent) "mention north-second"))
                (not (str/includes? (:out north-agent) "interrupt author-third"))
                (str/includes? (:out author) "author-third"))))
      (let [watch
            (wrapper-result
             tmp {"AGENT_ID" "observer"}
             "watch" "agent-under-test")]
        (check "north watch remains on the transcript-oriented agents route"
               (and
                (zero? (:exit watch))
                (str/includes? (:out watch)
                               (str root "/cli/agents-cli.clj watch agent-under-test")))))
      (finally
        (doseq [file (reverse (file-seq tmp))]
          (io/delete-file file true))))))

(let [mode (first *command-line-args*)]
  (case mode
    "mention" (run-mention!)
    "interrupt" (run-interrupt!)
    "wrapper" (run-wrapper!)
    (do
      (binding [*out* *err*]
        (println
         "usage: directed-attention-integration-test.clj {mention|interrupt|wrapper}"))
      (System/exit 2))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println
     (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println
   (format "\ndirected attention (%s): %d / %d PASS"
           (first *command-line-args*) passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
