;; north-reconcile.clj — telemetry reconciliation: reads kind=run facts and surfaces
;; exact usage, estimate-vs-actual drift, and model/provider patterns. This is the
;; consumer that closes the feedback loop — without it, run telemetry is inert.
;;
;; usage:
;;   bb north-reconcile.clj <port>                    — full report
;;   bb north-reconcile.clj <port> by-model            — breakdown by model tier
;;   bb north-reconcile.clj <port> drift               — estimate vs actual, sorted by overshoot
;;   bb north-reconcile.clj <port> recent [N]           — last N runs (default 20)
;;   bb north-reconcile.clj <port> agent <uuid>         — runs for one agent
(require '[clojure.java.io :as io])

;; shared coord substrate (Foundation Part B): send-op lives once in cli/coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(def send-op north.coord/send-op)

(defn all-runs [port]
  (->> (:ok (send-op port {:op :query
                           :query {:find "r"
                                   :rules [{:head {:rel "r" :args [{:var "e"}]}
                                            :body [{:rel "triple" :args [{:var "e"} "kind" "run"]}]}]}}))
       (map first)
       sort))

(defn run-meta [port re]
  (let [preds ["agent" "tokens" "input_tokens" "output_tokens" "cache_read_tokens"
               "cache_create_tokens" "cached_input_tokens" "reasoning_output_tokens"
               "usage_terminal_count" "usage_scope" "usage_total_status"
               "duration_ms" "num_turns" "stop_reason" "model"
               "provider" "effort" "caveman" "wall_s" "estimate_output_tokens"
               "confidence" "fallback_count" "fallback_path" "outcome" "ended_at" "at"]]
    (reduce (fn [m p]
              (let [v (:value (send-op port {:op :resolved :te re :p p}))]
                (if v (assoc m (keyword p) v) m)))
            {:entity re} preds)))

(defn parse-num [s] (when s (try (parse-double s) (catch Exception _ nil))))

(defn drift-ratio [est act]
  (when (and est act (pos? est))
    (/ (double act) (double est))))

(defn fmt-drift [ratio]
  (when ratio
    (let [pct (int (* 100 (- ratio 1)))]
      (cond (> pct 50)  (format "+%d%% !!!" pct)
            (> pct 20)  (format "+%d%% !" pct)
            (> pct 0)   (format "+%d%%" pct)
            (< pct -20) (format "%d%% (under)" pct)
            :else       (format "%d%%" pct)))))

(defn print-summary [runs]
  (let [tokens (keep #(parse-num (:tokens %)) runs)
        durations (keep #(parse-num (:duration_ms %)) runs)
        turns (keep #(parse-num (:num_turns %)) runs)
        fallbacks (keep #(parse-num (:fallback_count %)) runs)
        drifts (keep (fn [r]
                       (let [est (parse-num (:estimate_output_tokens r))
                             act (parse-num (:output_tokens r))]
                         (drift-ratio est act)))
                     runs)
        confs (keep #(parse-num (:confidence %)) runs)]
    (println (format "%-20s %d" "total runs" (count runs)))
    (when (seq tokens)
      (println (format "%-20s %d (%d/%d runs reported)" "total tokens"
                       (long (reduce + tokens)) (count tokens) (count runs)))
      (println (format "%-20s %d" "avg tokens/run"
                       (long (/ (reduce + tokens) (count tokens))))))
    (when (seq durations)
      (println (format "%-20s %d" "total duration ms" (long (reduce + durations)))))
    (when (seq turns)
      (println (format "%-20s %d" "total turns" (long (reduce + turns)))))
    (when (seq fallbacks)
      (println (format "%-20s %d" "provider fallbacks" (long (reduce + fallbacks)))))
    (when (seq drifts)
      (let [avg-drift (/ (reduce + drifts) (count drifts))]
        (println (format "%-20s %.1fx (1.0 = perfect)" "avg estimate drift" avg-drift))
        (println (format "%-20s %.1fx" "worst overshoot" (apply max drifts)))))
    (when (seq confs)
      (println (format "%-20s %.1f / 5" "avg confidence" (/ (reduce + confs) (count confs)))))))

(defn print-by-model [runs]
  (let [groups (group-by #(or (:model %) "unknown") runs)]
    (println (format "%-16s %5s %12s %12s %8s %9s %10s"
                     "MODEL" "RUNS" "TOKENS" "DURATION_MS" "TURNS" "FALLBACKS" "AVG_DRIFT"))
    (doseq [[model rs] (sort groups)]
      (let [tokens (keep #(parse-num (:tokens %)) rs)
            durations (keep #(parse-num (:duration_ms %)) rs)
            turns (keep #(parse-num (:num_turns %)) rs)
            fallbacks (keep #(parse-num (:fallback_count %)) rs)
            drifts (keep (fn [r]
                           (drift-ratio (parse-num (:estimate_output_tokens r))
                                        (parse-num (:output_tokens r))))
                         rs)]
        (println (format "%-16s %5d %12s %12d %8d %9d %10s"
                         model (count rs)
                         (if (seq tokens) (str (long (reduce + tokens))) "?")
                         (long (reduce + 0 durations))
                         (long (reduce + 0 turns))
                         (long (reduce + 0 fallbacks))
                         (if (seq drifts) (format "%.1fx" (/ (reduce + drifts) (count drifts))) "-")))))))

(defn print-drift [runs]
  (let [with-drift (->> runs
                        (keep (fn [r]
                                (let [est (parse-num (:estimate_output_tokens r))
                                      act (parse-num (:output_tokens r))
                                      d (drift-ratio est act)]
                                  (when d (assoc r ::drift d)))))
                        (sort-by ::drift >))]
    (println (format "%-36s %6s %6s %8s %10s %s" "RUN" "EST" "ACTUAL" "DRIFT" "TOKENS" "MODEL"))
    (doseq [r with-drift]
      (println (format "%-36s %6s %6s %8s %10s %s"
                       (subs (str (:entity r)) 0 (min 36 (count (str (:entity r)))))
                       (or (:estimate_output_tokens r) "?")
                       (or (:output_tokens r) "?")
                       (or (fmt-drift (::drift r)) "?")
                       (or (:tokens r) "?")
                       (or (:model r) "?"))))))

(defn print-recent [runs n]
  (let [recent (take-last n (sort-by #(or (:at %) (:ended_at %) "") runs))]
    (println (format "%-36s %10s %12s %6s %-28s %s"
                     "RUN" "TOKENS" "DURATION_MS" "TURNS" "FALLBACKS/PATH" "PROVIDER/MODEL/EFFORT"))
    (doseq [r recent]
      (println (format "%-36s %10s %12s %6s %-28s %s"
                       (subs (str (:entity r)) 0 (min 36 (count (str (:entity r)))))
                       (or (:tokens r) "?")
                       (or (:duration_ms r) "?")
                       (or (:num_turns r) "?")
                       (str (or (:fallback_count r) "0") ":" (or (:fallback_path r) "-"))
                       (str (or (:provider r) "?") "/" (or (:model r) "?") "/"
                            (or (:effort r) "?")))))))

(let [[port-s verb & args] *command-line-args*
      port (Integer/parseInt port-s)
      entities (all-runs port)
      runs (mapv #(run-meta port %) entities)]
  (case (or verb "full")
    "full"
    (do (println "=== AGENT USAGE RECONCILIATION ===\n")
        (print-summary runs)
        (println) (print-by-model runs)
        (println "\n--- recent (last 10) ---")
        (print-recent runs 10))

    "by-model" (print-by-model runs)

    "drift" (print-drift runs)

    "recent"
    (let [n (if (seq args) (Integer/parseInt (first args)) 20)]
      (print-recent runs n))

    "agent"
    (let [[uuid] args
          mine (filter #(= (:agent %) uuid) runs)]
      (if (seq mine)
        (do (println (str "Runs for agent " uuid ":"))
            (print-summary mine)
            (println)
            (print-recent mine 50))
        (println (str "No runs found for " uuid))))

    (do (println "usage: north-reconcile.clj <port> [full|by-model|drift|recent [N]|agent <uuid>]")
        (System/exit 2))))
