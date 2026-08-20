#!/usr/bin/env bb
;; north dashboard / doctor — the cockpit over the agentic stack (Beagle Store · North ·
;; orchestration · beagle/firn). Ported from convoy/bin/my-agents (2026-07-10): convoy
;; folded into north — dashboard answers "what is happening", doctor answers "is
;; everything healthy". Both WRAP primitives and PRINT the one they run: teach
;; the tool, don't hide it. Never re-derives doctrine, never owns state beyond
;; ~/.cache/north.
;;
;; Vocabulary law: facts (never claims), lanes/agents throughout.
;;
;;   north dashboard   → cmd-dashboard   (agents, concerns, board, daemons, health, profile)
;;   north doctor      → cmd-doctor      (coordinator handshake, daemons, coordination health, rev skew, env, guard hooks)

(require '[babashka.process :as p]
         '[clojure.string :as str]
         '[clojure.edn :as edn]
         '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def HOME (System/getenv "HOME"))
;; this file lives in north/cli — NORTH is its repo root.
(def SCRIPT (or (System/getProperty "babashka.file") *file*))
(def NORTH (some-> SCRIPT io/file .getCanonicalFile .getParentFile .getParentFile str))
(defn required-store-selection [key]
  (or (not-empty (System/getenv key))
      (throw (ex-info (str "canonical Beagle Store selection is incomplete; missing " key)
                      {:missing key}))))
(def STORE (required-store-selection "BEAGLE_STORE_HOME"))
(required-store-selection "BEAGLE_STORE_BIN")
(required-store-selection "BEAGLE_STORE_OUT")
(def BEAGLE (or (System/getenv "BEAGLE_HOME") (str HOME "/code/beagle/main")))
(def NIXCFG (or (System/getenv "NIXOS_CONFIG_HOME") (str HOME "/code/nixos-config")))
(def AGENT-LOGDIR (str HOME "/.local/state/north/agents"))
(load-file (str NORTH "/cli/coord.clj"))
(load-file (str NORTH "/cli/message-routing.clj"))
;; Scheduled maintenance writes one durable heartbeat per responsibility.
(load-file (str NORTH "/cli/worker-heartbeat.clj"))
(load-file (str NORTH "/out/north/worker_policy.clj"))
(load-file (str NORTH "/cli/dashboard-state.clj"))
(load-file (str NORTH "/cli/dashboard-collectors.clj"))
(load-file (str NORTH "/cli/dashboard-render.clj"))
(def CACHE-DIR (str HOME "/.cache/north"))
(def PORT (or (System/getenv "NORTH_PORT") "7977"))
(def CACHE-SCOPE
  (str (hash (str (or (System/getenv "BEAGLE_STORE_SPACE_ID") "north-coordination") "|"
                  (or (System/getenv "NORTH_TELEMETRY_SPACE_ID") "north-telemetry") "|"
                  PORT))))

;; Doctor is an operational liveness command, so its coordinator handshake must
;; be bounded independently of corpus size.
(def COORD-SAFETY-TIMEOUT-MS 5000)

;; ---- ANSI (respect NO_COLOR / non-tty) --------------------------------------
(def color? (and (nil? (System/getenv "NO_COLOR"))
                 (not (System/getenv "NORTH_NO_COLOR"))))
(defn c [code s] (if color? (str "\033[" code "m" s "\033[0m") s))
(defn dim [s]  (c "2" s))
(defn bold [s] (c "1" s))
(defn grn [s]  (c "32" s))
(defn red [s]  (c "31" s))
(defn ylw [s]  (c "33" s))
(defn cyn [s]  (c "36" s))
(defn ok-x [b] (if b (grn "up") (red "down")))

;; ---- process helper: never hang; short timeout; degrade -----------------------
(defn run
  "Run argv, bounded by :timeout ms. Returns {:out :err :exit :ok} or
   {:timeout true :ok false} / {:error msg :ok false}. Never throws."
  [argv & {:keys [timeout in] :or {timeout 3000}}]
  (try
    (let [proc (p/process argv (cond-> {:out :string :err :string} in (assoc :in in)))
          res  (deref proc timeout ::timeout)]
      (if (= res ::timeout)
        (do (p/destroy-tree proc) {:timeout true :ok false})
        {:out (or (:out res) "") :err (or (:err res) "") :exit (:exit res)
         :ok (zero? (:exit res))}))
    (catch Exception e {:error (.getMessage e) :ok false})))

(defn coord-safety-probe []
  (assoc (run [(str NORTH "/bin/north") "coord-safety"]
              :timeout COORD-SAFETY-TIMEOUT-MS)
         :timeout-ms COORD-SAFETY-TIMEOUT-MS))

(defn echo-cmd
  "Print the underlying primitive being wrapped (teaching surface)."
  [& parts] (println (dim (str "» " (str/join " " parts)))))

;; ---- honest cache: slow-moving aggregates only, short TTL ---------------------
;; The single-threaded coordinator serializes every probe, so fan-out is capped
;; by the daemon, not by cores. The one dashboard-side lever is to keep the slowest
;; probe off that queue. `north health` is both the tallest pole (~2.3s) and the
;; slowest-moving data (24h lane aggregates + STALE concern count) -> cache it.
;; Never caches an error/timeout; the dashboard is a point-in-time snapshot so a
;; ~60s-stale 24h window is invisible. Doctor uses the uncached path (live check).
(defn cache-get
  "Cached value from CACHE-DIR/name if written within ttl-ms, else nil. Never throws."
  [name ttl-ms]
  (try
    (let [f (io/file CACHE-DIR (str CACHE-SCOPE "-" name))]
      (when (.exists f)
        (let [{:keys [ts val]} (edn/read-string (slurp f))]
          (let [age (- (System/currentTimeMillis) (or ts 0))]
            (when (and ts (>= age 0) (< age ttl-ms)) val)))))
    (catch Exception _ nil)))

(defn cache-put!
  "Persist val under CACHE-DIR/name with a timestamp; returns val. Never throws."
  [name val]
  (try
    (let [dir (io/file CACHE-DIR)
          f (io/file dir (str CACHE-SCOPE "-" name))]
      (.mkdirs dir)
      (.setReadable dir false false) (.setWritable dir false false) (.setExecutable dir false false)
      (.setReadable dir true true) (.setWritable dir true true) (.setExecutable dir true true)
      (spit f (pr-str {:ts (System/currentTimeMillis) :val val}))
      (.setReadable f false false) (.setWritable f false false) (.setExecutable f false false)
      (.setReadable f true true) (.setWritable f true true))
    (catch Exception _ nil))
  val)

;; ---- portable listener discovery: ss on Linux, lsof on Darwin ---------------
(defn listening-ports []
  (let [ss (run ["ss" "-tlnH"] :timeout 1500)]
    (if (:ok ss)
      (set (map second (re-seq #":(\d+)\s" (:out ss))))
      (let [lsof (run ["lsof" "-nP" "-iTCP" "-sTCP:LISTEN" "-Fn"] :timeout 1500)]
        (if (:ok lsof)
          (->> (str/split-lines (:out lsof))
               (keep #(some-> (re-find #":(\d+)$" %) second))
               set)
          #{})))))

(defn daemon-health []
  ;; The fact coordinator is the only North daemon surface. Web modules and
  ;; their listeners were retired; dashboard health must not report them.
  (let [ports (listening-ports)]
    {:north (contains? ports PORT)     ; fact coordinator (the canonical log)
     :ports ports}))

;; ---- presence: live agents --------------------------------------------------
;; CACHED 20s. `presence-online` starts from the bounded set of unexpired lease
;; facts and enriches only those rows; it never walks the lifetime registry of
;; lapsed sessions. The cache still insulates back-to-back dashboard renders and
;; brief contention on the shared coordinator. Only successful reads are cached.
(defn presence-rows []
  (or (cache-get "presence.edn" 20000)
      (let [r (run ["bb" (str NORTH "/cli/presence-cli.clj") PORT "presence-online"] :timeout 6000)]
        (cond
          (:timeout r) {:err "presence probe timed out"}
          (not (:ok r)) {:err "presence unavailable"}
          :else
          (let [lines (->> (str/split-lines (:out r))
                           (drop 1)                       ; header
                           (remove str/blank?))]
            (cache-put! "presence.edn"
              {:agents
               ;; PIN column is blank in data rows, so parse by semantics not position:
               ;; online = the yes|no token, expires = the <n>s|lapsed token.
               (doall
                (for [ln lines
                      :let [toks (str/split (str/trim ln) #"\s+")
                            agent (first toks)
                            online (some #{"yes" "no"} toks)
                            expires (some #(when (re-matches #"\d+s|lapsed" %) %) toks)
                            focus (last toks)]
                      :when (and agent (seq agent))]
                  {:id agent :online (= online "yes") :expires (or expires "?")
                   :focus (when-not (#{"-" online expires} focus) focus)}))}))))))

;; ---- concerns: active, grouped by repo --------------------------------------
;; The dashboard consumes the STRICT VERSIONED MACHINE projection (`concern list-json`)
;; — never the human render. A malformed or wrong-version payload FAILS CLOSED to an
;; error line rather than guessing, so a projection format bump can never silently
;; misrender concern counts.
(def concern-projection-version 1)

;; The EXACT contract concern-cli's `projection-row` emits: the full key set, the
;; closed maturity/classification vocabularies, and the semantic invariant tying
;; classification to (retired, online, maturity). A machine consumer that accepts a
;; row it did not fully validate is a silent-corruption vector, so the consumer
;; re-checks every field and REJECTS the whole payload on any deviation — missing,
;; extra, wrong-type, unknown-value, or contradictory. `:agent` is the one nullable
;; field: agent-less concerns render live with a null agent (concern-cli).
(def ^:private concern-row-keys
  #{:id :agent :repo :intent :maturity :classification :online :retired :touches})
;; The full top-level envelope contract: exactly `:version` and `:concerns`, no
;; more, no less. A payload with an extra top-level field is as much a silent-
;; corruption vector as an extra row field, so it's rejected the same way.
(def ^:private concern-envelope-keys #{:version :concerns})
(def ^:private concern-maturities #{"exploring" "building" "likely-to-land" "landed" "deployed"})
(def ^:private concern-classifications #{"live" "stale" "orphaned" "retired"})

(defn- expected-classification
  "The class concern-cli's `classification-of` MUST have emitted for a row's
   (retired, online, maturity) triple. Retired wins, then online⇒live, then a
   lapsed likely-to-land is orphaned, else stale. Recomputing it lets the consumer
   reject any row whose declared class contradicts its own fields."
  [{:keys [retired online maturity]}]
  (cond
    retired                       "retired"
    online                        "live"
    (= maturity "likely-to-land") "orphaned"
    :else                         "stale"))

(defn- validate-concern-row
  "nil if the row exactly matches the producer contract, else a human error string.
   Rejects missing/extra keys, wrong value types, unknown maturity/classification,
   and a classification inconsistent with (retired, online, maturity)."
  [c]
  (let [{:keys [id agent repo intent maturity classification online retired touches]} c]
    (cond
      (not (map? c))                             (str "row not an object: " (pr-str c))
      (not= (set (keys c)) concern-row-keys)
      (str "row key set " (pr-str (vec (sort (map name (keys c)))))
           " ≠ " (pr-str (vec (sort (map name concern-row-keys)))))
      (not (and (string? id) (seq id)))          (str "row :id not a non-empty string: " (pr-str id))
      (not (or (nil? agent) (string? agent)))    (str "row :agent not string-or-null: " (pr-str agent))
      ;; repo/intent are string-or-null exactly like :agent above: a concern
      ;; declared without a repo or intent emits null, and demanding a string
      ;; here rejected the WHOLE projection over a single row — which is how
      ;; `north doctor` reported "concerns (unavailable)" against 167 perfectly
      ;; readable concerns. Fail closed on a malformed TYPE, never on an absent
      ;; optional value.
      (not (or (nil? repo) (string? repo)))      (str "row :repo not string-or-null: " (pr-str repo))
      (not (or (nil? intent) (string? intent)))  (str "row :intent not string-or-null: " (pr-str intent))
      (not (contains? concern-maturities maturity))
      (str "row :maturity not one of " (pr-str (vec (sort concern-maturities))) ": " (pr-str maturity))
      (not (contains? concern-classifications classification))
      (str "row :classification not one of " (pr-str (vec (sort concern-classifications))) ": " (pr-str classification))
      (not (boolean? online))                    (str "row :online not a boolean: " (pr-str online))
      (not (boolean? retired))                   (str "row :retired not a boolean: " (pr-str retired))
      (not (and (vector? touches) (every? string? touches)))
      (str "row :touches not a vector of strings: " (pr-str touches))
      (not= classification (expected-classification c))
      (str "row :classification " (pr-str classification) " contradicts retired="
           retired " online=" online " maturity=" (pr-str maturity)
           " (expected " (pr-str (expected-classification c)) ")")
      :else nil)))

(defn parse-concern-projection
  "Strictly consume the versioned concern machine projection. Returns
   {:concerns [{:id :status :repo :classification} ...]} of the NON-retired,
   active-by-repository rows, or {:err ...} on ANY deviation from the contract:
   a malformed/wrong-version envelope, OR any row that fails `validate-concern-row`
   (missing/extra key, wrong type, unknown maturity/classification, or a class
   contradicting its own fields). Every row is validated BEFORE retired rows are
   dropped, so a corrupt retired row still fails the whole payload closed. Retired
   (abandoned-stale) rows are then excluded so downstream counts/grouping are over
   active concerns only."
  [text]
  (let [parsed (try (json/parse-string (str text) true)
                    (catch Exception _ ::unparseable))]
    (cond
      (= parsed ::unparseable)          {:err "concern projection unparseable"}
      (not (map? parsed))               {:err "concern projection not an object"}
      (not= (set (keys parsed)) concern-envelope-keys)
      {:err (str "concern projection envelope key set "
                 (pr-str (vec (sort (map name (keys parsed)))))
                 " ≠ " (pr-str (vec (sort (map name concern-envelope-keys)))))}
      (not= (:version parsed) concern-projection-version)
      {:err (str "concern projection version mismatch (want "
                 concern-projection-version ", got " (pr-str (:version parsed)) ")")}
      (not (vector? (:concerns parsed))) {:err "concern projection concerns not a list"}
      (not (every? map? (:concerns parsed))) {:err "concern projection row not an object"}
      :else
      (if-let [bad (some validate-concern-row (:concerns parsed))]
        {:err (str "concern projection " bad)}
        {:concerns
         (->> (:concerns parsed)
              (remove :retired)
              (mapv (fn [c] {:id (:id c) :status (:maturity c)
                             :repo (:repo c) :classification (:classification c)})))}))))

(defn fetch-concern-projection
  "Shell `concern list-json` once and strictly parse it — {:concerns ...} on success
   or {:err ...} on an unavailable probe / malformed payload. UNCACHED: the caller
   owns caching (the dashboard hot path caches; `doctor` reads it live)."
  []
  (let [r (run [(str NORTH "/bin/concern") "list-json"] :timeout 30000)]
    (if (or (:timeout r) (not (:ok r)))
      {:err "concern probe unavailable"}
      (parse-concern-projection (:out r)))))

(defn concern-rows
  "Active concerns grouped by repo. CACHED 90s. `concern list-json` runs the same
   owner-lease decay projection ON the coordinator that `concern ls` does — measured
   12-24s on the current large log — but returns a strict versioned JSON document, so
   the dashboard consumes structured rows instead of scraping rendered text. Active
   concerns move slowly, so a point-in-time dashboard tolerates ~90s staleness. Cache
   miss runs with a 30s budget — it MUST exceed real cost or the probe can never seed.
   Only successful, well-formed reads are cached; a timeout/error/malformed payload
   returns fresh and retries next run (fail closed, never cache a bad projection)."
  []
  (or (cache-get "concerns.edn" 90000)
      (let [parsed (fetch-concern-projection)]
        (if (:err parsed) parsed (cache-put! "concerns.edn" parsed)))))

(defn concern-summary
  "Coarse active/stale concern counts for the health pane, derived from the SAME
   structured machine projection the by-repo pane consumes — NEVER scraped from
   `north health` rendered text. `:active` is every non-retired row (parse already
   dropped retired), `:stale` those the projection classified stale. An {:err ...}
   projection passes straight through so the pane fails closed, not silently zeroed."
  [concern-projection-result]
  (if (:err concern-projection-result)
    concern-projection-result
    {:active (count (:concerns concern-projection-result))
     :stale  (count (filter #(= "stale" (:classification %))
                            (:concerns concern-projection-result)))}))

;; ---- board / ready counts ---------------------------------------------------
(defn board-counts []
  ;; The curated `north threads` header carries all counts on one line
  ;; ("THREADS — N open threads · N active · N ready · N blocked · N concerns"),
  ;; so one shell-out covers what used to need board+ready.
  (let [b (run [(str NORTH "/bin/north") "threads"] :timeout 4000)
        grab (fn [re] (when (:out b) (some-> (re-find re (:out b)) second)))]
    {:open   (grab #"THREADS\s+—\s+(\d+)\s+open")
     :active (grab #"(\d+)\s+active")
     :ready  (grab #"(\d+)\s+ready")
     :err    (when-not (:ok b) "board unavailable")}))

;; ---- north health probe -------------------------------------------------------
(defn north-health
  ;; Runs `north health`; never throws.
  ;; Returns {:raw "..."} on success or {:err "..."} on timeout/error/absent.
  ;; The dashboard's cache-miss path passes a generous budget: under a probe
  ;; burst the single-threaded coordinator serializes clients and health lands
  ;; last, so a tight budget times out and seeds nothing. Doctor runs it alone
  ;; and keeps the default.
  ([] (north-health 4000))
  ([timeout-ms]
   (let [r (run [(str NORTH "/bin/north") "health"] :timeout timeout-ms)]
     (cond
       (:ok r)      {:raw (:out r)}
       (:timeout r) {:err "timed out"}
       :else        {:err "unavailable"}))))

(defn parse-health
  "Extract LANE signals from north-health output: lanes ran/died (24h). Matches on
   the leading label word so spacing/counts can vary freely. Concern counts are NOT
   scraped here — they derive from the structured `concern list-json` projection via
   `concern-summary`, so no dashboard count depends on human-rendered health text."
  [{:keys [raw err]}]
  (if err
    {:err err}
    (let [lines    (str/split-lines (or raw ""))
          find-ln  (fn [label]
                     (some #(when (re-find (re-pattern (str "(?i)^\\s*" label "\\b")) %) %) lines))
          lanes-ln (find-ln "lanes")]
      {:lanes-ran-24h  (some-> (re-find #"24h\s+(\d+)\s+ran" (or lanes-ln "")) second parse-long)
       :lanes-died-24h (some-> (re-find #"24h\s+\d+\s+ran\s+·\s+(\d+)\s+died" (or lanes-ln "")) second parse-long)})))

(defn dashboard-health
  "Health for the dashboard hot path: cached 300s (slow-moving 24h aggregates +
   STALE concern count). Cache miss runs with a 30s budget — `north health` folds
   the whole coordination graph with multi-clause Datalog and measures 21-24s on
   the current large corpus, so the old 8s budget ALWAYS timed out and NEVER seeded (every render lied
   'timed out'). The budget must EXCEED real cost or the cache can never warm; one
   slow seed reseeds for 5 min and every other pane speeds up too. Only successful
   reads are cached; a timeout/error returns fresh and retries next run. Doctor keeps
   the uncached, default-budget `(north-health)` path."
  []
  (or (cache-get "health.edn" 300000)
      (let [h (parse-health (north-health 30000))]
        (if (:err h) h (cache-put! "health.edn" h)))))

(defn primary-repo [name]
  (str HOME "/code/" name "/main"))

(defn source-revision
  "Packaged runtimes identify their immutable inputs; source runs use checkout HEAD."
  ([name repo] (source-revision name repo #(System/getenv %)))
  ([name repo getenv]
   (let [git-result (or (let [result (run ["git" "-C" repo "rev-parse" "--short" "HEAD"] :timeout 2000)]
                          (when (:ok result) result))
                        (run ["git" "-C" (primary-repo name) "rev-parse" "--short" "HEAD"] :timeout 2000))
         git-rev (when (:ok git-result) (not-empty (str/trim (:out git-result))))
         package-mode (when (= name "north") (getenv "NORTH_PACKAGE_MODE"))
         package-rev (case name
                       "north" (getenv "NORTH_PACKAGE_REV")
                       "beagle" (getenv "BEAGLE_PACKAGE_REV")
                       nil)
         identity (cond
                    (not-empty package-rev)
                    {:revision package-rev
                     :origin (if (= package-mode "checkout") "checkout rev" "package rev")}
                    git-rev {:revision git-rev :origin "tree HEAD"}
                    :else {:revision "?" :origin "source rev"})]
     (if (= name "north")
       (assoc identity :package-mode package-mode)
       identity))))

(defn runtime-source-note [package-mode origin]
  (when-not (= package-mode "checkout")
    (if (= origin "package rev")
      "         (installed via nix store; embedded package revision shown above)"
      "         (installed via nix store; tree HEAD is checkout context, not the store closure identity)")))

(defn deployment-drift
  "Compare a runtime revision with the named component's primary checkout.
   Missing primary checkouts are a diagnostic condition, never a doctor failure."
  [name revision]
  (let [primary (primary-repo name)]
    (if-not (.isDirectory (io/file primary))
      {:available false}
      (let [head-result (run ["git" "-C" primary "rev-parse" "HEAD"] :timeout 2000)
            head (when (:ok head-result) (not-empty (str/trim (:out head-result))))
            dirty-result (run ["git" "-C" primary "status" "--porcelain"] :timeout 2000)
            dirty-files (when (:ok dirty-result)
                          (count (remove #(str/starts-with? % "??")
                                         (remove str/blank?
                                                 (str/split-lines (:out dirty-result))))))
            behind-result (when (and head (not= revision "?"))
                            (run ["git" "-C" primary "rev-list" "--count"
                                  (str revision ".." head)] :timeout 2000))
            behind (when (:ok behind-result)
                     (parse-long (str/trim (:out behind-result))))]
        {:available true :behind behind :dirty-files dirty-files}))))

(def runtime-promote-state (str HOME "/.local/state/north/runtime"))

(defn promoted-runtime
  "What `north-runtime promote` currently selects. The deployment directory is
   named by the exact commit, so the selector alone carries the revision; an
   absent selector is the pre-promote state, never a doctor failure."
  ([] (promoted-runtime runtime-promote-state))
  ([root]
   (let [selector (io/file root "current")]
     (if-not (.exists selector)
       {:promoted? false}
       (let [deployment (.getCanonicalPath selector)
             revision (.getName (io/file deployment))]
         (if-not (re-matches #"[0-9a-f]{40}" revision)
           {:promoted? false :malformed deployment}
           {:promoted? true :deployment deployment :revision revision}))))))

;; ============================================================================
;; COMMANDS
;; ============================================================================

(defn cmd-dashboard [args]
  (let [once? (some #{"--once"} args)
        ids? (some #{"--ids"} args)
        tty? (and (not once?) (some? (System/console)))]
    (if once?
      (north.dashboard.state/record! :lanes {:status :ok :data (north.dashboard.collectors/lanes)})
      (north.dashboard.collectors/refresh!))
    (if-not tty?
      (print (north.dashboard.render/render ids?))
      (let [key (atom nil)
            reader (future (try (reset! key (char (.read *in*))) (catch Exception _ nil)))]
        (loop []
        (print "\033[H\033[2J") (print (north.dashboard.render/render ids?)) (flush)
        (Thread/sleep 1000)
        (when (= @key \r) (north.dashboard.collectors/refresh!))
        (when-not (#{\q \u001b} @key)
          (north.dashboard.collectors/refresh!)
          (recur)))))))

(def scheduled-maintenance-tasks
  [:spend-guard :stale-lanes :stale-concerns :worktrees :agent-logs])

(defn maintenance-heartbeat-threshold-ms [task]
  (+ (* 3 (north.worker-policy/task-cadence-ms task))
     (north.worker-policy/task-timeout-ms task)))

(defn maintenance-doctor-line
  "One liveness verdict for an independently scheduled maintenance task."
  [port task]
  (let [worker (name task)
        threshold-ms (maintenance-heartbeat-threshold-ms task)
        {:keys [state age-ms ts]}
        (north.worker-heartbeat/heartbeat-status worker port threshold-ms)]
    (case state
      :fresh
      (str (grn "[ok]  ") " " worker " last completed "
           (north.worker-heartbeat/humanize-age age-ms) " ago (" ts ")")
      :stale
      (str (red "[ERR] ") " " worker " STALE — last completed "
           (north.worker-heartbeat/humanize-age age-ms)
           " ago; inspect `systemctl --user status north-" worker "`")
      :missing
      (str (red "[ERR] ") " " worker " heartbeat MISSING — no successful run; "
           "inspect `systemctl --user status north-" worker "`"))))

(defn maintenance-doctor-lines [port]
  (mapv #(maintenance-doctor-line port %) scheduled-maintenance-tasks))

;; ---- coordinator JVM health -------------------------------------------------
;; A coordinator can be UP, serving the right SpaceId, and still unusable. On
;; 2026-07-29 it sat at old-gen 99.98% with 2,715 full GCs — 1,704s of GC in a
;; 69-minute lifetime, 41% of its wall clock — and `north show @swarm` took 57.8s
;; on an IDLE box after measuring 93ms earlier the same day. Nothing about the
;; query changed; the JVM had stopped being able to allocate.
;;
;; Every existing doctor probe said healthy throughout, because none of them ask
;; whether the process can still do work. Finding it took hours; this makes it
;; one line. Old-gen occupancy alone is not a restart signal: a healthy JVM can
;; legitimately retain a large old generation. Sustained GC time is the evidence
;; that the process is thrashing.
(def OLD-GEN-WARN-PCT 90.0)
(def GC-TIME-ERR-PCT 20.0)   ; share of process lifetime spent in GC

(defn coordinator-pid
  "PID of the coordinator, or nil. Never throws.

  systemd owns the listening socket (the daemon logs 'inherited socket'), so
  `ss -tlnp` reports no `pid=` for it — the unit is the reliable source. `ss`
  remains a fallback for a coordinator started outside systemd."
  [port]
  (or (let [{:keys [out ok]} (run ["systemctl" "--user" "show" "north-store.service"
                                   "-p" "MainPID" "--value"] :timeout 3000)
            pid (some-> out str/trim)]
        (when (and ok (seq pid) (not= pid "0")) pid))
      (let [{:keys [out ok]} (run ["ss" "-tlnp"] :timeout 3000)]
        (when ok
          (some->> (str/split-lines (or out ""))
                   (filter #(str/includes? % (str "127.0.0.1:" port)))
                   first
                   (re-find #"pid=(\d+)")
                   second)))))

(defn parse-gcutil
  "{:old-pct :fgc :gc-seconds} from `jstat -gcutil` output, or nil.

  Column order — getting this wrong is silent, which it was:
    0:S0 1:S1 2:E 3:O 4:M 5:CCS 6:YGC 7:YGCT 8:FGC 9:FGCT 10:CGC 11:CGCT 12:GCT
  GCT is the TOTAL at index 12. Index 11 is CGCT — CONCURRENT GC only — which
  read 0.478s against 1,704s total and rendered as '0% of uptime in GC' on a
  coordinator spending 41% of its life collecting. Parsed here so a wrong index
  fails a test instead of quietly reporting health."
  [out]
  (let [rows (str/split-lines (str out))
        vals (some-> rows second str/trim (str/split #"\s+"))
        num #(try (Double/parseDouble %) (catch Exception _ nil))]
    (when (>= (count (or vals [])) 13)
      (let [old (num (nth vals 3))
            fgc (num (nth vals 8))
            gct (num (nth vals 12))]
        (when (and old gct)
          {:old-pct old :fgc (long (or fgc 0)) :gc-seconds gct})))))

(defn jvm-gc-health
  "{:old-pct :fgc :gc-seconds :uptime-seconds} for a JVM pid, or nil.

  Best-effort: jstat ships with the JDK but the coordinator may be started by
  a runtime that does not expose it, and a diagnostic must degrade rather than
  fail. nil means 'could not measure', which the caller renders distinctly from
  'measured and healthy' — absence is never health."
  [pid]
  (when pid
    (let [jstat (first (for [d (or (seq (.listFiles (io/file "/nix/store"))) [])
                             :when (str/includes? (.getName d) "openjdk")
                             :let [f (io/file d "bin" "jstat")]
                             :when (.canExecute f)]
                         (.getPath f)))]
      (when jstat
        (let [{:keys [out ok]} (run [jstat "-gcutil" (str pid)] :timeout 5000)
              rows (when ok (str/split-lines (or out "")))
              vals (some-> rows second str/trim (str/split #"\s+"))]
          (parse-gcutil out))))))

(defn process-uptime-seconds [pid]
  (when pid
    (let [{:keys [out ok]} (run ["ps" "-o" "etimes=" "-p" (str pid)] :timeout 3000)]
      (when ok (try (Long/parseLong (str/trim (or out ""))) (catch Exception _ nil))))))

(defn coordinator-jvm-line
  "One line: healthy, degraded, or unmeasurable. Never throws."
  [port]
  (let [pid (coordinator-pid port)]
    (cond
      (nil? pid) (str (dim "[--]  ") " coordinator pid not resolvable — GC health unknown")
      :else
      (if-let [{:keys [old-pct fgc gc-seconds]} (jvm-gc-health pid)]
        (let [up (process-uptime-seconds pid)
              gc-pct (when (and up (pos? up)) (* 100.0 (/ gc-seconds up)))
              thrashing? (and gc-pct (>= gc-pct GC-TIME-ERR-PCT))
              high-old-gen? (>= old-pct OLD-GEN-WARN-PCT)
              detail (format "old-gen %.1f%% · %d full GC%s%s"
                             old-pct fgc (if (= 1 fgc) "" "s")
                             (if gc-pct (format " · %.0f%% of uptime in GC" gc-pct) ""))]
          (cond
            thrashing?
            (str (red "[ERR] ") " coordinator JVM is thrashing: " detail
                 " — operator intervention required")

            high-old-gen?
            (str (ylw "[warn]") " coordinator JVM old-gen occupancy is high: " detail
                 " — observe GC-time share; no cutover warranted yet")

            :else
            (str (grn "[ok]  ") " coordinator JVM " detail)))
        (str (dim "[--]  ") " coordinator GC health unmeasurable (no jstat)")))))

(def doctor-failed? (atom false))
(defn mark-doctor-failed! [] (reset! doctor-failed? true))

;; Keep an absent-recipient failure loud for one bounded recovery window. North
;; session leases last 30 minutes and dead lanes are reaped after a further
;; 30-minute lapse bar; an hour therefore leaves time to inspect or reroute a
;; newly broken delivery. Older mail remains visible below as unacknowledged
;; history, but cannot pin doctor's process status red forever. A missing or
;; malformed age fails closed as recent.
(def DEAD-LETTER-ACTION-WINDOW-MS (* 60 60 1000))

(defn actionable-dead-letter? [{:keys [age-ms]}]
  (or (nil? age-ms)
      (< age-ms DEAD-LETTER-ACTION-WINDOW-MS)))

(defn render-dead-letters! [port]
  (println (bold "  dead letters"))
  (let [{:keys [rows error]}
        (north.message-routing/readiness-dead-letter-scan
         (Integer/parseInt (str port))
         DEAD-LETTER-ACTION-WINDOW-MS)]
    (cond
      error
      (do
        (mark-doctor-failed!)
        (println (str "    " (red "[ERR] ")
                      "dead-letter scan unavailable: " error)))

      (empty? rows)
      (println (str "    " (grn "[ok]  ")
                    "no pending mail targets an absent identity"))

      :else
      (let [{actionable true historical false}
            (group-by actionable-dead-letter? rows)]
        (when (seq actionable)
          (mark-doctor-failed!)
          (println (str "    " (red "[ERR] ") (count actionable)
                        " recent pending message(s) target absent identities"))
          (println (format "    %-24s %-34s %s" "SENDER" "RECIPIENT" "AGE"))
          (doseq [{:keys [sender recipient resolved-recipient age]} actionable]
            (println
             (format "    %-24s %-34s %s"
                     sender
                     (if (= recipient resolved-recipient)
                       recipient
                       (str recipient " -> " resolved-recipient))
                     age))))
        (when (seq historical)
          (let [oldest (apply max-key #(or (:age-ms %) -1) historical)]
            (println (str "    " (ylw "[warn]") " " (count historical)
                          " unacknowledged message(s) outside the "
                          (quot DEAD-LETTER-ACTION-WINDOW-MS 3600000)
                          "h action window; oldest " (:age oldest)))))
        (when (empty? actionable)
          (println (str "    " (grn "[ok]  ")
                        "no recent pending mail targets absent identities")))))))

;; ---- coordination health ----------------------------------------------------
;; Doctor was green for months while `north agents` printed nothing and the
;; presence table was empty. Roster darkness is now a RED doctor finding.
(def COORDINATION-PROBE-TIMEOUT-MS 20000)
(def ROSTER-PROBE-TIMEOUT-MS 25000)

(defn coordination-probe
  "Exercise the hook client's direct Store RPC path, including a lease write and
   readback, without routing the probe through the dashboard wrapper."
  []
  (let [r (run ["bb" (str NORTH "/cli/presence-cli.clj") PORT
                "coordination-probe-json"]
               :timeout COORDINATION-PROBE-TIMEOUT-MS)
        payload (try (json/parse-string (str/trim (or (:out r) "")) true)
                     (catch Exception _ nil))]
    (cond
      (:timeout r) {:err (str "coordination probe exceeded its "
                              COORDINATION-PROBE-TIMEOUT-MS "ms budget")}
      (not (map? payload))
      {:err (str "coordination probe returned no usable payload"
                 (when-let [e (not-empty (str/trim (or (:err r) "")))] (str ": " e)))}
      (not= "north:coordination-probe:v1" (:version payload))
      {:err "coordination probe returned an unrecognised contract"}
      :else payload)))

(defn roster-projection-probe []
  (let [r (run [(str NORTH "/bin/north") "agents" "--json"]
               :timeout ROSTER-PROBE-TIMEOUT-MS)]
    (cond
      (:timeout r) {:err (str "roster projection exceeded its "
                              ROSTER-PROBE-TIMEOUT-MS "ms budget")}
      (not (:ok r)) {:err (str "roster projection failed"
                               (when-let [e (not-empty (str/trim (or (:err r) "")))]
                                 (str ": " e)))}
      :else
      (let [payload (try (json/parse-string (str/trim (or (:out r) "")) true)
                         (catch Exception _ nil))]
        (if (and (map? payload)
                 (= "north:agent-roster:v1" (:version payload))
                 (vector? (:agents payload)))
          {:entries (count (:agents payload))}
          {:err "roster projection did not emit north:agent-roster:v1"})))))

(defn render-coordination-health! []
  (println (bold "  coordination health"))
  (echo-cmd "bb" (str NORTH "/cli/presence-cli.clj") PORT
            "coordination-probe-json")
  (let [probe (coordination-probe)
        fail! (fn [msg] (mark-doctor-failed!)
                (println (str "    " (red "[ERR] ") " " msg)))
        ok! (fn [msg] (println (str "    " (grn "[ok]  ") " " msg)))]
    (if-let [e (:err probe)]
      (fail! e)
      (let [{:keys [error space_fence_ok space_id lease_write_readback_ok
                    live_session_leases lineage_registrations_in_ttl lease_ttl_ms]} probe]
        (cond
          (false? space_fence_ok)
          (fail! "Store RPC status did not return a SpaceId")

          (nil? space_fence_ok)
          (fail! (str "coordination probe failed: " error))

          :else (ok! (str "Store RPC SpaceId fence " space_id)))
        (when (some? space_fence_ok)
          (if lease_write_readback_ok
            (ok! "presence write + readback through the hook path")
            (fail! "presence lease did not survive write + readback through the hook path")))
        (when (and space_fence_ok error)
          (fail! (str "coordination probe failed: " error)))
        (when (and space_fence_ok (not error))
          (let [ttl-min (when (integer? lease_ttl_ms) (quot lease_ttl_ms 60000))
                summary (str live_session_leases " live lease(s) · "
                             lineage_registrations_in_ttl
                             " session registration(s) in the " ttl-min "m TTL")]
            (if (and (integer? live_session_leases) (zero? live_session_leases)
                     (integer? lineage_registrations_in_ttl)
                     (pos? lineage_registrations_in_ttl))
              (fail! (str "presence is DARK: " summary
                          " — the roster cannot see sessions the lineage hooks registered"))
              (ok! (str "presence " summary)))))))
    (let [roster (roster-projection-probe)]
      (if-let [e (:err roster)]
        (fail! e)
        (ok! (str "roster projection north:agent-roster:v1 · "
                  (:entries roster) " entries"))))))

(defn cmd-doctor [_]
  (reset! doctor-failed? false)
  (println (bold "north doctor"))
  ;; Coordinator safety is a bounded runtime-identity + SpaceId round trip.
  (println (bold "  coordinator"))
  (echo-cmd (str NORTH "/bin/north") "coord-safety")
  (let [{:keys [timeout timeout-ms ok out err error]} (coord-safety-probe)]
    (cond
      timeout
      (do
        (mark-doctor-failed!)
        (println (str "    " (red "[ERR] ") " coord-safety exceeded its "
                      timeout-ms "ms budget — readiness was not inferred")))

      (not ok)
      (do
        (mark-doctor-failed!)
        (println (str "    " (red "[ERR] ") " coord-safety failed"
                      (when (seq (str/trim (or error err "")))
                        (str ": " (str/trim (or error err "")))))))

      :else
      (doseq [ln (remove str/blank? (str/split-lines (str (or out "") (or err ""))))]
        (println (str "    " ln)))))
  ;; daemons
  (let [dh (daemon-health)]
    (println (bold "  daemons"))
    (doseq [[label k crit] [[(str PORT " coordination (the Store RPC authority)") :north true]]]
      (let [up (get dh k)]
        (when (and crit (not up)) (mark-doctor-failed!))
        (println (str "    " (if up (grn "[ok]  ") (if crit (red "[ERR] ") (ylw "[warn]")))
                      " " label " " (ok-x up))))))
  ;; A coordinator can be UP, serving the right SpaceId, and still unable to work.
  ;; Nothing else here asks that question — see coordinator-jvm-line.
  (println (bold "  coordinator JVM"))
  (let [jvm-line (coordinator-jvm-line PORT)]
    (when (str/includes? jvm-line "[ERR]") (mark-doctor-failed!))
    (println (str "    " jvm-line)))
  (println (bold "  scheduled maintenance"))
  (doseq [line (maintenance-doctor-lines PORT)]
    (when (str/includes? line "[ERR]") (mark-doctor-failed!))
    (println (str "    " line)))
  (render-dead-letters! PORT)
  (render-coordination-health!)
  ;; 24h/7d activity is observability, not readiness. `north health` currently
  ;; runs a deliberately broad aggregate and can take tens of seconds on the
  ;; large graph. Doctor consumes only a recent successful dashboard cache and
  ;; never starts that workload itself.
  (println (bold "  activity summary"))
  (if-let [{:keys [lanes-ran-24h lanes-died-24h]}
           (cache-get "health.edn" 300000)]
    (println (str "    " (grn "[ok]  ") " cached "
                  (or lanes-ran-24h "?") " ran"
                  (when lanes-died-24h (str " · " lanes-died-24h " died"))
                  " (24h)"))
    (println (str "    " (ylw "[warn] ")
                  "no recent aggregate cache; readiness checks continue"
                  (dim " (run `north health` explicitly for the broad rollup)"))))
  ;; Runtime source identity. A package revision identifies the
  ;; installed closure; a checkout HEAD is only source context, not proof that a
  ;; separately installed store path contains that tree.
  (println (bold "  runtime source identity"))
  (doseq [[name repo] [["north" NORTH] ["store" STORE] ["beagle" BEAGLE]]]
    (let [{:keys [revision origin package-mode]} (source-revision name repo)
          command-result (run ["bash" "-c" "command -v \"$1\"" "north-doctor" name] :timeout 1500)
          which (when (:ok command-result)
                  (some-> (:out command-result) str/trim not-empty
                          io/file .getCanonicalPath))
          store (some->> which (re-find #"/nix/store/[^/]+"))]
      (println (str "    " (cyn name) "  " origin " " revision
                    "  installed " (or store which "?")))
      (when (and store (not (str/includes? (or which "") repo)))
        (when-let [note (runtime-source-note package-mode origin)]
          (println (dim note)))
      (let [{:keys [available behind dirty-files]} (deployment-drift name revision)]
        (if-not available
          (println (str "    " (ylw "[warn] ") name ": repo main unavailable"))
          (do
            (println (str "    " (if (and behind (pos? behind)) (ylw "[warn] ") (grn "[ok]  "))
                          name ": running " (or behind "?") " commits behind repo main"))
            (when (pos? (or dirty-files 0))
              (mark-doctor-failed!)
              (println (str "    " (red "[ERR] ") "PRIMARY DIRTY: " dirty-files
                            " files — snapshot builds EXCLUDE these (silent-exclusion risk)")))))))))
  ;; North's own runtime ships by promote, so its selected revision is a
  ;; different question from the installed closure printed above.
  (let [{:keys [promoted? revision deployment malformed]} (promoted-runtime)]
    (cond
      malformed
      (println (str "    " (ylw "[warn] ") " promoted north runtime: selector does not "
                    "resolve to a revision-named deployment (" malformed ")"))

      (not promoted?)
      (println (str "    " (dim "[--]  ") " promoted north runtime: no promote yet"
                    (dim (str " (bin/north-runtime promote " (primary-repo "north") " HEAD)"))))

      :else
      (let [{:keys [available behind]} (deployment-drift "north" revision)]
        (println (str "    " (cyn "north") "  promoted rev " (subs revision 0 7)
                      "  " deployment))
        (if-not available
          (println (str "    " (ylw "[warn] ") " promoted north runtime: repo main unavailable"))
          (println (str "    " (if (and behind (pos? behind)) (ylw "[warn] ") (grn "[ok]  "))
                        " promoted north runtime: " (or behind "?")
                        " commits behind repo main"))))))
  ;; guard hooks present
  (println (bold "  guard hooks"))
  (let [hookdir (str HOME "/.agents/hooks")
        settings (str HOME "/.claude/settings.json")
        stxt (when (.exists (io/file settings)) (slurp settings))]
    (doseq [h ["agent-spawn-guard.sh" "tripwire-guard.sh"]]
      (let [present (or (.exists (io/file hookdir h))
                        (some #(.exists (io/file hookdir %)) [(str h ".sh") h]))
            wired (and stxt (str/includes? stxt (str/replace h #"\.sh$" "")))]
        (println (str "    " (if present (grn "[ok]  ") (ylw "[warn]"))
                      " " (format "%-22s" h)
                      (if present "file present" "not found")
                      (when wired (dim "  · wired in settings.json")))))))
  (not @doctor-failed?))

;; ---- dispatch ---------------------------------------------------------------
(when-not (= (System/getenv "NORTH_DASHBOARD_LIB") "1")
  (let [[cmd & args] *command-line-args*]
    (case cmd
      (nil "dashboard") (cmd-dashboard args)
      "doctor"          (when-not (cmd-doctor args) (System/exit 1))
      (do (binding [*out* *err*] (println (red (str "dashboard-cli: unknown command: " cmd))))
          (System/exit 2)))))
