(ns north.dashboard.render
  (:require [clojure.string :as str] [north.dashboard.state :as state]))

(defn width []
  (try (max 1 (Long/parseLong (or (System/getenv "COLUMNS") "100")))
       (catch Exception _ 100)))
(defn color? [] (and (not (seq (System/getenv "NO_COLOR"))) (some? (System/console))))
(defn dim [s] (if (color?) (str "\u001b[2m" s "\u001b[0m") s))
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
(defn header [name panel] (str (dim name) " " (dim (str "· " (panel-status panel)))))
(defn terminal? [status] (#{"finished" "failed"} status))
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
        (for [{:keys [title status role provider last-output-age]} shown]
          (str "  " (format "%-11s" status) (format "%-30s" (clip title 30))
               (clip (str/join "/" (remove nil? [role provider])) 22)
               " " (age last-output-age)))
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
(defn board-lines [text]
  (if-not (seq text) ["  collecting…"]
    (let [ls (str/split-lines text)
          counts (or (some #(when (re-find #"THREADS\s+—" %) %) ls) "THREADS unavailable")
          after-active (drop-while #(not (re-find #"(^ACTIVE(?:\s|$)|ACTIVE\s+—)" (str/trim %))) ls)
          entries (->> (rest after-active)
                       (take-while #(not (or (str/blank? %) (re-find #"^[A-Z][A-Z _-]+$" (str/trim %)))))
                       (remove str/blank?) (take 5))]
      (concat [(str "  " counts)] (if (seq entries) (map #(str "  " (str/trim %)) entries) ["  no active entries"])))))
(defn account-lines [document]
  (let [targets (mapcat :targets (:providers document))]
    (if (seq targets)
      (for [{:keys [id routing usage]} targets
            :let [window (first (get usage :windows))]]
        (str "  " id "  " routing "  " (if window (str (:usedPercent window) "%") "usage —")
             "  reset " (or (:resetsAt window) "—")))
      ["  collecting…"])))
(defn render []
  (let [lanes (get-in (state/read-panel :lanes) [:last-good :data :lanes])
        health (get-in (state/read-panel :health) [:last-good :data :services])
        board (get-in (state/read-panel :board) [:last-good :data :text])
        providers (get-in (state/read-panel :providers) [:last-good :data])
        lines (concat ["north dashboard" "" (header "FLEET" :lanes)] (fleet-lines lanes)
                      ["" (header "HEALTH" :health)] (health-lines health)
                      ["" (header "BOARD" :board)] (board-lines board)
                      ["" (header "ACCOUNTS" :providers)] (account-lines providers))]
    (str (str/join "\n" (map #(clip % (width)) (take 40 lines))) "\n")))
