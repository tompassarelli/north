#!/usr/bin/env bb
;; wip-cli.clj — `north wip`: live managed-lane capacity against ready work.
;;
;; READ-ONLY. Work and presence are one bounded fold of North's canonical
;; append log, limited to facts consumed by ready/leverage and live-lane
;; projection. A configured telemetry origin contributes reservation bindings.
(ns north.wip-cli
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [babashka.process :as process]
            [fram.kernel :as k]
            [north.projections :as projections]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def default-floor 4)
(def lease-prefix "@lease:session:")
(def max-live-controls 256)
(def lane-kind-tag "north:wip/lane-kind")
(def lane-thread-tag "north:wip/lane-thread")
(def coordinator-thread-tag "north:wip/coordinator-thread")
(def coordination-log (north.coord/expected-log))
(def telemetry-log (some-> (System/getenv "FRAM_TELEMETRY_LOG") str/trim not-empty))
(def thread-predicates
  #{"title" "committed" "outcome" "abandoned" "superseded_by" "driver"
    "depends_on" "part_of" "do_on" "valid_until" "estimate_hours" "lead"
    "proposed_by" "created_at" "updated_at" "repo" "wip_floor" "kind"
    "entity_kind"})
(def selected-facts-awk
  (str
   "BEGIN {"
   " work[\"title\"]=work[\"committed\"]=work[\"outcome\"]=1;"
   " work[\"abandoned\"]=work[\"superseded_by\"]=work[\"driver\"]=1;"
   " work[\"depends_on\"]=work[\"part_of\"]=work[\"do_on\"]=1;"
   " work[\"valid_until\"]=work[\"estimate_hours\"]=work[\"lead\"]=1;"
   " work[\"proposed_by\"]=work[\"created_at\"]=work[\"updated_at\"]=1;"
   " work[\"repo\"]=work[\"wip_floor\"]=work[\"kind\"]=work[\"entity_kind\"]=1;"
   " reservation[\"run_reservation_agent\"]=1;"
   " reservation[\"run_reservation_thread\"]=1;"
   " multi[\"depends_on\"]=multi[\"proposed_by\"]=multi[\"repo\"]=1;"
   " multi[\"run_reservation_agent\"]=1;"
   " multi[\"run_reservation_thread\"]=1"
   " }"
   " {"
   " marker=index($0, \", :p \\\"\");"
   " rest=substr($0, marker + 6);"
   " predicate=substr(rest, 1, index(rest, \"\\\"\") - 1);"
   " selected=((ARGIND == 1 &&"
   "           (work[predicate] || predicate == \"lease\" ||"
   "            predicate == \"current_thread\")) || reservation[predicate]);"
   " if (!selected) next;"
   " lmarker=index($0, \", :l \\\"\");"
   " subject=substr($0, lmarker + 6, marker - (lmarker + 6) - 1);"
   " rmarker=index($0, \", :r \\\"\");"
   " rstart=rmarker + 6;"
   " rest=substr($0, rstart);"
   " rend=index(rest, \"\\\", :ts \");"
   " frameend=index(rest, \"\\\", :frame \");"
   " if (frameend && frameend < rend) rend=frameend;"
   " value=substr(rest, 1, rend - 1);"
   " operation=(index(substr($0, 1, 64), \":op \\\"assert\\\"\") ?"
   "            \"assert\" : \"retract\");"
   " key=subject SUBSEP predicate;"
   " if (multi[predicate]) key=key SUBSEP value;"
   " if (operation == \"assert\") {"
   "   live[key]=value; subjects[key]=subject; predicates[key]=predicate"
   " } else if ((key in live) && live[key] == value) {"
   "   delete live[key]; delete subjects[key]; delete predicates[key]"
   " }"
   " }"
   " END {"
   " for (key in live)"
   "   print subjects[key] \"\\t\" predicates[key] \"\\t\" live[key]"
   " }"))

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

(defn decode-log-string [value]
  (if (str/includes? value "\\")
    (edn/read-string (str "\"" value "\""))
    value))

(defn folded-row [line]
  (let [first-tab (.indexOf line "\t")
        second-tab (.indexOf line "\t" (inc first-tab))]
    (when (or (neg? first-tab) (neg? second-tab))
      (throw (ex-info "malformed live triple from canonical log fold" {})))
    [(decode-log-string (subs line 0 first-tab))
     (subs line (inc first-tab) second-tab)
     (decode-log-string (subs line (inc second-tab)))]))

(defn fold-selected-reader [reader]
    (loop [leases {}
           managed #{}
           session-threads {}
           reservations {}
           work (transient [])]
      (if-let [line (.readLine reader)]
        (let [[subject predicate value] (folded-row line)]
          (cond
            (and (= predicate "lease")
                 (str/starts-with? subject lease-prefix))
            (recur (assoc leases subject value)
                   managed session-threads reservations work)

            (and (= predicate "kind")
                 (= value "lane")
                 (str/starts-with? subject "@agent:"))
            (recur leases
                   (conj managed (subs subject (count "@agent:")))
                   session-threads reservations work)

            (and (= predicate "current_thread")
                 (str/starts-with? subject "@session:"))
            (recur leases managed
                   (assoc session-threads
                          (subs subject (count "@session:"))
                          value)
                   reservations work)

            (#{"run_reservation_agent" "run_reservation_thread"} predicate)
            (recur leases managed session-threads
                   (update-in reservations [subject predicate]
                              (fnil conj #{}) value)
                   work)

            (contains? thread-predicates predicate)
            (recur leases managed session-threads reservations
                   (conj! work [subject predicate value]))

            :else
            (recur leases managed session-threads reservations work)))
        {:leases leases
         :managed managed
         :session-threads session-threads
         :reservations reservations
         :work (persistent! work)})))

(defn fold-log-paths [coordination-path telemetry-path]
  (let [command
        (cond-> ["gawk" selected-facts-awk coordination-path]
          telemetry-path (conj telemetry-path))
        child (process/process command {:out :stream :err :string})
        state
        (with-open [reader (io/reader (:out child))]
          (fold-selected-reader reader))
        result @child]
    (when-not (zero? (:exit result))
      (throw
       (ex-info
        (str "canonical log selection failed: " (str/trim (:err result)))
        {:exit (:exit result)})))
    state))

(defn coordination-state []
  (fold-log-paths coordination-log telemetry-log))

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
  (let [{:keys [leases managed session-threads reservations work]}
        (coordination-state)
        parsed (mapv (fn [[entity value]] (parse-lease entity value)) leases)
        controls (->> parsed
                      (filter #(< now-ms (:expiry %)))
                      (map :control)
                      distinct
                      sort
                      vec)]
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

(defn work-thread? [idx subject]
  (let [bare (str/replace-first subject #"^@" "")
        explicit (k/one-i idx subject "entity_kind")
        legacy (k/one-i idx subject "kind")]
    (cond
      explicit (= explicit "thread")
      legacy (= legacy "thread")
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
      :else (some? (k/one-i idx subject "title")))))

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
        ready (->> (projections/ready idx today #(< (compare %1 %2) 0) live?)
                   (filterv #(work-thread? idx %)))
        scored
        (->> ready
             (map (fn [thread]
                    {:thread thread
                     :leverage
                     (if (seq (k/dependents-i idx thread))
                       (projections/leverage-score idx thread)
                       0)}))
             (sort-by (juxt (comp - :leverage) :thread))
             (take 5)
             (mapv
              (fn [row]
                (assoc row :title
                       (or (k/one-i idx (:thread row) "title") ""))))
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
