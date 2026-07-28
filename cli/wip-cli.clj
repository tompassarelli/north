#!/usr/bin/env bb
;; wip-cli.clj — `north wip`: live managed-lane capacity against ready work.
;;
;; READ-ONLY. The work projection is one bounded Datalog fold. It emits only
;; facts needed by north.projections/ready + leverage-score, rather than moving
;; the whole live graph across the coordinator wire. Presence remains the
;; canonical renewable session-lease projection.
(ns north.wip-cli
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [fram.kernel :as k]
            [north.projections :as projections]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def default-floor 4)
(def port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))
(def lease-prefix "@lease:session:")
(def max-live-controls 256)
(def max-query-rows 100000)
(def lane-kind-tag "north:wip/lane-kind")
(def lane-thread-tag "north:wip/lane-thread")
(def coordinator-thread-tag "north:wip/coordinator-thread")

(def terminal-predicates ["outcome" "abandoned" "superseded_by"])
(def anchor-axis-predicates
  ["driver" "depends_on" "part_of" "do_on" "valid_until" "estimate_hours"
   "lead" "proposed_by" "created_at" "updated_at" "repo"])
(def candidate-predicates
  ["title" "committed" "driver" "depends_on" "part_of" "do_on"
   "valid_until" "estimate_hours" "lead" "proposed_by" "created_at"
   "updated_at" "repo"])

(defn variable [name] {:var name})
(defn literal
  ([rel args] {:rel rel :args args})
  ([rel args neg?] {:rel rel :args args :neg neg?}))
(defn rule [rel args body] {:head {:rel rel :args args} :body body})

(defn work-query
  "Return the one work projection. LIVE-CONTROLS are already lease-derived and
  only bound constants are added, keeping lane hydration bounded."
  [live-controls include-reservations?]
  (let [e (variable "e")
        r (variable "r")
        title (variable "title")
        dependent (variable "dependent")
        dependency (variable "dependency")
        middle (variable "middle")
        run (variable "run")
        thread (variable "thread")
        terminal-rules
        (mapv #(rule "terminal" [e]
                     [(literal "triple" [e % r])])
              terminal-predicates)
        work-axis-rules
        (mapv #(rule "work_axis" [e]
                     [(literal "triple" [e % r])])
              anchor-axis-predicates)
        candidate-rules
        [(rule "candidate" [e title]
               [(literal "triple" [e "title" title])
                (literal "triple" [e "committed" r])
                (literal "work_axis" [e])
                (literal "terminal" [e] true)])]
        reach-rules
        [(rule "reaches" [e dependent]
               [(literal "candidate" [e title])
                (literal "triple" [dependent "depends_on" e])])
         (rule "reaches" [e dependent]
               [(literal "reaches" [e middle])
                (literal "triple" [dependent "depends_on" middle])])]
        candidate-fact-rules
        (mapv
         (fn [predicate]
           (rule "wip_row" [e predicate r]
                 [(literal "candidate" [e title])
                  (literal "triple" [e predicate r])]))
         candidate-predicates)
        graph-fact-rules
        (vec
         (concat
          [(rule "wip_row" [e "depends_on" r]
                 [(literal "triple" [e "depends_on" r])])
           (rule "wip_row" [dependency "title" title]
                 [(literal "triple" [e "depends_on" dependency])
                  (literal "triple" [dependency "title" title])])
           (rule "wip_row" [e "wip_floor" r]
                 [(literal "triple" [e "wip_floor" r])])
           (rule "wip_row" [e "north:wip/leverage-dependent" dependent]
                 [(literal "candidate" [e title])
                  (literal "reaches" [e dependent])
                  (literal "terminal" [dependent] true)])]
          (map
           #(rule "wip_row" [e % r]
                  [(literal "triple" [e % r])])
           terminal-predicates)))
        lane-rules
        (vec
         (mapcat
          (fn [control]
            (let [agent-subject (str "@agent:" control)
                  reporter agent-subject
                  session-subject (str "@session:" control)]
              (cond->
               [(rule "wip_row" [control lane-kind-tag "lane"]
                      [(literal "triple" [agent-subject "kind" "lane"])])
                (rule "wip_row" [control coordinator-thread-tag thread]
                      [(literal "triple" [session-subject "current_thread" thread])])]
                include-reservations?
                (into
                 [(rule "wip_row" [control lane-thread-tag thread]
                        [(literal "triple" [agent-subject "kind" "lane"])
                         (literal "triple" [run "run_reservation_agent" reporter])
                         (literal "triple" [run "run_reservation_thread" thread])])
                  (rule "wip_row" [control coordinator-thread-tag thread]
                        [(literal "triple" [run "run_reservation_agent" reporter])
                         (literal "triple" [run "run_reservation_thread" thread])])
                  (rule "wip_row" [thread "title" title]
                        [(literal "triple" [agent-subject "kind" "lane"])
                         (literal "triple" [run "run_reservation_agent" reporter])
                         (literal "triple" [run "run_reservation_thread" thread])
                         (literal "triple" [thread "title" title])])]))))
          live-controls))]
    {:find "wip_row"
     :strata
     [terminal-rules
      work-axis-rules
      candidate-rules
      reach-rules
      (vec (concat candidate-fact-rules graph-fact-rules lane-rules))]}))

