#!/usr/bin/env bb
;; Last-good rendered list snapshots.  The dashboard owns the same :board
;; envelope, so a dashboard collection is immediately useful to `north threads`.
(ns north.fast-list-cli)
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (or (System/getenv "NORTH_HOME") (System/getProperty "user.dir")))
(load-file (str root "/cli/dashboard-state.clj"))

(defn panel-for [verb args]
  (keyword (str verb (when (some #{"--all"} args) "-all")
                (when-let [since (second (drop-while #(not= % "--since") args))]
                  (str "-" since)))))

(defn dim [s]
  (if (or (= "1" (System/getenv "NO_COLOR")) (= "true" (System/getenv "NO_COLOR"))) s
      (str "\u001b[2m" s "\u001b[0m")))

(defn render! [verb args]
  (let [command (into [(str root "/bin/north") verb] args)
        process (p/process command {:out :string :err :string
                                    :extra-env {"NORTH_FAST_LIST_RENDER" "1"}})
        result @process]
    (if (zero? (:exit result))
      (:out result)
      (throw (ex-info (str/trim (:err result)) {:exit (:exit result)})))))

(defn launch! [verb args]
  ;; flock owns the lock for the detached renderer's lifetime, so a crashed
  ;; renderer cannot leave a stale single-flight marker behind.
  (let [lock (str (north.dashboard.state/cache-dir) "/" verb ".lock")]
    (.mkdirs (io/file (north.dashboard.state/cache-dir)))
    (doto (ProcessBuilder. ^java.util.List
            (java.util.ArrayList. (into ["flock" "-n" lock (str root "/bin/north") verb "--fresh"] args)))
      (.redirectOutput java.lang.ProcessBuilder$Redirect/DISCARD)
      (.redirectError java.lang.ProcessBuilder$Redirect/DISCARD)
      (.start))))

(defn fresh! [panel verb args]
  (try
    (let [text (render! verb args)]
      (north.dashboard.state/record! panel {:status :ok :data {:text text}})
      (print text))
    (catch Exception error
      (north.dashboard.state/record! panel {:status :error :detail (.getMessage error)})
      (binding [*out* *err*] (println (str "north: " (.getMessage error))))
      (System/exit 1))))

(defn -main [& argv]
  (let [verb (first argv)
        fresh? (some #{"--fresh"} argv)
        args (vec (remove #{"--fresh"} (rest argv)))
        panel (panel-for verb args)
        cached (north.dashboard.state/read-panel panel)
        text (get-in cached [:last-good :data :text])]
    (if (or fresh? (nil? text))
      (fresh! panel verb args)
      (do
        (print text)
        (when-not (str/ends-with? text "\n") (println))
        (println (dim (str "· as of " (north.dashboard.state/age cached) " ago — refreshing")))
        (launch! verb args)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
