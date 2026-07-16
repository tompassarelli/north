#!/usr/bin/env bb
;; Evidence-aware routing feedback. Operational completion is reported separately
;; from thread verification; evidence coverage is never presented as model quality.

(require '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def multi-preds #{"done_when" "bar_evidence" "domain_requirement"})

(defn default-paths []
  (let [home (System/getenv "HOME")
        dir (str home "/.local/state/north")
        split (io/file dir "coordination.log")]
    [(or (System/getenv "FRAM_LOG")
         (if (.exists split) (.getPath split) (str dir "/facts.log")))
     (or (System/getenv "FRAM_TELEMETRY_LOG")
         (let [path (str dir "/telemetry.log")] (when (.exists (io/file path)) path)))]))

(defn read-ops [paths]
  (mapcat (fn [path]
            (if (and path (.exists (io/file path)))
              (with-open [reader (io/reader path)]
                (doall
                 (keep (fn [line]
                         (try (edn/read-string line) (catch Exception _ nil)))
                       (line-seq reader))))
              []))
          (distinct (remove nil? paths))))

(defn fold-facts [ops]
  (reduce
   (fn [facts {:keys [op l p r]}]
     (if-not (and l p) facts
       (let [current (get-in facts [l p] [])]
         (cond
           (= op "assert")
           (assoc-in facts [l p]
                     (if (multi-preds p) (if (some #{r} current) current (conj current r)) [r]))
           (= op "retract")
           (let [remaining (vec (remove #{r} current))]
             (if (seq remaining) (assoc-in facts [l p] remaining) (update facts l dissoc p)))
           :else facts))))
   {} ops))

(defn one [facts entity pred] (last (get-in facts [entity pred])))
(defn many [facts entity pred] (get-in facts [entity pred] []))
(defn long' [value] (try (parse-long (str value)) (catch Exception _ 0)))
(defn thread-ref [value]
  (when (and value (not= value "(ad-hoc)"))
    (if (str/starts-with? value "@") value (str "@" value))))

(defn evidence [facts thread]
  (if-not thread
    {:status "no-contract" :bars 0 :evidenced 0 :hasOutcome false}
    (let [bars (many facts thread "done_when")
          evs (many facts thread "bar_evidence")
          outcome? (boolean (one facts thread "outcome"))
          evidenced (count (filter (fn [bar] (some #(str/includes? (str %) (str bar)) evs)) bars))
          total (count bars)
          status (cond
                   (zero? total) "no-contract"
                   (and outcome? (= evidenced total)) "verified"
                   (= evidenced total) "evidenced-open"
                   (pos? evidenced) "partial"
                   :else "unevidenced")]
      {:status status :bars total :evidenced evidenced :hasOutcome outcome?})))

(defn run-rows [facts]
  (for [[entity predicates] facts
        :when (and (= "run" (one facts entity "kind"))
                   (str/starts-with? entity "@run-"))]
    (let [agent (one facts entity "agent")
          identity (str "@agent:" agent)
          get' (fn [pred fallback] (or (one facts entity pred) (one facts identity pred) fallback))
          thread (thread-ref (one facts entity "thread"))]
      {:entity entity :thread thread
       :provider (get' "provider" "?") :tier (get' "requested_tier" "?")
       :model (get' "model" "?") :effort (get' "effort" "?")
       :role (get' "role" "?") :taskGrade (get' "task_grade" "?")
       :outcome (get' "outcome" "?") :tokens (long' (get' "tokens" 0))
       :durationMs (long' (get' "duration_ms" 0)) :turns (long' (get' "num_turns" 0))
       :fallbacks (long' (get' "fallback_count" 0))
       :escalations (long' (get' "escalation_count" 0))
       :compositionKind (get' "composition_kind" nil)
       :compositionId (get' "composition_id" nil)
       :nearestPreset (get' "nearest_preset" nil)
       :bespokeReason (get' "bespoke_reason" nil)
       :promotionCandidate (= "true" (get' "promotion_candidate" "false"))
       :evidence (evidence facts thread)})))

(def cohort-fields [:provider :tier :role :taskGrade])
(defn cohort-label [row] (str/join "/" (map #(get row %) cohort-fields)))

(defn performance-row [[label rows]]
  (let [statuses (frequencies (map #(get-in % [:evidence :status]) rows))]
    {:cohort label :runs (count rows) :operationalRan (count (filter #(= "ran" (:outcome %)) rows))
     :threadOutcomes (count (filter #(get-in % [:evidence :hasOutcome]) rows))
     :verifiedEvidence (get statuses "verified" 0) :partialEvidence (get statuses "partial" 0)
     :unevidenced (get statuses "unevidenced" 0) :noContract (get statuses "no-contract" 0)
     :evidencedOpen (get statuses "evidenced-open" 0)
     :escalated (count (filter #(pos? (:escalations %)) rows))}))

(defn performance-report [rows]
  {:report "performance" :claim "operational outcomes plus current thread evidence coverage; not causal model quality"
   :runs (count rows)
   :cohorts (->> rows (group-by cohort-label) (map performance-row) (sort-by :cohort) vec)})

(defn usage-row [[provider rows]]
  {:provider provider :runs (count rows) :tokens (reduce + (map :tokens rows))
   :wallSeconds (long (/ (reduce + (map :durationMs rows)) 1000))
   :turns (reduce + (map :turns rows)) :fallbacks (reduce + (map :fallbacks rows))
   :escalatedRuns (count (filter #(pos? (:escalations %)) rows))})

(defn usage-report [rows]
  {:report "usage" :unit "observed work, never dollars or API credits"
   :runs (count rows) :providers (->> rows (group-by :provider) (map usage-row) (sort-by :provider) vec)})

(defn promotion-row [[composition rows]]
  (let [threads (set (keep :thread rows))
        verified (count (filter #(= "verified" (get-in % [:evidence :status])) rows))
        flagged (some :promotionCandidate rows)
        recurrent (>= (count threads) 2)
        review-status (cond
                        (not flagged) "not-requested"
                        (not recurrent) "insufficient-recurrence"
                        (zero? verified) "needs-verification-evidence"
                        :else "review-candidate")]
    {:compositionId composition :runs (count rows) :distinctThreads (count threads)
     :nearestPresets (vec (sort (set (keep :nearestPreset rows))))
     :reasons (vec (sort (set (keep :bespokeReason rows))))
     :operationalRan (count (filter #(= "ran" (:outcome %)) rows))
     :verifiedEvidence verified :promotionRequested (boolean flagged)
     :reviewStatus review-status
     :note "recurrence is evidence for human review; this report never promotes a role"}))

(defn promotions-report [rows]
  (let [bespoke (filter #(= "bespoke" (:compositionKind %)) rows)]
    {:report "promotions" :claim "observed bespoke recurrence; never automatic promotion"
     :compositions (->> bespoke (group-by :compositionId) (map promotion-row)
                        (sort-by (juxt (comp - :distinctThreads) :compositionId)) vec)}))

(defn report [kind rows]
  (case kind
    "performance" (performance-report rows)
    "usage" (usage-report rows)
    "promotions" (promotions-report rows)
    (throw (ex-info "usage: north routing report [performance|usage|promotions] [--json]" {}))))

(defn print-table [data]
  (case (:report data)
    "performance"
    (do (println "ROUTING PERFORMANCE — operational outcomes + verification coverage")
        (println "Evidence is joined from current thread done_when/bar_evidence/outcome facts; it is not causal model quality.")
        (println (format "%-38s %5s %5s %5s %5s %5s %5s" "COHORT provider/tier/role/grade" "runs" "ran" "ver" "part" "none" "esc"))
        (doseq [row (:cohorts data)]
          (println (format "%-38s %5d %5d %5d %5d %5d %5d" (:cohort row) (:runs row)
                           (:operationalRan row) (:verifiedEvidence row) (:partialEvidence row)
                           (+ (:unevidenced row) (:noContract row)) (:escalated row)))))
    "usage"
    (do (println "ROUTING USAGE — observed work (never dollars or API credits)")
        (println (format "%-12s %6s %12s %10s %8s %9s %9s" "PROVIDER" "runs" "tokens" "wall-s" "turns" "fallbacks" "escalated"))
        (doseq [row (:providers data)]
          (println (format "%-12s %6d %12d %10d %8d %9d %9d" (:provider row) (:runs row)
                           (:tokens row) (:wallSeconds row) (:turns row) (:fallbacks row) (:escalatedRuns row)))))
    "promotions"
    (do (println "ROUTING PROMOTIONS — bespoke recurrence for human review")
        (println "This report never changes Gaffer's standard library.")
        (if (empty? (:compositions data)) (println "  (no bespoke compositions observed)")
          (doseq [row (:compositions data)]
            (println (format "%-28s threads=%d runs=%d verified=%d  %s" (:compositionId row)
                             (:distinctThreads row) (:runs row) (:verifiedEvidence row) (:reviewStatus row))))))))

(defn -main [& args]
  (let [[verb kind & flags] args]
    (when-not (= verb "report")
      (binding [*out* *err*] (println "usage: north routing report [performance|usage|promotions] [--json]"))
      (System/exit 2))
    (let [facts (fold-facts (read-ops (default-paths)))
          data (report (or kind "performance") (run-rows facts))]
      (if (some #{"--json"} flags)
        (println (json/generate-string data))
        (print-table data)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
