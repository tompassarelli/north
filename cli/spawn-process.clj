(ns north.spawn-process
  "Shared managed-process boundary for North's shell and MCP spawn surfaces.

  A successful OS fork is not a successful North spawn.  The caller may only
  acknowledge a live lane after the child has published its structured identity,
  committed an effective live-input route, and acquired an online presence lease.
  Fast terminal outcomes are reported as completed; early exits and
  acknowledgement timeouts are explicit failures."
  (:require [babashka.process :as p]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(load-file (str (.getParent (io/file *file*)) "/agent-provenance.clj"))
(load-file (str (.getParent (io/file *file*)) "/terminal-projection.clj"))

;; Must lose the race against the child's own pre-identity preflight: admission
;; spends up to 30s per projector subprocess (sdk/src/orchestration-graph-source.ts)
;; before provider selection resolves the model/effort identity facts carry.
(def default-startup-timeout-ms 180000)
(def default-startup-poll-ms 100)
(def default-exit-grace-ms 300)
(def detached-pid-suffix ".lane.pid")
(def detached-exit-suffix ".lane.exit")
(def detached-heartbeat-suffix ".lane.heartbeat")
(def startup-diagnostic-prefix "[north startup] NEVER-ACKNOWLEDGED")
(def ^:dynamic *heartbeat-interval-seconds* 30)

(defn create-agent-id
  "Mint an opaque process identity with sortable time plus the complete UUID.
  `now` and `uuid` are injectable only for deterministic tests."
  ([prefix]
   (create-agent-id prefix (System/currentTimeMillis) (java.util.UUID/randomUUID)))
  ([prefix now uuid]
   (str prefix "-" (Long/toString (long now) 36) "-" uuid)))

(defn identity-defects
  "Return the load-bearing identity proofs that are absent or contradictory.
  Managed lanes are always Orchestration-selected: `none` is valid only for native
  provider sessions and can never cross this startup acknowledgement gate."
  [facts]
  (north.agent-provenance/identity-defects facts))

(defn identity-valid?
  [facts]
  (empty? (identity-defects facts)))

(defn effective-route?
  "A live startup acknowledgement requires public input authority, not merely
  a digest-valid pending identity. Terminal acknowledgement remains independent
  because a lane may validly finish before it ever arms a feed."
  [facts]
  (or (and (= "streaming" (get facts "live_input"))
           (= "armed" (get facts "live_input_state")))
      ;; turn-messaged accepts input only between turns, so it starts frozen and
      ;; arms when a session goes interactive; both are settled public authority.
      (and (= "turn-messaged" (get facts "live_input"))
           (contains? #{"frozen" "armed"} (get facts "live_input_state")))
      (and (= "unsupported" (get facts "live_input"))
           (= "frozen" (get facts "live_input_state")))))

(defn identity-ready?
  [facts]
  (and (identity-valid? facts) (effective-route? facts)))

(defn startup-defects [facts]
  (cond-> (vec (identity-defects facts))
    (and (identity-valid? facts) (not (effective-route? facts)))
    (conj "effective_live_input_route")))

(defn env-ms
  [name fallback]
  (let [raw (System/getenv name)]
    (try
      (let [parsed (when raw (Long/parseLong raw))]
        (if (and parsed (pos? parsed)) parsed fallback))
      (catch Exception _ fallback))))

(defn default-startup-timeout-for-capabilities
  [_capabilities]
  default-startup-timeout-ms)

(defn startup-timeout-for-capabilities
  "Honor the existing operator override or use the managed-lane default."
  [capabilities]
  (env-ms "NORTH_SPAWN_STARTUP_TIMEOUT_MS"
          (default-startup-timeout-for-capabilities capabilities)))

(defn- executable-path [command environment]
  (let [executable (first command)]
    (if (str/includes? executable "/")
      executable
      (or (some (fn [directory]
                  (let [candidate (io/file directory executable)]
                    (when (and (.isFile candidate) (.canExecute candidate))
                      (.getPath candidate))))
                (str/split (or (get environment "PATH")
                               (System/getenv "PATH")
                               "")
                           #":"))
          executable))))

(defn launch-detached!
  "Daemonize command into a new session while retaining a bounded startup handle.
  `setsid --fork` exits its launcher immediately, so ancestor tree cleanup cannot
  discover and SIGTERM the lane after North has acknowledged admission. The
  daemon writes its PID before exec; startup and timeout cleanup use that PID
  while terminal publication remains the authoritative exit-status proof."
  [command extra-env log-file]
  (let [log (io/file log-file)
        pid-file (io/file (str log-file detached-pid-suffix))
        exit-file (io/file (str log-file detached-exit-suffix))
        heartbeat-file (io/file (str log-file detached-heartbeat-suffix))
        resolved-command (assoc (vec command) 0
                                (executable-path command extra-env))
        launcher-script
        (str "umask 077; pid_file=\"$1\"; exit_file=\"$2\"; heartbeat_file=\"$3\"; "
             "heartbeat_interval=\"$4\"; shift 4; "
             "printf '%s\\n' \"$$\" > \"$pid_file\"; "
             "touch \"$heartbeat_file\"; lane_pid=$$; "
             "(while kill -0 \"$lane_pid\" 2>/dev/null; do sleep \"$heartbeat_interval\"; "
             "touch \"$heartbeat_file\"; done) & "
             "heartbeat_pid=$!; "
             "cleanup_heartbeat() { kill \"$heartbeat_pid\" 2>/dev/null || true; "
             "wait \"$heartbeat_pid\" 2>/dev/null || true; }; "
             "trap cleanup_heartbeat EXIT; "
             "\"$@\"; status=$?; printf '%s\\n' \"$status\" > \"$exit_file\"; "
             "exit \"$status\"")]
    (.mkdirs (.getParentFile log))
    (io/delete-file pid-file true)
    (io/delete-file exit-file true)
    (io/delete-file heartbeat-file true)
    (let [process (p/process (into ["setsid" "--fork" "sh" "-c"
                                    launcher-script "north-lane"
                                    (.getPath pid-file) (.getPath exit-file)
                                    (.getPath heartbeat-file)
                                    (str *heartbeat-interval-seconds*)]
                                   resolved-command)
                             ;; Exact environment, not a merge. Managed callers
                             ;; pass a parent copy with every staffing/routing key
                             ;; scrubbed; :extra-env would silently reintroduce the
                             ;; invoking director's omitted axes.
                             {:env (assoc extra-env
                                          "NORTH_LANE_PID_FILE" (.getPath pid-file)
                                          "NORTH_LANE_HEARTBEAT_FILE" (.getPath heartbeat-file))
                              :out :write :out-file log
                              :err :out
                              ;; The daemon is deliberately reparented. A caller
                              ;; shutdown hook may reap only this short launcher.
                              :shutdown nil})]
      ;; The SDK entrypoints never consume their own stdin. Closing the pipe
      ;; prevents a detached lane from retaining the invoking CLI's input edge.
      (when-let [input (:in process)]
        (try (.close input) (catch Exception _ nil)))
      (assoc process ::pid-file pid-file ::exit-file exit-file
             ::heartbeat-file heartbeat-file))))

(defn- detached-pid [process]
  (when-let [pid-file (::pid-file process)]
    (try
      (let [raw (str/trim (slurp pid-file))
            pid (Long/parseLong raw)]
        (when (> pid 1) pid))
      (catch Exception _ nil))))

(defn- pid-alive? [pid]
  (try
    (let [handle (java.lang.ProcessHandle/of (long pid))]
      (and (.isPresent handle) (.isAlive (.get handle))))
    (catch Exception _ false)))

(defn- launcher-exit [process]
  (try
    (when-not (p/alive? process)
      (:exit @process))
    (catch Exception _ :unknown)))

(defn- detached-exit [process]
  (when-let [exit-file (::exit-file process)]
    (try
      (Long/parseLong (str/trim (slurp exit-file)))
      (catch Exception _ nil))))

(defn process-exit
  "Return nil while the daemon is alive, otherwise its observed terminal shape.
  The short `setsid` launcher normally exits 0 before admission and therefore
  cannot be mistaken for the lane's terminal."
  [process]
  (if (::pid-file process)
    ;; The receipt is authoritative once published. Read it before consulting
    ;; the PID so PID reuse cannot make a completed child appear live.
    (or (detached-exit process)
        (if-let [pid (detached-pid process)]
          (when-not (pid-alive? pid) :unknown)
          (let [exit (launcher-exit process)]
            (when (and (some? exit) (not= 0 exit)) exit))))
    (launcher-exit process)))

(defn process-alive?
  "True while the managed daemon, not merely its short launcher, is alive."
  [process]
  (and process (nil? (process-exit process))))

(defn await-process-exit
  "Wait for the detached child terminal receipt or observed child death.
  Never treats the short `setsid --fork` launcher as the managed lifetime."
  [process & {:keys [poll-ms] :or {poll-ms 20}}]
  (loop []
    (if-some [exit (process-exit process)]
      exit
      (do (Thread/sleep poll-ms) (recur)))))

(defn- signal-process-group! [pid signal]
  (try
    @(p/process ["kill" signal "--" (str "-" pid)]
                {:out :string :err :string :continue true})
    (catch Exception _ nil)))

(defn stop-process!
  [process]
  (when process
    (when-let [pid (detached-pid process)]
      (signal-process-group! pid "-TERM")
      (let [deadline (+ (System/currentTimeMillis) 1000)]
        (while (and (pid-alive? pid)
                    (< (System/currentTimeMillis) deadline))
          (Thread/sleep 20)))
      (when (pid-alive? pid)
        (signal-process-group! pid "-KILL")))
    ;; The launcher is normally already gone. This remains the forced cleanup
    ;; for a startup stall before the daemon PID could be published.
    (try (p/destroy-tree process) (catch Exception _ nil))
    ;; Retain PID/exit receipts until the next launch replaces them. A detached
    ;; lifetime watcher may still need the terminal proof after forced cleanup.
    (try (deref process 2000 nil) (catch Exception _ nil)))
  nil)

(defn- final-terminal-facts
  "Close the exit/read race: outcome is synchronously written before a managed
  SDK process exits, but the first graph read may have started just before that
  write. Re-read for a short bounded grace and merge with already observed
  identity so a fast clean completion cannot be mislabeled as construction
  failure."
  [agent-id initial-facts probe-identity grace-ms poll-ms]
  (let [deadline (+ (System/currentTimeMillis) grace-ms)]
    (loop [facts initial-facts]
      (let [observed (try (or (probe-identity agent-id) {}) (catch Exception _ {}))
            merged (merge facts observed)
            outcome (north.terminal-projection/terminal-process-outcome merged)]
        (if (or (and (identity-valid? merged) outcome)
                (>= (System/currentTimeMillis) deadline))
          merged
          (do (Thread/sleep poll-ms) (recur merged)))))))

(defn startup-diagnostic
  [{:keys [status agent-id exit missing timeout-ms]}]
  (let [why (case status
              :timeout (str "startup acknowledgement timed out after " timeout-ms "ms")
              :failed (str "child exited before startup acknowledgement"
                           (when (some? exit) (str " (exit " exit ")")))
              "startup acknowledgement failed")]
    (str startup-diagnostic-prefix " agent " agent-id " " why
         (when (seq missing)
           (str "; missing identity: " (str/join "," missing))))))

(defn append-startup-diagnostic!
  "Make a failed acknowledgement self-explanatory even when the dispatcher's
  stdout is discarded. Best effort because the original startup failure must
  still be returned when the durable-log filesystem is itself unavailable."
  [result]
  (when-let [log (:log result)]
    (try
      (spit (io/file log) (str (startup-diagnostic result) "\n") :append true)
      (catch Exception _ nil)))
  result)

(defn await-startup
  "Wait for three durable startup proofs: complete structured lane identity,
  effective route authority, and an online presence lease. `probe-identity`
  returns the current predicate map; `probe-online` returns whether this exact
  id owns an unexpired lease."
  [process agent-id log-file probe-identity probe-online
   & {:keys [timeout-ms poll-ms exit-grace-ms]
      :or {timeout-ms (env-ms "NORTH_SPAWN_STARTUP_TIMEOUT_MS" default-startup-timeout-ms)
           poll-ms (env-ms "NORTH_SPAWN_STARTUP_POLL_MS" default-startup-poll-ms)
           exit-grace-ms (env-ms "NORTH_SPAWN_EXIT_GRACE_MS" default-exit-grace-ms)}}]
  (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
    (loop [last-facts {}]
      (let [facts (try (or (probe-identity agent-id) {})
                       (catch Exception _ last-facts))
            identity? (identity-valid? facts)
            route-ready? (and identity? (effective-route? facts))
            outcome (north.terminal-projection/terminal-process-outcome facts)
            first-exit (process-exit process)
            online? (and identity? (nil? first-exit)
                         (try (boolean (probe-online agent-id)) (catch Exception _ false)))
            exit (or first-exit (process-exit process))]
        (cond
          ;; A lane can finish between fork and acknowledgement. Its structured
          ;; identity plus terminal outcome is stronger evidence than presence.
          (and identity? outcome)
          {:status :completed :agent-id agent-id :handle (get facts "display_handle")
           :outcome outcome :facts facts :log (str log-file)}

          (and route-ready? online? (nil? exit))
          {:status :ready :agent-id agent-id :handle (get facts "display_handle")
           :facts facts :log (str log-file)}

          (some? exit)
          (let [final-facts (final-terminal-facts
                             agent-id facts probe-identity exit-grace-ms poll-ms)
                final-outcome (north.terminal-projection/terminal-process-outcome final-facts)]
            (if (and (identity-valid? final-facts) final-outcome)
              {:status :completed :agent-id agent-id
               :handle (get final-facts "display_handle")
               :outcome final-outcome :facts final-facts :log (str log-file)}
              (append-startup-diagnostic!
               {:status :failed :agent-id agent-id :exit exit :facts final-facts
                :missing (startup-defects final-facts)
                :log (str log-file)})))

          (>= (System/currentTimeMillis) deadline)
          (do
            (stop-process! process)
            (append-startup-diagnostic!
             {:status :timeout :agent-id agent-id :facts facts
              :missing (startup-defects facts)
              :log (str log-file) :timeout-ms timeout-ms}))

          :else
          (do (Thread/sleep poll-ms) (recur facts)))))))

(defn log-tail
  ([log-file] (log-tail log-file 2048))
  ([log-file max-bytes]
   (try
     (let [file (io/file log-file)]
       (if-not (.isFile file)
         ""
         (with-open [raf (java.io.RandomAccessFile. file "r")]
           (let [size (.length raf)
                 start (max 0 (- size max-bytes))
                 bytes (byte-array (int (- size start)))]
             (.seek raf start)
             (.readFully raf bytes)
             (str/trim (String. bytes java.nio.charset.StandardCharsets/UTF_8))))))
     (catch Exception _ ""))))

(defn settled-log-tail
  "The child's last output, waiting briefly for it to land.

  The child writes its stderr to the durable log and the bytes flush as it
  exits, so reading the instant the parent notices the exit usually returns
  NOTHING. That is how a dispatch failure loses its cause: observed
  2026-07-29, a lane died and the operator was shown

    child exited before startup acknowledgement (exit 0); missing identity:
    kind,role,goal,provider,... (16 fields)

  — a list of symptoms — while the log itself held the one useful line,
  `Connection refused` against the coordinator on :7977. The message already
  appended a tail; the tail was simply empty at the moment it was read.

  Bounded and best-effort: a failing spawn is already terminal, so a few
  hundred milliseconds to make it explicable is cheap, and giving up quietly
  is no worse than the behaviour it replaces."
  [log]
  (let [deadline (+ (System/currentTimeMillis) 750)]
    (loop []
      (let [tail (log-tail log)]
        (cond
          (seq tail) tail
          (>= (System/currentTimeMillis) deadline) ""
          :else (do (Thread/sleep 50) (recur)))))))

(defn failure-message
  [{:keys [status agent-id exit missing log timeout-ms]}]
  (let [why (case status
              :timeout (str "startup acknowledgement timed out after " timeout-ms "ms")
              :failed (str "child exited before startup acknowledgement"
                           (when (some? exit) (str " (exit " exit ")")))
              "startup failed")
        missing-note (when (seq missing)
                       (str "; missing identity: " (str/join "," missing)))
        tail (settled-log-tail log)]
    (str "agent " agent-id " " why missing-note
         "; durable log: " log
         (when (seq tail) (str "\nlast log output:\n" tail)))))
