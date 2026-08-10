(ns north.dashboard.render
  (:require [clojure.string :as str] [north.dashboard.state :as state]))

(defn width []
  (try (max 1 (Long/parseLong (or (System/getenv "COLUMNS") "100")))
       (catch Exception _ 100)))
(defn color? [] (and (nil? (System/getenv "NO_COLOR"))
                     (or (some? (System/getenv "FORCE_COLOR"))
                         (some? (System/console)))))
(defn dim [s] (if (color?) (str "\u001b[2m" s "\u001b[0m") s))
(defn bold [s] (if (color?) (str "\u001b[1m" s "\u001b[0m") s))
(defn paint [code s] (if (color?) (str "\u001b[" code "m" s "\u001b[0m") s))
(def ansi-pattern #"\u001b\[[0-9;]*m")
(defn strip-ansi [s] (str/replace (str (or s "")) ansi-pattern ""))
(defn clip [s n]
  (let [s (str (or s ""))]
    (if (> (count s) n) (str (subs s 0 (max 0 (dec n))) "…") s)))
(defn clip-visible [s n]
  (let [s (str (or s ""))]
    (if (<= (count (strip-ansi s)) n) s
      (loop [tokens (re-seq #"\u001b\[[0-9;]*m|." s) visible 0 out ""]
        (let [token (first tokens)]
          (if (or (nil? token) (>= visible (max 0 (dec n))))
            (str out "…" (if (color?) "\u001b[0m" ""))
            (if (re-matches ansi-pattern token)
              (recur (rest tokens) visible (str out token))
              (recur (rest tokens) (inc visible) (str out token)))))))))
(defn age [ms]
  (let [s (long (max 0 (or ms 0)))]
    (cond (< s 60000) (str (quot s 1000) "s")
          (< s 3600000) (str (quot s 60000) "m")
          (< s 86400000) (str (quot s 3600000) "h")
          :else (str (quot s 86400000) "d"))))
(defn panel-status [panel]
  (let [e (state/read-panel panel) attempt (:last-attempt e) ms (state/age-ms e)]
    (cond
      (= "error" (:status attempt)) (str "panel error: " (:detail attempt) " (retrying)")
      (nil? (get e :last-good)) "collecting"
      (and attempt (not= "ok" (:status attempt)))
      (str "failed " (age (max 0 (- (state/now) (:at attempt)))) " ago")
      :else (str "data " (age ms) " old"))))
(defn header [name panel] (str (bold name) " " (dim (str "· " (panel-status panel)))))
(defn terminal? [status] (#{"finished" "failed"} status))
(declare fixed-column)
(defn agent-status [status]
  (cond
    (= status "advancing") "running"
    ;; re-find on a groupless regex returns the match string itself; taking
    ;; first of it yields a character and broke retention for every live lane.
    (or (= status "live quiet") (str/starts-with? status "working (quiet "))
    (or (re-find #"quiet [0-9]+m" status) "quiet")
    (= status "finished") "done"
    (= status "failed") "crashed"
    :else "vanished"))
(defn pad-then-paint [code value width]
  (paint code (fixed-column value width)))
(defn agent-label [status width]
  (let [agent (agent-status status)]
    (cond
      (= agent "running") (pad-then-paint 32 agent width)
      (str/starts-with? agent "quiet") (dim (pad-then-paint 32 agent width))
      (= agent "crashed") (pad-then-paint 31 agent width)
      (= agent "vanished") (pad-then-paint 33 agent width)
      :else (dim (fixed-column agent width)))))
(defn work-label [lane width]
  (let [work (or (:work lane)
                 (case (agent-status (:status lane))
                   "vanished" "unknown"
                   ("done" "crashed") "none"
                   "unknown"))]
    (case work
      "delivered" (pad-then-paint 32 work width)
      "none" (pad-then-paint 31 work width)
      "unknown" (pad-then-paint 33 work width)
      (fixed-column work width))))
(def model-labels
  {"sol" "GPT 5.6 Sol" "gpt-5.6-sol" "GPT 5.6 Sol"
   "terra" "GPT 5.6 Terra" "gpt-5.6-terra" "GPT 5.6 Terra"
   "luna" "GPT 5.6 Luna" "gpt-5.6-luna" "GPT 5.6 Luna"})
(defn model-label [model] (or (get model-labels model) model))
(defn lane-details [{:keys [role effort provider model]}]
  (let [role-effort (str/join "/" (remove str/blank? [role effort]))
        identity (if (str/blank? model) provider (model-label model))]
    (str/join " · " (remove str/blank? [role-effort identity]))))
(defn title-slug [title]
  (-> title str/lower-case (str/replace #"[^a-z0-9]+" "-") (str/replace #"^-|-$" "") (clip 28)))
(defn lane-title [{:keys [id title] :as lane}]
  (if (and (seq title) (not= title id)) title "(untitled)"))
(declare started-at)
(defn spawn-time [lane]
  (when-let [ms (started-at lane)]
    (try
      (let [instant (java.time.Instant/ofEpochMilli (long ms))
            zone (java.time.ZoneId/systemDefault)
            date (.toLocalDate (.atZone instant zone))
            now-date (.toLocalDate (.atZone (java.time.Instant/ofEpochMilli (state/now)) zone))
            pattern "HH:mm"]
        (.format (java.time.format.DateTimeFormatter/ofPattern pattern) (.atZone instant zone)))
      (catch Exception _ "—"))))
(def terminal-retention-ms 600000)
(def vanished-retention-ms 1800000)
(def fleet-column-defaults {:details 34 :task 26 :agent 9 :work 9 :wall 4 :started 7})
(def fleet-column-minimums {:details 13 :task 4 :agent 5 :work 4 :wall 4 :started 7})
(def queue-columns {:marker 5 :task 56 :id 8 :unblocks 8})
(def account-columns {:account 38 :status 10 :used 3 :resets 8})
(defn fleet-columns []
  (let [available (- (width) 7 (reduce + (vals (dissoc fleet-column-defaults :details))))
        details (max (:details fleet-column-minimums) (min (:details fleet-column-defaults) available))]
    (assoc fleet-column-defaults :details details)))
(defn retained? [{:keys [status last-output-age]}]
  (let [age (or last-output-age Long/MAX_VALUE)
        agent (agent-status status)]
    (or (or (= agent "running") (str/starts-with? agent "quiet"))
        (and ((set ["done" "crashed"]) agent) (< age terminal-retention-ms))
        (and (= agent "vanished") (< age vanished-retention-ms)))))
(defn fixed-column [value width]
  (format (str "%-" width "s") (clip value width)))
(defn fleet-header [columns]
  (dim (str "  " (fixed-column "agent · model" (:details columns)) " "
            (fixed-column "task" (:task columns)) " " (fixed-column "agent" (:agent columns)) " "
            (fixed-column "work" (:work columns)) " " (fixed-column "wall" (:wall columns)) " "
            (fixed-column "started" (:started columns)))))
(defn queue-header []
  (dim (str "  " (fixed-column "" (:marker queue-columns)) " "
            (fixed-column "task" (:task queue-columns)) "  "
            (fixed-column "id" (:id queue-columns)) "  "
            (fixed-column "unblocks" (:unblocks queue-columns)))))
(defn account-header []
  (dim (str "  " (fixed-column "account" (:account account-columns)) " "
            (fixed-column "status" (:status account-columns)) " "
            (format "%3s" "used") " " (fixed-column "resets" (:resets account-columns)))))
(defn fleet-lines [lanes ids?]
  (let [columns (fleet-columns)
        visible (->> lanes
                     (filter retained?)
                     (sort-by :last-output-age)
                     vec)
        shown (take 12 visible)]
    (concat
      (if (seq shown)
        (for [{:keys [id status last-output-age title] :as lane} shown]
          (let [details (lane-details lane)
                details (if (and (str/blank? details) (or (str/blank? title) (= title id)))
                          (subs id 0 (min 8 (count id))) details)]
            (str "  " (fixed-column details (:details columns)) " "
                 (fixed-column (lane-title lane) (:task columns)) " "
                 (agent-label status (:agent columns)) " "
                 (work-label lane (:work columns)) " "
                 (fixed-column (age last-output-age) (:wall columns)) " "
                 (format (str "%-" (:started columns) "s") (clip (spawn-time lane) 5))
                 (if ids? (str " " (dim (subs id 0 (min 8 (count id))))) ""))))
        ;; Data present but every lane filtered by retention is a real, calm
        ;; state — never wear the same face as a collector that has no data.
        [(if (seq lanes) "  no recent lane activity" "  collecting…")])
      (when (> (count visible) 12) [(str "  (+" (- (count visible) 12) " older)")]))))
(defn bytes [n]
  (try (let [n (Double/parseDouble (str n))]
         (cond (< n 1024) (str (long n) "B")
               (< n (* 1024 1024)) (format "%.1fK" (/ n 1024))
               (< n (* 1024 1024 1024)) (format "%.1fM" (/ n 1048576))
               :else (format "%.1fG" (/ n 1073741824))))
       (catch Exception _ "—")))
(defn health-lines [services]
  (if (seq services)
    (for [[unit {:keys [active socket memory]}] services
          :let [name (str/replace unit #"\.service$" "")
                current (get memory "memory.current") max (get memory "memory.max")]]
      (str "  " name " :" (if (= name "north-fram") "7977" "7978")
           "  process " (if active "up" "down") " · socket " (if socket "ok" "down")
           " · mem " (bytes current) "/" (bytes max)))
    ["  collecting…"]))
(defn board-counts [text]
  (let [n #(some-> (re-find (re-pattern (str "\\b(\\d+) " % "\\b")) text) second)]
    (str (or (n "active") "0") " active · "
         (or (n "ready") "0") " ready · "
         (or (n "blocked") "0") " blocked")))
(defn board-entry [line]
  (when-let [[_ leverage id title] (re-find #"(?i)^\s*(?:unblocks\s+(\d+)\s+)?(?:\S+\s+)?([0-9a-f]{8}-[0-9a-f-]{27,})\s+(.+)" line)]
    {:id id :title title :leverage (if leverage (Long/parseLong leverage) 0)}))
(defn board-section [lines label]
  (let [after (drop-while #(not (re-find (re-pattern (str "^" label "(?:\\s|$)|" label "\\s+—")) (str/trim %))) lines)]
    (->> (rest after)
         (take-while #(not (or (str/blank? %) (re-find #"^[A-Z][A-Z _-]+$" (str/trim %)))))
         (keep board-entry))))
(defn started-at [lane]
  (try (.toEpochMilli (java.time.Instant/parse (:startedAt lane)))
       (catch Exception _ (:started-at lane))))
(defn active-lanes [lanes]
  (into {} (for [lane lanes
                 :when (#{"advancing" "live quiet"} (:status lane))]
             [(:thread lane) lane])))
(defn queue-lines [text lanes]
  (if-not (seq text) ["  collecting…"]
    (let [ls (str/split-lines text)
          live (active-lanes lanes)
          active (->> (board-section ls "(?:🔵\\s+)?ACTIVE")
                      (keep #(when-let [lane (get live (:id %))]
                               (assoc % :lane lane)))
                      (sort-by #(or (started-at (:lane %)) Long/MAX_VALUE)))
          ready (sort-by (comp - :leverage) (board-section ls "(?:🟢\\s+)?READY"))
          visible (take 8 (concat active ready))
          shown-ready (count (filter #(nil? (:lane %)) visible))]
      (concat [(dim (str "  " (board-counts text)))]
              (if (seq visible)
                (map (fn [{:keys [id title leverage lane]}]
                       (let [stint (when lane (age (max 0 (- (state/now) (or (started-at lane) 0)))))]
                         (str "  " (fixed-column (if lane (str "● " stint) "○") (:marker queue-columns)) " "
                              (fixed-column title (:task queue-columns)) "  "
                              (dim (fixed-column (subs id 0 (min 8 (count id))) (:id queue-columns))) "  "
                              (format (str "%" (:unblocks queue-columns) "s") (if lane "" leverage)))))
                     visible)
                ["  no queued entries"])
              (when (< shown-ready (count ready))
                [(str "  (+" (- (count ready) shown-ready) " more ready)")])))))
(defn reset-age [at]
  (try
    (let [ms (- (.toEpochMilli (java.time.Instant/parse at)) (state/now))]
      (cond (<= ms 0) "now"
            (< ms 3600000) (str (max 1 (quot ms 60000)) "m")
            (< ms 86400000) (str (max 1 (quot ms 3600000)) "h")
            :else (str (max 1 (quot ms 86400000)) "d")))
    (catch Exception _ "—")))
(defn account-lines [document]
  (let [targets (mapcat :targets (:providers document))]
    (if (seq targets)
      (for [{:keys [id routing usage]} targets]
        (let [window (first (get usage :windows))]
          (str "  " (fixed-column id (:account account-columns)) " "
               (cond (= routing "exhausted") (pad-then-paint 31 routing (:status account-columns))
                     (= routing "eligible") (pad-then-paint 32 routing (:status account-columns))
                     :else (fixed-column routing (:status account-columns))) " "
               (format "%3s" (if window (str (:usedPercent window) "%") "—")) " "
               (fixed-column (reset-age (:resetsAt window)) (:resets account-columns)))))
      ["  collecting…"])))
(defn render ([] (render false)) ([ids?]
  (let [lanes (get-in (state/read-panel :lanes) [:last-good :data :lanes])
        health (get-in (state/read-panel :health) [:last-good :data :services])
        board (get-in (state/read-panel :board) [:last-good :data :text])
        providers (get-in (state/read-panel :providers) [:last-good :data])
        lines (concat ["north dashboard" "" (header "FLEET" :lanes) (fleet-header (fleet-columns))] (fleet-lines lanes ids?)
                      ["" (header "HEALTH" :health)] (health-lines health)
                      ["" (header "QUEUE" :board) (queue-header)] (queue-lines board lanes)
                      ["" (header "ACCOUNTS" :providers) (account-header)] (account-lines providers)
                      ["" (dim "agent: running/quiet = live · done/crashed = ended · vanished = gone")
                       (dim "work: delivered = result or commit · none = ended empty · pending = still working · unknown = vanished")])]
    (str (str/join "\n" (map #(clip-visible % (width)) (take 40 lines))) "\n"))))
