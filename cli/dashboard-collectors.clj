(ns north.dashboard.collectors
  (:require [babashka.process :as p]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json]
            [north.dashboard.state :as state])
  (:import [java.net Socket InetSocketAddress]))

(def home (System/getenv "HOME"))
(def state-dir (str home "/.local/state/north"))
(def north-root (some-> (or (System/getProperty "babashka.file") *file*) io/file .getCanonicalFile .getParentFile .getParentFile str))
(def running (atom {}))
(def log-sizes (atom {}))
(def last-started (atom {}))
(def last-finished (atom {}))
(def failures (atom {}))
(def titles (atom {}))
(defn now [] (state/now))
(defn run [argv timeout]
  (try (let [x (p/process argv {:out :string :err :string}) r (deref x timeout ::timeout)]
         (if (= r ::timeout) (do (p/destroy-tree x) {:status :timeout :detail "deadline"})
           (if (zero? (:exit r)) {:status :ok :data (:out r)} {:status :error :detail (str/trim (:err r))})))
       (catch Exception e {:status :error :detail (.getMessage e)})))
(defn alive? [pid]
  ;; /proc is available on the Linux hosts that own North's lane receipts.
  (try (.exists (io/file "/proc" (str pid))) (catch Exception _ false)))
(defn agent-processes []
  ;; A single sweep keeps fleet collection bounded independently of lane count.
  (into {}
        (keep (fn [proc]
                (try
                  (let [pid (.getName proc)
                        env (slurp (io/file proc "environ"))]
                    (when-let [id (some #(when (str/starts-with? % "AGENT_ID=") (subs % 9))
                                        (str/split env #"\u0000"))]
                      [id (Long/parseLong pid)]))
                  (catch Exception _ nil))))
        (filter #(.isDirectory %) (or (seq (.listFiles (io/file "/proc"))) []))))
(defn title [id thread-id]
  (or (get @titles id)
      (let [value (try (let [text (some->> (.listFiles (io/file state-dir "threads"))
                                           (filter #(str/starts-with? (.getName %) (str thread-id "-"))) first slurp)]
                         (or (some-> (re-find #"(?m)^#\s+(.+)$" text) second)
                             (some-> (re-find #"(?m)^title\s+\"([^\"]+)\"" text) second)))
                       (catch Exception _ nil))]
        (swap! titles assoc id value) value)))
(defn log-head [log] (with-open [r (io/reader log)] (let [b (char-array 4096) n (.read r b)] (String. b 0 (max 0 n)))))
(defn log-text [log] (slurp log))
(defn spawn-details [log]
  (try
    ;; Mutation lanes provision their worktree before emitting the spawn line.
    (let [header (some #(when (str/starts-with? % "[spawn]") %)
                       (str/split-lines (log-head log)))]
      (cond-> {}
        (some->> header (re-find #"\bprovider=([^\s;()]+)") second)
        (assoc :provider (some->> header (re-find #"\bprovider=([^\s;()]+)") second))
        (some->> header (re-find #"\btier=([^\s;()]+)") second)
        (assoc :role (some->> header (re-find #"\btier=([^\s;()]+)") second))
        (some->> header (re-find #"\broute=[^/\s;()]+/([^\s;()]+)") second)
        (assoc :effort (some->> header (re-find #"\broute=[^/\s;()]+/([^\s;()]+)") second))))
    (catch Exception _ {})))
(defn lane-meta [dir id]
  (try (json/parse-string (slurp (io/file dir (str "lane-" id ".meta.json"))) true)
       (catch Exception _ {})))
(defn work-status [agent completion harvested?]
  ;; A clean receipt alone records agent exit, not a task result.
  (cond
    (= agent "vanished") "unknown"
    (and (= agent "done") (or (= completion "ran") harvested?)) "delivered"
    (#{"done" "crashed"} agent) "none"
    :else "unknown"))
(defn lanes []
  (let [dir (io/file state-dir "agents") processes (agent-processes)]
    {:lanes (for [log (or (seq (.listFiles dir)) []) :when (re-matches #"lane-.+\.log" (.getName log))
                  :let [id (subs (.getName log) 5 (- (count (.getName log)) 4)) pidf (io/file dir (str (.getName log) ".lane.pid")) exitf (io/file dir (str (.getName log) ".lane.exit"))
                        pid (try (Long/parseLong (str/trim (slurp pidf))) (catch Exception _ nil)) terminal (.exists exitf)
                        prior-size (get @log-sizes id) grew (and (some? prior-size) (> (.length log) (long prior-size))) _ (swap! log-sizes assoc id (.length log))
                        text (log-text log)
                        completion (some-> (re-find #"complete \(process=([^,\)]+)" text) second)
                        harvested? (boolean (re-find #"(?m)\bharvested\s+[1-9][0-9]*\s+commit\(s\)" text))
                        meta (lane-meta dir id)
                        thread-id (or (:thread meta) (some-> (re-find #"(?m)AGENT_THREAD=([^\s]+)" (log-head log)) second))
                        discovered-pid (get processes id)
                        status (cond terminal (if (zero? (try (Long/parseLong (str/trim (slurp exitf))) (catch Exception _ 1))) "finished" "failed")
                                     completion (if (= completion "ran") "finished" "failed")
                                     (< (- (now) (.lastModified log)) 120000) "advancing"
                                     ;; The receipt pid is authoritative; the /proc scan is a
                                     ;; best-effort supplement that sees nothing in some sandboxes.
                                     (or discovered-pid (and pid (alive? pid)))
                                     (str "working (quiet " (quot (- (now) (.lastModified log)) 60000) "m)")
                                     :else "vanished")]]
              (let [agent (case status
                            "advancing" "running"
                            "finished" "done"
                            "failed" "crashed"
                            "vanished")]
                (merge {:id id :title (or (and thread-id (title id thread-id)) id) :status status
                        :agent agent :work (work-status agent completion harvested?)
                        :pid (or discovered-pid (when (and pid (alive? pid)) pid))
                      :started-at (.lastModified log)
                      :elapsed (max 0 (- (now) (.lastModified log)))
                      :last-output-age (max 0 (- (now) (.lastModified log)))}
                     (spawn-details log)
                     (select-keys meta [:role :effort :provider :model :thread :startedAt]))))}))
(defn socket-up? [port] (try (with-open [s (Socket.)] (.connect s (InetSocketAddress. "127.0.0.1" port) 400) true) (catch Exception _ false)))
(defn cgroup [unit]
  (let [base (io/file "/sys/fs/cgroup/system.slice" unit)]
    (into {} (for [k ["memory.current" "memory.high" "memory.max"]]
               [k (try (str/trim (slurp (io/file base k))) (catch Exception _ "unavailable"))]))))
(defn health []
  {:services (into {} (for [[unit port] [["north-coord.service" 7977] ["north-telemetry-coord.service" 7978]]]
                        [unit {:active (= "active" (str/trim (or (:data (run ["systemctl" "is-active" unit] 1500)) "")))
                               :socket (socket-up? port) :memory (cgroup unit)}]))})
(defn board [] (let [r (run [(str north-root "/bin/north") "board" "--fresh"] 120000)] (if (= :ok (:status r)) (assoc r :data {:text (:data r)}) r)))
(defn providers [] (let [r (run [(str north-root "/bin/north") "providers" "--json"] 45000)] (if (= :ok (:status r)) (try (assoc r :data (json/parse-string (:data r) true)) (catch Exception e {:status :error :detail (.getMessage e)})) r)))
(defn failure-detail [t]
  (str (class t) ": " (.getMessage t)))
(defn record-failure! [panel t]
  (try
    (state/record-error-fallback! panel (failure-detail t))
    (catch Throwable _ nil)))
(defn collect! [panel f]
  (when-not (contains? @running panel)
    (let [started (now)]
      (swap! running assoc panel started)
      (swap! last-started assoc panel started)
      (future (try
                (let [result {:status :ok :data (f)}]
                  (state/record! panel result)
                  (swap! failures assoc panel 0))
                (catch Throwable t
                  (record-failure! panel t)
                  (swap! failures update panel (fnil inc 0)))
                (finally (swap! last-finished assoc panel (now))
                         (swap! running #(if (= (get % panel) started) (dissoc % panel) %))))))))
(defn clear-stuck! []
  (doseq [[panel started] @running
          :when (> (- (now) started) 60000)]
    (swap! running dissoc panel)
    (record-failure! panel (ex-info "collector exceeded 60s" {}))
    (swap! failures update panel (fnil inc 0))))
(defn due? [panel interval]
  (let [at (get @last-finished panel 0)]
    (>= (- (now) at) interval)))
(defn board-interval [] (nth [60000 120000 300000] (min 2 (get @failures :board 0))))
(defn refresh! []
  (clear-stuck!)
  (when (due? :lanes 1000) (collect! :lanes lanes))
  (when (due? :health 5000) (collect! :health health))
  ;; Completion timestamps make cadence and backoff start after a bounded attempt.
  (when (due? :board (board-interval)) (collect! :board #(let [r (board)] (if (= :ok (:status r)) (:data r) (throw (ex-info (:detail r) {}))))))
  (when (due? :providers 300000) (collect! :providers #(let [r (providers)] (if (= :ok (:status r)) (:data r) (throw (ex-info (:detail r) {})))))))
