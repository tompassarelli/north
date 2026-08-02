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
(defn clip [s n]
  (let [s (str (or s ""))]
    (if (> (count s) n) (str (subs s 0 (max 0 (dec n))) "…") s)))
(defn age [ms]
  (let [s (long (max 0 (or ms 0)))]
    (cond (< s 60000) (str (quot s 1000) "s")
          (< s 3600000) (str (quot s 60000) "m")
          (< s 86400000) (str (quot s 3600000) "h")
          :else (str (quot s 86400000) "d"))))
(defn panel-status [panel]
  (let [e (state/read-panel panel) attempt (:last-attempt e) ms (state/age-ms e)]
    (cond
      (nil? (get e :last-good)) "collecting"
      (and attempt (not= "ok" (:status attempt)))
      (str "failed " (age (max 0 (- (state/now) (:at attempt)))) " ago")
      :else (str "data " (age ms) " old"))))
(defn header [name panel] (str (bold name) " " (dim (str "· " (panel-status panel)))))
(defn terminal? [status] (#{"finished" "failed"} status))
(defn lane-status [status]
  (cond
    (or (= status "advancing") (= status "live quiet") (str/starts-with? status "working (quiet ")) "working"
    (= status "finished") "done"
    (= status "failed") "failed"
    :else "lost"))
(defn status-label [status]
  (let [s (lane-status status) label (if (= s "working") (if (str/starts-with? status "working (quiet ") status s) s)]
    (case s
      "working" (paint 32 (if (str/starts-with? status "working (quiet ") (dim label) label))
      "failed" (paint 31 s)
      "lost" (paint 33 s)
      (dim s))))
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
  (if (and (seq title) (not= title id)) title (dim "(untitled)")))
(declare started-at)
(defn spawn-time [lane]
  (when-let [ms (started-at lane)]
    (try
      (let [instant (java.time.Instant/ofEpochMilli (long ms))
            zone (java.time.ZoneId/systemDefault)
            date (.toLocalDate (.atZone instant zone))
            now-date (.toLocalDate (.atZone (java.time.Instant/ofEpochMilli (state/now)) zone))
            pattern (if (= date now-date) "HH:mm" "EEE HH:mm")]
        (.format (java.time.format.DateTimeFormatter/ofPattern pattern) (.atZone instant zone)))
      (catch Exception _ "—"))))
(def terminal-retention-ms 600000)
(def lost-retention-ms 1800000)
(def details-width 34)
(defn retained? [{:keys [status last-output-age]}]
  (let [age (or last-output-age Long/MAX_VALUE)
        state (lane-status status)]
    (or (= state "working")
        (and ((set ["done" "failed"]) state) (< age terminal-retention-ms))
        (and (= state "lost") (< age lost-retention-ms)))))
(defn fixed-column [value width]
  (format (str "%-" width "s") (clip value width)))
(defn fleet-header []
  (dim (str "  " (fixed-column "agent · model" details-width) " "
            (fixed-column "task" 34) " " (fixed-column "status" 8) " "
            (fixed-column "wall" 4) " started")))
(defn queue-header []
  (dim (str "  " (fixed-column "task" 56) "  " (fixed-column "id" 8) "  unblocks")))
(defn account-header []
  (dim (str "  " (fixed-column "account" 38) " " (fixed-column "status" 10) " "
            (fixed-column "used" 4) " resets")))
(defn fleet-lines [lanes ids?]
  (let [visible (->> lanes
                     (filter retained?)
                     (sort-by :last-output-age)
                     vec)
        shown (take 12 visible)]
    (concat
      (if (seq shown)
        (for [{:keys [id status last-output-age title] :as lane} shown]
          (let [details (lane-details lane)
                details (if (and (str/blank? details) (or (str/blank? title) (= title id)))
                          (dim (subs id 0 (min 8 (count id)))) details)]
            (str "  " (format (if ids? "%s %-34s %-8s %-4s %-12s %s"
                                  "%s %-34s %-8s %-4s %s")
                               (fixed-column details details-width)
                             (clip (lane-title lane) 34)
                             (status-label status)
                             (age last-output-age)
                             (spawn-time lane)
                             (when ids? (dim (subs id 0 (min 8 (count id)))))))))
        ["  collecting…"])
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
      (str "  " name " :" (if (= name "north-coord") "7977" "7978")
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
      (concat [(str "  " (board-counts text))]
              (if (seq visible)
                (map (fn [{:keys [id title leverage lane]}]
                       (let [stint (when lane (age (max 0 (- (state/now) (or (started-at lane) 0)))))]
                         (str "  " (if lane (paint 32 "●") (dim "○"))
                              (when stint (str " " stint)) "  " (clip title 56)
                              "  " (dim (subs id 0 (min 8 (count id))))
                              (when-not lane (str "  unblocks " leverage)))))
                     visible)
                ["  no queued entries"])
              (when (< shown-ready (count ready))
                [(str "  (+" (- (count ready) shown-ready) " more ready)")])))))
(defn reset-age [at]
  (try
    (let [ms (- (.toEpochMilli (java.time.Instant/parse at)) (state/now))]
      (cond (<= ms 0) "now"
            (< ms 3600000) (str "resets " (max 1 (quot ms 60000)) "m")
            (< ms 86400000) (str "resets " (max 1 (quot ms 3600000)) "h")
            :else (str "resets " (max 1 (quot ms 86400000)) "d")))
    (catch Exception _ "resets —")))
(defn account-lines [document]
  (let [targets (mapcat :targets (:providers document))]
    (if (seq targets)
      (for [{:keys [id routing usage]} targets]
        (let [window (first (get usage :windows))]
          (str "  " (format "%-38s %-10s %4s %s"
                           (clip id 38)
                           (if (= routing "exhausted") (paint 31 routing) (if (= routing "eligible") (paint 32 routing) routing))
                           (if window (str (:usedPercent window) "%") "usage —")
                           (reset-age (:resetsAt window))))))
      ["  collecting…"])))
(defn render ([] (render false)) ([ids?]
  (let [lanes (get-in (state/read-panel :lanes) [:last-good :data :lanes])
        health (get-in (state/read-panel :health) [:last-good :data :services])
        board (get-in (state/read-panel :board) [:last-good :data :text])
        providers (get-in (state/read-panel :providers) [:last-good :data])
        lines (concat ["north dashboard" "" (header "FLEET" :lanes) (fleet-header)] (fleet-lines lanes ids?)
                      ["" (header "HEALTH" :health)] (health-lines health)
                      ["" (header "QUEUE" :board) (queue-header)] (queue-lines board lanes)
                      ["" (header "ACCOUNTS" :providers) (account-header)] (account-lines providers)
                      ["" (dim "working = producing output · done/failed = finished · lost = died without reporting")])]
    (str (str/join "\n" (map #(clip % (width)) (take 40 lines))) "\n"))))
