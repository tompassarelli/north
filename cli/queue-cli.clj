#!/usr/bin/env bb
;; Durable manual ordering for the derived ready lane. A move writes one
;; single-valued receipt; lifecycle condition remains projection-only.
(ns north.queue-cli
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [north.main :as main]))

(def ^:private cli-dir
  (.getParent
   (io/file (or (System/getProperty "babashka.file") *file*))))

(load-file (str cli-dir "/coord.clj"))

(def positions #{"before" "after" "first" "last"})

(def thread-id-pattern
  #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

(defn- exact-thread [value]
  (let [trimmed (some-> value str str/trim)
        bare (some-> trimmed (str/replace-first #"^@" ""))]
    (when (and bare (re-matches thread-id-pattern bare))
      (str "@" bare))))

(defn- subject-facts [port subject]
  (reduce (fn [facts [predicate value]] (assoc facts predicate value))
          {}
          (north.coord/show-rows port subject)))

(defn- require-thread! [port subject role]
  (let [facts (subject-facts port subject)
        kind (get facts "kind")]
    (when-not (and (string? (get facts "title"))
                   (or (nil? kind) (= kind "thread")))
      (throw (ex-info (str "queue " role " is not an existing work thread")
                      {:type :not-work-thread :role role :subject subject})))
    subject))

(defn- plan-move!
  [port target position raw-anchor receipt]
  (let [version (north.coord/cur-ver-for-subject port target)
        _ (require-thread! port target "target")
        anchor (when (#{"before" "after"} position)
                 (require-thread! port raw-anchor "anchor"))]
    (when (= target anchor)
      (throw (ex-info "queue target and anchor must differ"
                      {:type :self-anchor :target target})))
    (let [rank (main/queue-rank-value version position (or anchor ""))]
      (reset! receipt {:target target :position position :anchor anchor :rank rank})
      {:facts [{:p "queue_rank" :r rank}]})))

(defn move!
  [port raw-target position raw-anchor]
  (when-not (contains? positions position)
    (throw (ex-info "queue position must be before, after, first, or last"
                    {:type :invalid-position :position position})))
  (when (and (#{"before" "after"} position) (str/blank? raw-anchor))
    (throw (ex-info "before and after require an anchor thread"
                    {:type :missing-anchor :position position})))
  (when (and (#{"first" "last"} position) (not (str/blank? raw-anchor)))
    (throw (ex-info "first and last do not accept an anchor thread"
                    {:type :unexpected-anchor :position position})))
  (let [target (exact-thread raw-target)
        anchor (when (#{"before" "after"} position) (exact-thread raw-anchor))]
    (when-not target
      (throw (ex-info "queue target must be an exact UUID thread id"
                      {:type :invalid-target :target raw-target})))
    (when (and (#{"before" "after"} position) (nil? anchor))
      (throw (ex-info "queue anchor must be an exact UUID thread id"
                      {:type :invalid-anchor :anchor raw-anchor})))
    (let [receipt (atom nil)
          result
          (north.coord/assert-batch-after-read!
           port target
           (fn [] (plan-move! port target position anchor receipt)))]
      (when (:reject result)
        (throw (ex-info "queue move lost coordinator contention"
                        {:type :queue-move-conflict :response result})))
      @receipt)))

(defn run! [args]
  (let [[port-token verb raw-target position raw-anchor & extra] args]
    (when-not (and (= verb "move")
                   raw-target
                   position
                   (empty? extra))
      (throw (ex-info "usage: north queue move <thread> <before|after|first|last> [anchor]"
                      {:type :usage})))
    (let [port (parse-long port-token)]
      (when-not (and port (<= 1 port 65535))
        (throw (ex-info "queue coordinator port must be 1..65535"
                        {:type :invalid-port :port port-token})))
      (move! (int port) raw-target position raw-anchor))))

(when (= *file* (System/getProperty "babashka.file"))
  (try
    (let [{:keys [target position anchor]} (run! *command-line-args*)]
      (println (str "QUEUED " target " " position
                    (if anchor (str " " anchor) ""))))
    (catch Exception error
      (binding [*out* *err*]
        (println (str "north queue: " (.getMessage error))))
      (System/exit (if (= :usage (:type (ex-data error))) 2 1)))))
