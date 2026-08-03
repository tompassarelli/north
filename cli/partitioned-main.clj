#!/usr/bin/env bb
;; Compatibility adapter for North's compiled Beagle CLI while run telemetry has
;; an independent writer. Reads compose each writer's materialized live facts.
(require '[fram.kernel :as kernel]
         '[fram.rt :as rt]
         '[north.main :as main]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def original-live-state rt/coord-live-state)
(defn coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn composed-live-facts [port log]
  (if-not (north.coord/telemetry-partition-enabled?)
    (when-let [state (original-live-state port log)]
      (assoc state :complete true))
    (let [{:keys [facts domains unavailable unavailable-detail complete]}
          (north.coord/live-facts-view (coordination-port))]
      (when (seq unavailable)
        (binding [*out* *err*]
          ;; Name WHY, not just WHICH. "unavailable domain(s): coordination"
          ;; reads as a dead daemon and was, on 2026-07-29, a response that had
          ;; simply outgrown its size cap — with the reason already in hand and
          ;; discarded one frame up.
          (println
           (str "north: partial cross-log view; unavailable domain(s): "
                (str/join ", " unavailable)))
          (doseq [[domain reason] unavailable-detail]
            (println (str "  " domain ": " reason)))))
      {:facts (mapv (fn [[subject predicate value]]
                      (kernel/->Fact subject predicate value))
                    facts)
       :domains domains
       :complete complete})))

(defn coordination-only-projection-command? [args]
  ;; The curated lifecycle views consume only thread, concern, driver, and lease
  ;; facts; their --all forms retain the historical cross-domain census.
  (and (contains? #{"ready" "board" "plate"} (first args))
       (not (some #{"--all"} args))))

(defn coordination-daemon-live-state [port log]
  (when-let [state (original-live-state port log)]
    (assoc state :complete true)))

(defn -main [& args]
  (let [live-reader (if (coordination-only-projection-command? args)
                      coordination-daemon-live-state
                      composed-live-facts)]
    (with-redefs
     [rt/coord-live-state live-reader
      main/compose-telemetry-log? (fn [] true)]
     (apply main/-main args))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
