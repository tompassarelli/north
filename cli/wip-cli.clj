#!/usr/bin/env bb
;; wip-cli.clj — `north wip`: live managed-lane capacity against ready work.
;;
;; READ-ONLY. Work and presence come from bounded FRAMRPC projections limited
;; to facts consumed by ready/leverage and live-lane projection. A configured
;; telemetry origin contributes reservation bindings.
(ns north.wip-cli
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [store.types :as t]
            [north.projections :as proj]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def default-floor 4)
(def max-live-controls 256)
(def lane-kind-tag "north:wip/lane-kind")
(def lane-thread-tag "north:wip/lane-thread")
(def coordinator-thread-tag "north:wip/coordinator-thread")
(def thread-predicates
  #{"title" "committed" "outcome" "abandoned" "superseded_by" "driver"
    "depends_on" "part_of" "do_on" "valid_until" "estimate_hours" "lead"
    "proposed_by" "created_at" "updated_at" "repo" "wip_floor" "kind"
    "entity_kind"})
(def reservation-predicates
  #{"run_reservation_agent" "run_reservation_thread"})
(def selected-predicates
  (into thread-predicates
        (concat #{"current_thread"} reservation-predicates)))
(def selected-page-limit north.coord/query-page-row-limit)

(defn selected-facts-query [predicates]
  {:find "north_wip_fact"
   :rules
   (mapv
    (fn [predicate]
      {:head {:rel "north_wip_fact"
              :args [{:var "subject"} predicate {:var "value"}]}
       :body [{:rel "triple"
               :args [{:var "subject"} predicate {:var "value"}]}]})
    (sort predicates))})

(defn selected-domain-rows [port domain predicates]
  (let [query (selected-facts-query predicates)]
    (loop [after nil at-version nil rows []]
      (let [response
            (north.coord/query-page-in-domain
             port domain query selected-page-limit after at-version)
            version (:served-version response)
            next-cursor (:cursor response)]
        (when (and at-version (not= at-version version))
          (throw (ex-info "WIP projection changed version while paging"
                          {:domain domain
                           :expected-version at-version
                           :actual-version version})))
        (when (and (not (:done? response)) (= after next-cursor))
          (throw (ex-info "WIP projection cursor did not advance"
                          {:domain domain})))
        (let [all-rows (into rows (:rows response))]
          (if (:done? response)
            all-rows
            (recur next-cursor (or at-version version) all-rows)))))))

(defn query-rows
  [work managed-controls session-threads reservation-bindings]
  (vec
   (concat
    work
    (map (fn [control] [control lane-kind-tag "lane"])
         (sort managed-controls))
    (map (fn [[control thread]]
           [control coordinator-thread-tag thread])
         (sort-by key session-threads))
    (mapcat
     (fn [control]
       (mapcat
        (fn [thread]
          [[control lane-thread-tag thread]
           [control coordinator-thread-tag thread]])
        (sort (get reservation-bindings control #{}))))
     (sort managed-controls)))))

(defn state-from-rows [rows]
  (reduce
   (fn [state [subject predicate value]]
     (cond
       (and (= predicate "kind")
            (= value "lane")
            (str/starts-with? subject "@agent:"))
       (update state :managed conj (subs subject (count "@agent:")))

       (and (= predicate "current_thread")
            (str/starts-with? subject "@agent:"))
       (assoc-in state [:session-threads
                        (subs subject (count "@agent:"))]
                 value)

       (contains? reservation-predicates predicate)
       (update-in state [:reservations subject predicate]
                  (fnil conj #{}) value)

       (contains? thread-predicates predicate)
       (update state :work conj [subject predicate value])

       :else state))
   {:managed #{}
    :session-threads {}
    :reservations {}
    :work []}
   rows))

(defn coordination-state []
  (let [port (parse-long north.coord/PORT)
        partitioned? (north.coord/telemetry-partition-enabled?)
        telemetry-future
        (when partitioned?
          (future
            (selected-domain-rows port :telemetry reservation-predicates)))
        coordination
        (selected-domain-rows port :coordination selected-predicates)
        telemetry (if telemetry-future @telemetry-future [])]
    (state-from-rows (into coordination telemetry))))

(defn reservation-bindings [reservations]
  (reduce-kv
   (fn [bindings _run facts]
     (let [agents (get facts "run_reservation_agent" #{})
           threads (get facts "run_reservation_thread" #{})]
       (if (and (= 1 (count agents)) (= 1 (count threads)))
         (let [control (str/replace-first (first agents) #"^@agent:" "")]
           (update bindings control (fnil conj #{}) (first threads)))
         bindings)))
   {}
   reservations))

(defn presence-state [now-ms]
  (let [{:keys [managed session-threads reservations work]}
        (coordination-state)
        controls
        (mapv :handle
              (north.coord/online-session-leases
               (parse-long north.coord/PORT) now-ms))]
    (when (> (count controls) max-live-controls)
      (throw (ex-info "live session roster exceeds the WIP bound" {})))
    {:controls controls
     :managed (set (filter managed controls))
     :session-threads (select-keys session-threads controls)
     :reservation-bindings
     (select-keys (reservation-bindings reservations) controls)
     :work work}))

(defn live-controls
  ([] (live-controls (System/currentTimeMillis)))
  ([now-ms]
   (:controls (presence-state now-ms))))

(defn fact-row? [[_ predicate _]]
  (not (or (= predicate lane-kind-tag)
           (= predicate lane-thread-tag)
           (= predicate coordinator-thread-tag))))

(defn rows->index [rows]
  (proj/index-triples
   (->> rows
        (filter fact-row?)
        (mapv (fn [[subject predicate value]]
                (t/triple subject predicate value))))))

(defn row-values [rows subject predicate]
  (->> rows
       (keep (fn [[s p value]]
               (when (and (= s subject) (= p predicate)) value)))
       distinct
       sort
       vec))

(defn parse-date-ms [value]
  (try
    (cond
      (re-matches #"\d{4}-\d{2}-\d{2}" value)
      (.toEpochMilli
       (.toInstant
        (.atStartOfDay
         (java.time.LocalDate/parse value)
         java.time.ZoneOffset/UTC)))

      (re-matches #"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}" value)
      (.toEpochMilli
       (.toInstant
        (.atZone
         (java.time.LocalDateTime/parse value)
         java.time.ZoneOffset/UTC)))

      (re-matches #"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}" value)
      (.toEpochMilli
       (.toInstant
        (.atZone
         (java.time.LocalDateTime/parse value)
         java.time.ZoneOffset/UTC)))

      :else nil)
    (catch Exception _ nil)))

(defn driver-live-predicate [live-set now-ms]
  (let [raw-days (or (System/getenv "NORTH_DRIVER_STALE_DAYS") "14")
        parsed-days (parse-long raw-days)
        days (if (and parsed-days (pos? parsed-days)) parsed-days 14)
        window-ms (* days 86400000)]
    (fn [idx thread]
      (when-let [driver (proj/string-value-at idx thread "driver")]
        (let [control (str/replace-first driver #"^@" "")
              updated
              (some-> (proj/string-value-at idx thread "updated_at")
                      parse-date-ms)]
          (boolean
           (or (contains? live-set control)
               (and updated (< (- now-ms updated) window-ms)))))))))

(defn work-thread? [idx subject]
  (let [bare (str/replace-first subject #"^@" "")
        explicit (proj/string-value-at idx subject "entity_kind")]
    (cond
      explicit (= explicit "thread")
      (str/starts-with? bare "concern-") false
      (str/starts-with? bare "agent:") false
      (or (str/starts-with? bare "msg:")
          (str/starts-with? bare "cmd:")) false
      (str/starts-with? bare "topic-") false
      (str/starts-with? bare "mine:") false
      (or (str/starts-with? bare "run-")
          (str/starts-with? bare "run:")) false
      (or (str/starts-with? bare "session:")
          (str/starts-with? bare "sess-")
          (str/starts-with? bare "cc-")) false
      (str/starts-with? bare "denial:") false
      (str/starts-with? bare "snapshot:") false
      (str/starts-with? bare "arena-") false
      :else (some? (proj/string-value-at idx subject "title")))))

(defn valid-floor [value]
  (let [parsed (some-> value str parse-long)]
    (when (and parsed (not (neg? parsed))) parsed)))

(defn resolve-coordinator-thread [rows {:keys [coordinator coordinator-thread]}]
  (or (some-> coordinator-thread str/trim not-empty)
      (when coordinator
        (let [threads (row-values rows coordinator coordinator-thread-tag)]
          (when (= 1 (count threads)) (first threads))))))

(defn resolve-floor [idx rows options]
  (if (contains? options :floor)
    (:floor options)
    (let [thread (resolve-coordinator-thread rows options)
          raw (when thread (proj/string-value-at idx thread "wip_floor"))]
      (cond
        (nil? raw) default-floor
        (valid-floor raw) (valid-floor raw)
        :else
        (throw
         (ex-info (str "wip_floor on " thread
                       " must be a non-negative integer")
                  {:thread thread :value raw}))))))

(defn snapshot
  [rows live-controls options now-ms]
  (let [idx (rows->index rows)
        live-set (set live-controls)
        live? (driver-live-predicate live-set now-ms)
        today (str (java.time.LocalDate/now java.time.ZoneOffset/UTC))
        ready (->> (proj/ready idx today #(< (compare %1 %2) 0) live?)
                   (filterv #(work-thread? idx %)))
        scored
        (->> ready
             (map (fn [thread]
                    {:thread thread
                     :leverage
                     (if (seq (proj/dependents-of idx thread))
                       (proj/leverage-score idx thread)
                       0)}))
             (sort-by (juxt (comp - :leverage) :thread))
             (take 5)
             (mapv
              (fn [row]
                (assoc row :title
                       (or (proj/string-value-at
                            idx (:thread row) "title") ""))))
             vec)
        lane-controls
        (->> live-controls
             (filter #(seq (row-values rows % lane-kind-tag)))
             sort
             vec)
        lanes
        (mapv
         (fn [control]
           (let [bindings (row-values rows control lane-thread-tag)
                 thread (when (= 1 (count bindings)) (first bindings))]
             {:control control
              :thread thread
              :title (when thread
                       (proj/string-value-at idx thread "title"))
              :binding-conflict (> (count bindings) 1)}))
         lane-controls)
        floor (resolve-floor idx rows options)]
    {:live (count lanes)
     :floor floor
     :ready-depth (count ready)
     :lanes lanes
     :top scored
     :shortfall (and (< (count lanes) floor) (pos? (count ready)))}))

(defn render-report [{:keys [live floor ready-depth lanes top shortfall]}]
  (let [pull (map :thread top)
        verdict
        (str "WIP " live "/" floor " — "
             (if shortfall
               (str "SHORTFALL: pull " (str/join " " pull))
               "OK"))]
    (str/join
     "\n"
     (concat
      [(str "LIVE MANAGED LANES — " live)]
      (if (seq lanes)
        (map
         (fn [{:keys [control thread title binding-conflict]}]
           (str "  " control " → "
                (cond
                  binding-conflict "CONFLICT"
                  thread (str thread (when (not (str/blank? title))
                                       (str "  " title)))
                  :else "unbound")))
         lanes)
        ["  none"])
      [(str "READY — " ready-depth " committed, undriven")
       "TOP PULLS — leverage-ranked"]
      (if (seq top)
        (map
         (fn [{:keys [thread title leverage]}]
           (str "  [" leverage "] " thread "  " title))
         top)
        ["  none"])
      [verdict]))))

(defn parse-options [args]
  (loop [remaining (vec args)
         options {:check false}]
    (if (empty? remaining)
      options
      (let [arg (first remaining)
            more (vec (rest remaining))
            value (first more)
            after-value (vec (rest more))]
        (cond
          (= arg "--check")
          (if (:check options)
            (throw (ex-info "duplicate --check" {:usage true}))
            (recur more (assoc options :check true)))

          (= arg "--floor")
          (let [floor (valid-floor value)]
            (when-not floor
              (throw
               (ex-info "--floor requires a non-negative integer" {:usage true})))
            (when (contains? options :floor)
              (throw (ex-info "duplicate --floor" {:usage true})))
            (recur after-value (assoc options :floor floor)))

          (#{"--help" "-h" "help"} arg)
          (if (or value (not= options {:check false}))
            (throw (ex-info "help cannot be combined with other options"
                            {:usage true}))
            (assoc options :help true))

          :else
          (throw (ex-info (str "unknown option: " arg) {:usage true})))))))

(defn environment-options [options]
  (assoc
   options
   :coordinator
   (some
    #(some-> (System/getenv %) str/trim not-empty)
    ["NORTH_WIP_COORDINATOR" "NORTH_WIP_WATCH_COORDINATOR"
     "AGENT_COORDINATOR" "AGENT_ID"])
   :coordinator-thread
   (some
    #(some-> (System/getenv %) str/trim not-empty)
    ["NORTH_THREAD_ID" "AGENT_THREAD"])))

(defn execute
  ([rows controls options now-ms]
   (let [report (snapshot rows controls options now-ms)]
     {:exit (if (and (:check options) (:shortfall report)) 3 0)
      :output (render-report report)
      :report report}))
  ([options]
   (let [{:keys [controls managed session-threads reservation-bindings work]}
         (presence-state (System/currentTimeMillis))
         rows (query-rows
               work managed session-threads reservation-bindings)]
     (execute rows controls options (System/currentTimeMillis)))))

(defn usage []
  (println "usage: north wip [--floor N] [--check]")
  (println "  --floor N  override the coordinator thread's wip_floor (default 4)")
  (println "  --check    exit 3 only when live lanes are below floor and ready > 0"))

(defn main [args]
  (try
    (let [options (parse-options args)]
      (if (:help options)
        (do (usage) 0)
        (let [{:keys [exit output]} (execute (environment-options options))]
          (println output)
          exit)))
    (catch clojure.lang.ExceptionInfo error
      (binding [*out* *err*]
        (println (str "north wip: " (.getMessage error)))
        (when (:usage (ex-data error)) (usage)))
      (if (:usage (ex-data error)) 2 1))
    (catch Exception error
      (binding [*out* *err*]
        (println (str "north wip: " (.getMessage error))))
      1)))

(when-not (= "1" (System/getProperty "north.wip.lib"))
  (System/exit (main *command-line-args*)))
