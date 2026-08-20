;; north-reconcile.clj — telemetry reconciliation: reads kind=run facts and surfaces
;; exact usage, partial lower bounds, estimate drift, and tier/provider patterns. This is the
;; consumer that closes the feedback loop — without it, run telemetry is inert.
;;
;; usage:
;;   bb north-reconcile.clj <port>                    — full report
;;   bb north-reconcile.clj <port> by-model            — breakdown by model tier
;;   bb north-reconcile.clj <port> drift               — estimate vs actual, sorted by overshoot
;;   bb north-reconcile.clj <port> recent [N]           — last N runs (default 20)
;;   bb north-reconcile.clj <port> agent <uuid>         — runs for one agent
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

;; Shared Store RPC coordination facade.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(defn all-runs [port]
  (->> (north.coord/query-rows
        port
        {:find "r"
         :rules [{:head {:rel "r" :args [{:var "e"}]}
                  :body [{:rel "triple" :args [{:var "e"} "kind" "run"]}]}]})
       (map first)
       sort))

(def run-predicates
  ["agent" "tokens" "lifetime_input_tokens" "lifetime_output_tokens"
   "lifetime_cache_read_tokens" "lifetime_cache_write_tokens"
   "lifetime_reasoning_tokens" "model_call_count"
   "usage_terminal_count" "usage_scope" "usage_total_status"
   "duration_ms" "provider_duration_ms" "num_turns" "provider_turn_units"
   "provider_tool_items" "provider_turn_metric_comparable"
   "provider" "model_tier" "effort"
   "estimate_hours" "estimate_delta_ms" "estimate_ratio" "estimate_classification"
   "fallback_count" "fallback_path" "outcome" "at"])

(defn run-meta [port re]
  (let [wanted (set run-predicates)
        values
        (reduce (fn [out [predicate value]]
                  (if (contains? wanted predicate)
                    (update out predicate (fnil conj []) value)
                    out))
                {}
                (:rows (north.coord/show-envelope port re)))]
    (reduce
     (fn [metadata predicate]
       (let [found (get values predicate [])]
         (case (count found)
           0 metadata
           1 (assoc metadata (keyword predicate) (first found))
           (throw (ex-info "run metadata predicate is ambiguous"
                           {:type :ambiguous-run-metadata
                            :entity re
                            :predicate predicate
                            :values found})))))
     {:entity re}
     run-predicates)))

(defn parse-num [s] (when s (try (parse-double s) (catch Exception _ nil))))
(defn parse-count [s] (when s (try (bigint s) (catch Exception _ nil))))

(defn usage-status [run]
  ;; Historical rows with an already exact aggregate remain readable. No old
  ;; component or model aliases are accepted.
  (or (:usage_total_status run) (when (:tokens run) "exact")))

(defn exact-token-count [run]
  (when (= "exact" (usage-status run))
    (parse-count (:tokens run))))

(defn partial-token-lower-bound [run]
  (when (= "partial" (usage-status run))
    (let [input (parse-count (:lifetime_input_tokens run))
          output (parse-count (:lifetime_output_tokens run))
          cache-read (parse-count (:lifetime_cache_read_tokens run))
          cache-write (parse-count (:lifetime_cache_write_tokens run))]
      (when (every? some? [input output cache-read cache-write])
        (case (:provider run)
          "anthropic" (+ input output cache-read cache-write)
          "openai" (+ input output)
          nil)))))

(defn usage-cell [run]
  (case (usage-status run)
    "exact" (or (:tokens run) "invalid-exact")
    "partial" (if-let [lower (partial-token-lower-bound run)]
                (str ">=" lower " partial")
                "partial")
    "unknown_incomplete_terminal" "unknown-terminal"
    "unknown_no_terminal" "no-terminal"
    "unknown"))

(defn fmt-drift [ratio]
  (when ratio
    (let [pct (int (* 100 (- ratio 1)))]
      (cond (> pct 50)  (format "+%d%% !!!" pct)
            (> pct 20)  (format "+%d%% !" pct)
            (> pct 0)   (format "+%d%%" pct)
            (< pct -20) (format "%d%% (under)" pct)
            :else       (format "%d%%" pct)))))

