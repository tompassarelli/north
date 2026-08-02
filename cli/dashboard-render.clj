(ns north.dashboard.render
  (:require [clojure.string :as str] [north.dashboard.state :as state]))
(defn line [panel] (let [e (state/read-panel panel)] (str (name panel) "  " (state/evidence e) "  age " (state/age e))))
(defn render []
  (let [lanes (get-in (state/read-panel :lanes) [:last-good :data :lanes]) health (get-in (state/read-panel :health) [:last-good :data :services])]
    (str "north dashboard\n\nFLEET  " (line :lanes) "\n"
         (if (seq lanes) (str/join "\n" (for [{:keys [id title status last-output-age]} lanes] (format "  %-28s %-12s %s  output %ss" title status (subs id 0 (min 12 (count id))) (quot last-output-age 1000)))) "  collecting…")
         "\n\nHEALTH  " (line :health) "\n"
         (if (seq health) (str/join "\n" (for [[unit {:keys [active socket memory]}] health] (str "  " unit " process " (if active "up" "down") " socket " (if socket "up" "down") " memory " (pr-str memory)))) "  collecting…")
         "\n\nBOARD  " (line :board) "\n  " (or (get-in (state/read-panel :board) [:last-good :data :text]) "collecting…")
         "\nACCOUNTS  " (line :providers) "\n  " (or (some-> (get-in (state/read-panel :providers) [:last-good :data]) pr-str) "collecting…") "\n")))
