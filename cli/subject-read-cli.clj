#!/usr/bin/env bb
;; Exact subject-addressed reads for Stage-A telemetry partitioning.
;;
;; bin/north invokes this only for telemetry-owned show/history subjects while
;; NORTH_TELEMETRY_PARTITION=1. The read is fenced through north.coord before
;; rendering, so a dead, wrong-log, or incompatible telemetry writer cannot be
;; misreported as an empty subject. A1/rollback never enters this adapter.
(ns north.subject-read-cli
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [fram.kernel :as kernel]
            [fram.main :as fram-main]
            [fram.rt :as rt]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(defn- coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn- subject-of [id]
  (if (str/starts-with? id "@") id (str "@" id)))

(defn- bare-subject [subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn- fail! [command subject error]
  (binding [*out* *err*]
    (println
     (str "north: " command " REFUSED — telemetry writer unavailable for "
          subject
          (when-let [detail (some-> error .getMessage not-empty)]
            (str ": " detail)))))
  (System/exit 4))

(defn- authoritative-show [command subject]
  (try
    (north.coord/show-envelope (coordination-port) subject)
    (catch Exception error
      (fail! command subject error))))

(def ^:private implicit-provenance-predicates
  #{"bar_evidence" "progress" "outcome"})

(defn- canonical-render-required? [rows provenance?]
  (or provenance?
      (some (fn [[predicate _]]
              (contains? implicit-provenance-predicates predicate))
            rows)))

(defn- render-exact-rows! [subject rows]
  (if (seq rows)
    (doseq [[predicate value] rows]
      (println (str "  " predicate "  " value)))
    (println (str "no facts for " subject))))

(defn- show! [id provenance?]
  (let [subject (subject-of id)
        response (authoritative-show "show" subject)
        rows (:rows response)]
    (if (canonical-render-required? rows provenance?)
      (let [facts (mapv (fn [[predicate value]]
                          (kernel/->Fact subject predicate value))
                        rows)
            telemetry-log (north.coord/telemetry-log-path)]
        ;; Provenance is log metadata, so only provenance-bearing output pays
        ;; for Fram's canonical log-backed renderer.
        (with-redefs [rt/coord-live-facts (fn [_port _log] facts)]
          (fram-main/cmd-show telemetry-log (bare-subject subject) provenance?)))
      (render-exact-rows! subject rows))))

(defn- history! [id]
  (let [subject (subject-of id)]
    ;; History is read from the telemetry origin log, but only after its sole
    ;; writer proves the fenced origin is currently authoritative.
    (authoritative-show "history" subject)
    (rt/history (north.coord/telemetry-log-path) subject)))

(defn -main [& args]
  (let [command (first args)
        id (second args)
        subject (when (and id (not (str/blank? id))) (subject-of id))]
    (cond
      (not (north.coord/telemetry-partition-enabled?))
      (do
        (binding [*out* *err*]
          (println "north: telemetry subject reader requires NORTH_TELEMETRY_PARTITION=1"))
        (System/exit 2))

      (or (nil? subject) (not (north.coord/telemetry-subject? subject)))
      (do
        (binding [*out* *err*]
          (println (str "usage: " (or command "show") " <exact telemetry subject>")))
        (System/exit 2))

      (= command "show")
      (show! id (= "--provenance" (nth args 2 nil)))

      (= command "history")
      (history! id)

      :else
      (do
        (binding [*out* *err*]
          (println "usage: show|history <exact telemetry subject>"))
        (System/exit 2)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