(defn print-summary [runs]
  (let [tokens (keep exact-token-count runs)
        partials (keep partial-token-lower-bound runs)
        incomplete-terminals (count (filter #(= "unknown_incomplete_terminal"
                                                (usage-status %)) runs))
        no-terminals (count (filter #(= "unknown_no_terminal" (usage-status %)) runs))
        durations (keep #(parse-num (:duration_ms %)) runs)
        turns (keep #(parse-num (:num_turns %)) runs)
        fallbacks (keep #(parse-num (:fallback_count %)) runs)
        drifts (keep #(parse-num (:estimate_ratio %)) runs)]
    (println (format "%-20s %d" "total runs" (count runs)))
    (when (seq tokens)
      (println (format "%-20s %s (%d/%d exact runs)" "exact token subtotal"
                       (str (reduce + tokens)) (count tokens) (count runs)))
      (println (format "%-20s %s" "avg tokens/exact"
                       (str (quot (reduce + tokens) (count tokens))))))
    (when (seq partials)
      (println (format "%-20s >=%s (%d partial runs with known formula)"
                       "partial lower bound" (str (reduce + partials)) (count partials))))
    (when (pos? incomplete-terminals)
      (println (format "%-20s %d" "unknown terminals" incomplete-terminals)))
    (when (pos? no-terminals)
      (println (format "%-20s %d" "no usage terminal" no-terminals)))
    (when (seq durations)
      (println (format "%-20s %d" "total duration ms" (long (reduce + durations)))))
    (when (seq turns)
      ;; Opaque provider-turn units use separate predicates and never enter the
      ;; assistant-turn sum.
      (println (format "%-20s %d (num_turns-reporting providers only)" "total turns" (long (reduce + turns)))))
    (when (seq fallbacks)
      (println (format "%-20s %d" "provider fallbacks" (long (reduce + fallbacks)))))
    (when (seq drifts)
      (let [avg-drift (/ (reduce + drifts) (count drifts))]
        (println (format "%-20s %.1fx (1.0 = perfect)" "avg estimate drift" avg-drift))
        (println (format "%-20s %.1fx" "worst overshoot" (apply max drifts)))))))

(defn usage-group-cell [runs]
  (let [exact (keep exact-token-count runs)
        partial (keep partial-token-lower-bound runs)
        unknown (count (filter #(#{"unknown_incomplete_terminal" "unknown_no_terminal"}
                                  (usage-status %)) runs))
        parts (cond-> []
                (seq exact) (conj (str (reduce + exact) " exact"))
                (seq partial) (conj (str ">=" (reduce + partial) " partial"))
                (pos? unknown) (conj (str unknown " unknown")))]
    (if (seq parts) (str/join "+" parts) "?")))

(defn print-by-model [runs]
  ;; TURNS here is num_turns only. Opaque provider-turn units are a separate,
  ;; non-comparable measurement and never enter this aggregate.
  (let [groups (group-by #(or (:model_tier %) "unknown") runs)]
    (println (format "%-16s %5s %26s %12s %8s %9s %10s"
                     "MODEL_TIER" "RUNS" "USAGE" "DURATION_MS" "TURNS*" "FALLBACKS" "AVG_DRIFT"))
    (doseq [[model-tier rs] (sort groups)]
      (let [durations (keep #(parse-num (:duration_ms %)) rs)
            turns (keep #(parse-num (:num_turns %)) rs)
            fallbacks (keep #(parse-num (:fallback_count %)) rs)
            drifts (keep #(parse-num (:estimate_ratio %)) rs)]
        (println (format "%-16s %5d %26s %12d %8d %9d %10s"
                         model-tier (count rs)
                         (usage-group-cell rs)
                         (long (reduce + 0 durations))
                         (long (reduce + 0 turns))
                         (long (reduce + 0 fallbacks))
                         (if (seq drifts) (format "%.1fx" (/ (reduce + drifts) (count drifts))) "-")))))))

(defn print-drift [runs]
  (let [with-drift (->> runs
                        (keep (fn [r]
                                (let [d (parse-num (:estimate_ratio r))]
                                  (when d (assoc r ::drift d)))))
                        (sort-by ::drift >))]
    (println (format "%-36s %10s %10s %8s %18s %s"
                     "RUN" "EST_HOURS" "ACTUAL_MS" "DRIFT" "USAGE" "MODEL_TIER"))
    (doseq [r with-drift]
      (println (format "%-36s %10s %10s %8s %18s %s"
                       (subs (str (:entity r)) 0 (min 36 (count (str (:entity r)))))
                       (or (:estimate_hours r) "?")
                       (or (:duration_ms r) "?")
                       (or (fmt-drift (::drift r)) "?")
                       (usage-cell r)
                       (or (:model_tier r) "?"))))))

(defn turns-cell [r]
  ;; The "pt" suffix keeps opaque provider-turn units visibly distinct from
  ;; assistant-turn counts in the same narrow display column.
  (cond
    (:num_turns r) (str (:num_turns r))
    (and (:provider_turn_units r)
         (= "false" (:provider_turn_metric_comparable r)))
    (str (:provider_turn_units r) "pt"
         (when (:provider_tool_items r) (str "/" (:provider_tool_items r) "it")))
    :else "?"))

(defn print-recent [runs n]
  (let [recent (take-last n (sort-by #(or (:at %) "") runs))]
    (println (format "%-36s %18s %12s %9s %-28s %s"
                     "RUN" "USAGE" "DURATION_MS" "TURNS*" "FALLBACKS/PATH" "PROVIDER/TIER/EFFORT"))
    (doseq [r recent]
      (println (format "%-36s %18s %12s %9s %-28s %s"
                       (subs (str (:entity r)) 0 (min 36 (count (str (:entity r)))))
                       (usage-cell r)
                       (or (:duration_ms r) "?")
                       (turns-cell r)
                       (str (or (:fallback_count r) "0") ":" (or (:fallback_path r) "-"))
                       (str (or (:provider r) "?") "/" (or (:model_tier r) "?") "/"
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
