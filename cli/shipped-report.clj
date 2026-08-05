#!/usr/bin/env bb
;; `north report` is a bounded Fram projection: it enumerates run subjects, then
;; reads the exact run and thread subjects in two batched operations.
(ns north.shipped-report
  (:require [clojure.string :as str])
  (:import [java.time Duration Instant]))

(def root (or (System/getenv "NORTH_HOME") (System/getProperty "user.dir")))
(load-file (str root "/cli/coord.clj"))

(def ^:dynamic *now* #(Instant/now))
(def max-runs 512)

(defn usage [] "usage: north report [--since 24h|7d] [--fresh]")
(defn parse-window [args]
  (let [args (vec (remove #{"--fresh"} args))]
    (cond
      (empty? args) (Duration/ofHours 24)
      (= ["--since" "24h"] args) (Duration/ofHours 24)
      (= ["--since" "7d"] args) (Duration/ofDays 7)
      :else (throw (ex-info (usage) {:type :usage})))))

(defn instant [value] (try (Instant/parse (str value)) (catch Exception _ nil)))
(defn one [facts predicate] (let [values (get facts predicate)]
                              (when (= 1 (count values)) (first values))))
(defn rows->facts [rows] (reduce (fn [m [p v]] (update m p (fnil conj #{}) v)) {} rows))

(defn run-subjects [port]
  (let [response (north.coord/bounded-query-in-domain
                  port :telemetry
                  {:find "shipped_run" :rules [{:head {:rel "shipped_run" :args [{:var "r"}]}
                                                 :body [{:rel "triple" :args [{:var "r"} "kind" "run"]}]}]}
                  max-runs)]
    (mapv first (:rows response))))

(defn exact-facts-many [port domain subjects]
  (if (seq subjects)
    (let [response (north.coord/show-many-in-domain port domain subjects)]
      (into {}
            (map (fn [[subject rows]] [subject (rows->facts rows)]))
            (:rows response)))
    {}))

(defn harness [facts]
  (let [provider (one facts "provider")
        composition (or (one facts "composition") (one facts "composition_id")
                        (one facts "composition_kind"))]
    (cond
      (= provider "anthropic") "native-claude"
      (and (= provider "openai") composition) "managed-codex"
      (= provider "openai") "native-codex"
      :else "unknown-harness")))

(defn commit-refs [facts]
  (->> ["outcome" "progress"]
       (mapcat #(get facts %))
       (mapcat #(re-seq #"(?i)\b[0-9a-f]{7,40}\b" %))
       distinct sort vec))

(defn report-rows [port since now]
  (let [subjects (run-subjects port)
        runs-by-subject (exact-facts-many port :telemetry subjects)
        runs (mapv (fn [subject] [subject (get runs-by-subject subject {})]) subjects)
        in-window (filter (fn [[_ facts]] (let [at (instant (one facts "at"))]
                                             (and at (not (.isBefore at since))))) runs)
        threads (->> in-window (keep (fn [[_ facts]] (one facts "thread"))) distinct vec)
        threads-by-subject (exact-facts-many port :coordination threads)]
    (->> in-window
         (keep (fn [[subject facts]]
                 (when (= "ran" (one facts "process_outcome"))
                   (let [thread (one facts "thread")
                         thread-facts (get threads-by-subject thread {})]
                     {:run subject :harness (harness facts) :at (one facts "at")
                      :duration-ms (one facts "duration_ms")
                      :delivery (or (one facts "delivery_outcome") "unverified")
                      :title (or (one thread-facts "title") thread "(no thread)")
                      :outcome (one thread-facts "outcome")
                      :commits (commit-refs thread-facts)}))))
         (sort-by :at #(compare %2 %1)) vec)))

(defn duration-label [ms]
  (if-let [n (try (parse-long (str ms)) (catch Exception _ nil))]
    (str (max 1 (long (Math/round (/ n 60000.0)))) "m") "wall time unavailable"))

(defn render [rows since now]
  (let [line (fn [{:keys [title delivery outcome commits duration-ms]}]
               (str "  " (if (= delivery "verified") "shipped" "unverified")
                    " · " title " · " (duration-label duration-ms)
                    (when outcome (str " · outcome: " outcome))
                    (when (seq commits) (str " · commits: " (str/join ", " commits)))))
        profile-lines (fn [[profile group]]
                        (into [(str "\n" profile)] (map line group)))
        body (if (empty? rows)
               ["  no completed runs in window"]
               (mapcat profile-lines (sort-by first (group-by :harness rows))))]
    (str/join "\n" (concat [(str "SHIPPED — since " since " (data as of " now ")")]
                            body [""]))))

(defn -main [& args]
  (try
    (let [window (parse-window args) now (*now*) since (.minus now window)
          port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977"))]
      (print (render (report-rows port since now) since now)))
    (catch clojure.lang.ExceptionInfo error
      (binding [*out* *err*] (println (.getMessage error)))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file")) (apply -main *command-line-args*))
