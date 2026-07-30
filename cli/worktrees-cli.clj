#!/usr/bin/env bb
;; `north worktrees` — machine-wide worktree census. READ-ONLY.
;; Composes git, `concern`, and the coordination graph, printing every primitive
;; it runs (cockpit ownership rule). Each join is separately bounded so a slow
;; coordinator degrades one column, never the census.
(require '[cheshire.core :as json]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def here (.getParent (io/file (System/getProperty "babashka.file"))))
(load-file (str here "/coord.clj"))
(load-file (str here "/worktree-census.clj"))

(def concern-bin
  (.getPath (io/file (.getParentFile (io/file here)) "bin" "concern")))
(def port (Integer/parseInt (or (System/getenv "NORTH_PORT")
                                (System/getenv "FRAM_PORT")
                                "7977")))
(def join-timeout-ms
  (or (some-> (System/getenv "NORTH_WORKTREES_JOIN_TIMEOUT_MS") str/trim parse-long)
      6000))

(def use-color? (and (nil? (System/getenv "NO_COLOR"))
                     (nil? (System/getenv "NORTH_NO_COLOR"))))
(defn- c [code s] (if use-color? (str "\033[" code "m" s "\033[0m") (str s)))
(defn- dim [s] (c "2" s))
(defn- bold [s] (c "1" s))
(defn- red [s] (c "31" s))
(defn- ylw [s] (c "33" s))

(defn- primitive [line] (println (dim (str "  » " line))))

;; ---- joins ------------------------------------------------------------------

(defn- bounded
  "Start `f` off-thread against a wall-clock deadline. Joins are started before the
   Git census so their latency overlaps it; `settle` collects them afterwards, and a
   join that cannot answer in time reports unavailable rather than holding the
   census hostage."
  [f]
  {:started (future (try {:ok (f)}
                         (catch Throwable error
                           {:error (or (.getMessage error)
                                       (.getName (class error)))})))
   :deadline (+ (System/currentTimeMillis) join-timeout-ms)})

(defn- settle [{:keys [started deadline]}]
  (deref started
         (max 0 (- deadline (System/currentTimeMillis)))
         {:error (str "no answer within " join-timeout-ms "ms")}))

(defn live-concerns
  "Live concerns keyed by the container path their `repo` resolves to. `concern
   list-json` owns liveness decay; this only resolves its repo strings to paths."
  [containers]
  (let [index (north.worktree-census/container-index containers)]
    (bounded
     (fn []
       (let [child (proc/process {:out :string :err :string :continue true}
                                 concern-bin "list-json")
             finished (deref child join-timeout-ms nil)]
         (when-not finished
           (proc/destroy-tree child)
           (throw (ex-info "concern list-json did not answer in time" {})))
         (when-not (zero? (:exit finished))
           (throw (ex-info "concern list-json failed" {})))
         (->> (:concerns (json/parse-string (str (:out finished)) true))
              (filter :online)
              (remove :retired)
              (reduce (fn [joined concern]
                        (if-let [container (north.worktree-census/resolve-container
                                            index (:repo concern))]
                          (update joined container (fnil conj []) concern)
                          joined))
                      {})))))))

(defn lane-registrations
  "Canonical worktree path -> the graph subject that registered it."
  []
  (bounded #(north.worktree-census/claimed-worktrees (north.coord/expected-log))))

;; ---- render -----------------------------------------------------------------

(defn- branch-cell [row]
  (cond (:foreign row) "(not a worktree)"
        (:branch row) (:branch row)
        :else "(detached)"))

(defn- divergence-cell [row]
  (cond
    (:foreign row) "n/a"
    (:detached row) "detached"
    (nil? (:ahead row)) "?"
    :else (str "+" (:ahead row) "/-" (:behind row))))

(defn- dirty-cell [row]
  (cond
    (:foreign row) "n/a"
    (not (:dirty_known row)) "?"
    (:clean row) "clean"
    :else (str (:dirty_tracked row) "t/" (:dirty_untracked row) "u")))

(defn- concern-cell [row concerns]
  (let [owners (->> (get concerns (:container row))
                    (map #(or (:agent %) (:id %)))
                    (remove nil?)
                    distinct)]
    (if (seq owners) (str/join "," (take 2 owners)) "-")))

(defn- lane-cell [row registrations]
  (or (get registrations (:worktree row)) "-"))

(defn- verdict
  "`joined?` is false when a liveness join could not answer. Reapability is a
   claim about ownership, so it is never asserted from the Git half alone."
  [row concerns registrations joined?]
  (cond
    (get registrations (:worktree row)) :registered
    (seq (get concerns (:container row))) :live-concern
    (not (north.worktree-census/stale? row)) :fresh
    (not (:dirty_known row)) :review
    (not (:clean row)) :review
    (not (true? (:merged row))) :review
    joined? :reapable
    :else :unproven))

(defn- verdict-label [v]
  (case v
    :registered "lane"
    :live-concern "concern"
    :fresh "active"
    :review (ylw "REVIEW")
    :unproven (ylw "STALE?")
    :reapable (red "STALE")))

(defn render-table [rows concerns registrations joined? sources]
  (println (bold "north worktrees")
           (dim (str "— " (count rows) " worktrees across "
                     (count (distinct (map :repo rows))) " repos")))
  (println)
  (let [sorted (sort-by (juxt :repo :name) rows)
        cells (fn [row]
                [(:repo row)
                 (:name row)
                 (branch-cell row)
                 (divergence-cell row)
                 (dirty-cell row)
                 (north.worktree-census/human-age (:age_ms row))
                 (concern-cell row concerns)
                 (lane-cell row registrations)])
        headers ["REPO" "WORKTREE" "BRANCH" "AHEAD/BEH" "DIRTY" "AGE"
                 "CONCERN" "LANE"]
        widths (reduce (fn [widths row]
                         (mapv max widths (map count (cells row))))
                       (mapv count headers)
                       sorted)
        line (fn [values]
               (str "  " (str/join "  " (map (fn [width value]
                                               (format (str "%-" width "s") value))
                                             widths values))))]
    (println (bold (line headers)))
    (doseq [row sorted]
      (let [v (verdict row concerns registrations joined?)]
        (println (cond-> (line (cells row))
                   (not= :fresh v) (str "  " (verdict-label v)))))))
  (println)
  (doseq [[label state] sources]
    (when (:error state)
      (println (ylw (str "  join unavailable: " label " — " (:error state)
                         "; its column reads '-' and nothing is called reapable this run")))))
  (let [counts (frequencies (map #(verdict % concerns registrations joined?) rows))]
    (println (dim (format "  %d reapable, %d unproven, %d needing review, %d held by a lane or live concern, %d active"
                          (get counts :reapable 0)
                          (get counts :unproven 0)
                          (get counts :review 0)
                          (+ (get counts :registered 0) (get counts :live-concern 0))
                          (get counts :fresh 0)))))
  (println (dim "  STALE = merged + clean + idle >48h with no lane or live concern; the reactor sweep reaps these."))
  (println (dim "  REVIEW = idle >48h but dirty or unmerged; never auto-removed — salvage or land it.")))

(defn render-json [rows concerns registrations joined? sources]
  (println
   (json/generate-string
    {:version 1
     :generated_at (str (java.time.Instant/now))
     :roots (north.worktree-census/roots)
     :stale_age_ms north.worktree-census/stale-age-ms
     :sources (into {} (map (fn [[label state]]
                              [label (if (:error state)
                                       {:available false :error (:error state)}
                                       {:available true})]))
                    sources)
     :worktrees (mapv (fn [row]
                        (assoc row
                               :concerns (mapv :id (get concerns (:container row)))
                               :lane_subject (get registrations (:worktree row))
                               :verdict (name (verdict row concerns registrations
                                                        joined?))))
                      (sort-by (juxt :repo :name) rows))})))

;; ---- main -------------------------------------------------------------------

(defn -main [& args]
  (let [flags (set (filter #(str/starts-with? % "--") args))
        repo-filter (first (remove #(str/starts-with? % "--") args))]
    (when (flags "--help")
      (println "usage: north worktrees [repo] [--json]")
      (System/exit 0))
    (let [containers (cond->> (north.worktree-census/containers)
                       repo-filter (filter #(= repo-filter (:repo %))))]
      (when-not (flags "--json")
        (primitive (str "git -C <repo>/main worktree list --porcelain    ("
                        (count containers) " container repos under "
                        (str/join ":" (north.worktree-census/roots)) ")"))
        (primitive "git -C <worktree> status --porcelain=v1 --untracked-files=all")
        (primitive "git -C <repo>/main rev-list --left-right --count refs/heads/<main>...<head>")
        (primitive "concern list-json                                    (live-concern join)")
        (primitive (str "gawk <live worktree facts> " (north.coord/expected-log)
                        "   (lane registration join)"))
        (println))
      (let [concern-join (live-concerns containers)
            registration-join (lane-registrations)
            rows (vec (mapcat north.worktree-census/repo-rows containers))
            concern-state (settle concern-join)
            registration-state (settle registration-join)
            concerns (or (:ok concern-state) {})
            registrations (or (:ok registration-state) {})
            joined? (and (contains? concern-state :ok)
                         (contains? registration-state :ok))
            sources {"concern list-json" concern-state
                     "coordinator lane registrations" registration-state}]
        (if (flags "--json")
          (render-json rows concerns registrations joined? sources)
          (render-table rows concerns registrations joined? sources))))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
