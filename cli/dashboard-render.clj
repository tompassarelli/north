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
      (> (or ms 0) 60000) (str (age ms) " stale")
      :else "fresh")))
(defn header [name panel] (str (bold name) " " (dim (str "· " (panel-status panel)))))
(defn terminal? [status] (#{"finished" "failed"} status))
(defn lane-status [status]
  (case status
    ("advancing" "live quiet") "working"
    "finished" "done"
    "failed" "failed"
    "stale"))
(defn status-label [status]
  (let [s (lane-status status)]
    (case s
      "working" (paint 32 s)
      "failed" (paint 31 s)
      "stale" (paint 33 s)
      (dim s))))
(defn lane-title [{:keys [id title role provider]}]
  (let [fallback (str/join " · " (remove str/blank? [role provider]))]
    (cond
      (and (seq title) (not= title id)) title
      (seq fallback) fallback
      :else id)))
(defn fleet-lines [lanes]
  (let [visible (->> lanes
                     (filter (fn [{:keys [status last-output-age pid]}]
                               (and (or (not (terminal? status)) (< (or last-output-age Long/MAX_VALUE) 600000))
                                    (or pid (< (or last-output-age Long/MAX_VALUE) 86400000)))))
                     (sort-by :last-output-age)
                     vec)
        shown (take 12 visible)]
    (concat
      (if (seq shown)
        (for [{:keys [id status last-output-age] :as lane} shown]
          (str "  " (format "%-44s" (clip (lane-title lane) 44))
               " " (status-label status)
               " " (dim (age last-output-age))
               " " (dim (clip id 8))))
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
  (when-let [[_ id title] (re-find #"(?i)\b([0-9a-f]{8}-[0-9a-f-]{27,})\b\s+(.+)" line)]
    {:id id :title title}))
(defn board-lines [text]
  (if-not (seq text) ["  collecting…"]
    (let [ls (str/split-lines text)
          after-active (drop-while #(not (re-find #"(^ACTIVE(?:\s|$)|ACTIVE\s+—)" (str/trim %))) ls)
          entries (->> (rest after-active)
                       (take-while #(not (or (str/blank? %) (re-find #"^[A-Z][A-Z _-]+$" (str/trim %)))))
                       (keep board-entry) (take 5))]
      (concat [(str "  " (board-counts text))]
              (if (seq entries)
                (map (fn [{:keys [id title]}]
                       (str "  " (clip title 60) " " (dim (subs id 0 (min 8 (count id)))))) entries)
                ["  no active entries"])))))
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
      (for [{:keys [id routing usage]} targets
            :let [window (first (get usage :windows))]]
        (str "  " id "  " (if (= routing "exhausted") (paint 31 routing) (if (= routing "eligible") (paint 32 routing) routing))
             "  " (if window (str (:usedPercent window) "%") "usage —") "  " (reset-age (:resetsAt window))))
      ["  collecting…"])))
(defn render []
  (let [lanes (get-in (state/read-panel :lanes) [:last-good :data :lanes])
        health (get-in (state/read-panel :health) [:last-good :data :services])
        board (get-in (state/read-panel :board) [:last-good :data :text])
        providers (get-in (state/read-panel :providers) [:last-good :data])
        lines (concat ["north dashboard" "" (header "FLEET" :lanes)] (fleet-lines lanes)
                      ["" (header "HEALTH" :health)] (health-lines health)
                      ["" (header "BOARD" :board)] (board-lines board)
                      ["" (header "ACCOUNTS" :providers)] (account-lines providers)
                      ["" (dim "working = producing output · done/failed = finished · stale = no recent signal")])]
    (str (str/join "\n" (map #(clip % (width)) (take 40 lines))) "\n")))