(defn reservation-query [live-controls]
  (let [run (variable "run")
        thread (variable "thread")]
    {:find "wip_row"
     :rules
     (vec
      (mapcat
       (fn [control]
         (let [reporter (str "@agent:" control)]
           [(rule "wip_row" [control lane-thread-tag thread]
                  [(literal "triple" [run "run_reservation_agent" reporter])
                   (literal "triple" [run "run_reservation_thread" thread])])
            (rule "wip_row" [control coordinator-thread-tag thread]
                  [(literal "triple" [run "run_reservation_agent" reporter])
                   (literal "triple" [run "run_reservation_thread" thread])])]))
       live-controls))}))

(defn strict-query-rows [response]
  (let [rows (:ok response)]
    (when-not
     (and (map? response)
          (integer? (:version response))
          (vector? rows)
          (<= (count rows) max-query-rows)
          (every? #(and (vector? %) (= 3 (count %))
                        (every? string? %))
                  rows))
      (throw (ex-info "coordinator returned a malformed WIP projection" {})))
    rows))

(defn query-rows [live-controls]
  (let [partitioned? (north.coord/telemetry-partition-enabled?)
        coordination
        (strict-query-rows
         (north.coord/send-op
          port {:op :query
                :query (work-query live-controls (not partitioned?))}))]
    (if-not partitioned?
      coordination
      (let [telemetry
            (binding [north.coord/*operation-domain* :telemetry]
              (strict-query-rows
               (north.coord/send-op
                port {:op :query
                      :query (reservation-query live-controls)})))]
        (vec (concat coordination telemetry))))))

(defn parse-lease [entity value]
  (let [parts (str/split value #"\|" -1)
        expiry (when (= 3 (count parts)) (parse-long (nth parts 1)))]
    (when-not
     (and (str/starts-with? entity lease-prefix)
          (= 3 (count parts))
          (not (str/blank? (first parts)))
          (some? expiry)
          (= (subs entity (count lease-prefix)) (first parts)))
      (throw (ex-info "coordinator returned a malformed session lease" {})))
    {:control (first parts) :expiry expiry}))

(defn live-controls
  ([] (live-controls (System/currentTimeMillis)))
  ([now-ms]
   (let [rows
         (north.coord/agg-rows
          port ["e" "r"]
          [(literal "triple" [(variable "e") "lease" (variable "r")])])
         session-rows (filter #(str/starts-with? (first %) lease-prefix) rows)
         parsed (mapv #(parse-lease (first %) (second %)) session-rows)
         live (->> parsed
                   (filter #(< now-ms (:expiry %)))
                   (map :control)
                   distinct
                   sort
                   vec)]
     (when (> (count live) max-live-controls)
       (throw (ex-info "live session roster exceeds the WIP bound" {})))
     live)))

(defn fact-row? [[_ predicate _]]
  (not (or (= predicate lane-kind-tag)
           (= predicate lane-thread-tag)
           (= predicate coordinator-thread-tag)
           (= predicate "north:wip/leverage-dependent"))))

(defn rows->index [rows]
  (k/build-index
   (->> rows
        (filter fact-row?)
        (mapv (fn [[subject predicate value]]
                (k/->Fact subject predicate value))))))

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
      (when-let [driver (k/one-i idx thread "driver")]
        (let [control (str/replace-first driver #"^@" "")
              updated (some-> (k/one-i idx thread "updated_at") parse-date-ms)]
          (boolean
           (or (contains? live-set control)
               (and updated (< (- now-ms updated) window-ms)))))))))

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
          raw (when thread (k/one-i idx thread "wip_floor"))]
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
        ready (projections/ready idx today #(< (compare %1 %2) 0) live?)
        scored
        (->> ready
             (map (fn [thread]
                    {:thread thread
                     :title (or (k/one-i idx thread "title") "")
                     :leverage
                     (count
                      (row-values
                       rows thread "north:wip/leverage-dependent"))}))
             (sort-by (juxt (comp - :leverage) :thread))
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
              :title (when thread (k/one-i idx thread "title"))
              :binding-conflict (> (count bindings) 1)}))
         lane-controls)
        floor (resolve-floor idx rows options)]
    {:live (count lanes)
     :floor floor
     :ready-depth (count ready)
     :lanes lanes
     :top (vec (take 5 scored))
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
      (let [[arg value & more] remaining]
        (cond
          (= arg "--check")
          (if (:check options)
            (throw (ex-info "duplicate --check" {:usage true}))
            (recur (vec (cons value more)) (assoc options :check true)))

          (= arg "--floor")
          (let [floor (valid-floor value)]
            (when-not floor
              (throw
               (ex-info "--floor requires a non-negative integer" {:usage true})))
            (when (contains? options :floor)
              (throw (ex-info "duplicate --floor" {:usage true})))
            (recur (vec more) (assoc options :floor floor)))

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
   (let [controls (live-controls)
         rows (query-rows controls)]
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
