#!/usr/bin/env bb
;; `north report` is deliberately a coordinator projection: it enumerates a
;; bounded set of run subjects, then reads just those subjects and their threads.
(ns north.shipped-report
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json])
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
  (let [response (north.coord/indexed-query-in-domain
                  port :telemetry
                  {:find "shipped_run" :rules [{:head {:rel "shipped_run" :args [{:var "r"}]}
                                                 :body [{:rel "triple" :args [{:var "r"} "kind" "run"]}]}]}
                  max-runs)]
    (mapv first (:ok response))))

(defn exact-facts [port subject]
  (rows->facts (:rows (north.coord/show-envelope port subject))))

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

(defn lane-wall-ms [thread]
  ;; The terminal receipt is authoritative; lane metadata only fills its absent
  ;; wall-time field from the harness's start and completed-log timestamps.
  (let [dir (io/file (str (System/getProperty "user.home") "/.local/state/north/agents"))
        bare-thread (str/replace (str thread) #"^@" "")]
    (some (fn [meta]
            (try
              (let [row (json/parse-string (slurp meta) true)
                    started (instant (:startedAt row))
                    id (some-> (.getName ^java.io.File meta)
                                (str/replace #"^lane-" "")
                                (str/replace #"\.meta\.json$" ""))
                    exit (io/file dir (str "lane-" id ".log.lane.exit"))]
                (when (and (= bare-thread (:thread row)) started (.isFile exit))
                  (str (max 0 (- (.lastModified exit) (.toEpochMilli started))))))
              (catch Exception _ nil)))
          (take max-runs (or (.listFiles dir) [])))))

(defn report-rows [port since now]
  (let [subjects (run-subjects port)
        runs (mapv (fn [subject] [subject (exact-facts port subject)]) subjects)
        in-window (filter (fn [[_ facts]] (let [at (instant (one facts "at"))]
                                             (and at (not (.isBefore at since))))) runs)
        thread-cache (atom {})]
    (->> in-window
         (keep (fn [[subject facts]]
                 (when (= "ran" (one facts "process_outcome"))
                   (let [thread (one facts "thread")
                         thread-facts (when thread
                                        (or (get @thread-cache thread)
                                            (let [value (exact-facts port thread)]
                                              (swap! thread-cache assoc thread value) value)))]
                     {:run subject :harness (harness facts) :at (one facts "at")
                      :duration-ms (or (one facts "duration_ms") (lane-wall-ms thread))
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
