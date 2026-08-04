#!/usr/bin/env bb
(ns north.reconciliation-worker-host
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [babashka.process :as proc]))

(def root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getCanonicalPath))
(load-file (str root "/out/north/worker_policy.clj"))

(def cli-dir (str root "/cli"))
(def concern-cli (str cli-dir "/concern-cli.clj"))
(def port
  (or (System/getenv "NORTH_PORT")
      (System/getenv "FRAM_PORT")
      "7977"))
(def spool-dir
  (io/file
   (or (System/getenv "NORTH_CONCERN_SPOOL_DIR")
       (str (or (System/getenv "XDG_STATE_HOME")
                (str (System/getProperty "user.home") "/.local/state"))
            "/north/concern-operations"))))

(defn pending-operation-count []
  (if-not (.isDirectory spool-dir)
    0
    (count
     (filter
      #(and (.isFile ^java.io.File %)
            (str/ends-with? (.getName ^java.io.File %) ".op.edn"))
      (.listFiles spool-dir)))))

(defn run-concern-pass! []
  (proc/shell
   {:out :string
    :err :string
    :continue true
    :extra-env
    {"NORTH_CONCERN_RECONCILE_MAX_ITEMS" "64"
     "NORTH_CONCERN_RECONCILE_MAX_MILLIS" "20000"}}
   "bb" concern-cli port "reconcile-local" "--operations-only"))

(defn run-attention-pass! []
  (proc/shell
   {:out :string :err :string :continue true}
   "bb" concern-cli port "reconcile-attention"))

(defn print-result! [worker result]
  (let [output (str/trim (str (:out result)))
        error (str/trim (str (:err result)))]
    (when (and (seq output)
               (or (= worker :concern)
                   (not (str/includes? output "events=0"))))
      (println (str "[" (name worker) "-reconciliation-worker] " output)))
    (when (seq error)
      (binding [*out* *err*]
        (println
         (str "[" (name worker) "-reconciliation-worker] " error))))
    (flush)))

(defn apply-decision! [decision]
  (let [sleep-ms (:sleep-ms decision)]
    (when (pos? sleep-ms)
      (Thread/sleep sleep-ms)))
  (:next-backoff-ms decision))

(defn attention-more? [result]
  (boolean (re-find #"(?:^|\\s)more=true(?:\\s|$)" (str (:out result)))))

(defn run-concern-worker! []
  (println
   (str "[concern-reconciliation-worker] port=" port
        " spool=" (.getPath spool-dir)))
  (flush)
  (loop [backoff-ms north.worker-policy/concern-idle-ms]
    (let [pending (pending-operation-count)]
      (if (zero? pending)
        (let [decision
              (north.worker-policy/concern-reconciliation-decision
               0 0 backoff-ms)]
          (recur (apply-decision! decision)))
        (let [result (run-concern-pass!)
              decision
              (north.worker-policy/concern-reconciliation-decision
               (pending-operation-count)
               (:exit result)
               backoff-ms)]
          (print-result! :concern result)
          (recur (apply-decision! decision)))))))

(defn run-attention-worker! []
  (println (str "[attention-reconciliation-worker] port=" port))
  (flush)
  (loop [backoff-ms north.worker-policy/attention-interval-ms]
    (let [result (run-attention-pass!)
          decision
          (north.worker-policy/attention-reconciliation-decision
           (:exit result) backoff-ms (attention-more? result))]
      (print-result! :attention result)
      (recur (apply-decision! decision)))))

(case (first *command-line-args*)
  "concerns" (run-concern-worker!)
  "attention" (run-attention-worker!)
  (do
    (binding [*out* *err*]
      (println
       "usage: reconciliation-worker-host.clj {concerns|attention}"))
    (System/exit 2)))
