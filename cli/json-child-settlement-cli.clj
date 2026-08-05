#!/usr/bin/env bb
;; Exact indexed child-settlement projection: one indexed query per envelope
;; component, because composing the whole live corpus through north.main does
;; not fit the SDK's 5s settlement deadline (sdk/src/children.ts).
;; Rows sort by (subject, predicate, value) — the order the whole-corpus
;; projection already emitted, so the envelope stays byte-identical.
(require '[cheshire.core :as json]
         '[clojure.string :as str]
         '[clojure.java.io :as io])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def show-envelope (ns-resolve 'north.coord 'show-envelope))

;; The FRAMRPC server's per-query row ceiling. Overflow REFUSES rather than
;; truncating: a short child or run set reads to the SDK as "those children
;; settled" and would retire a live lane.
(def max-rows 4096)

(def protocol "north.child-settlement")
(def envelope-version 1)

(def child-prefix "@agent:")

(defn- coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

;; Value-position literals only, so route-for-operation still routes these to
;; coordination: `coordinator` is a coordination-owned lifecycle predicate on
;; @agent: subjects, exactly like `part_of` in json-children-cli.
(defn- child-subject-query [coordinator]
  {:find "north_child"
   :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "coordinator" coordinator]}]}]})

(defn- child-fact-query [coordinator]
  {:find "north_child_fact"
   :rules [{:head {:rel "north_child_fact"
                   :args [{:var "subject"} {:var "predicate"} {:var "value"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "coordinator" coordinator]}
                   {:rel "triple"
                    :args [{:var "subject"} {:var "predicate"} {:var "value"}]}]}]})

;; A settlement-bearing run is a COMMITTED (kind=run) run tagged to an agent —
;; the same two-predicate conjunction the corpus path applied by hand.
(def tagged-run-query
  {:find "north_child_run"
   :rules [{:head {:rel "north_child_run" :args [{:var "subject"} {:var "agent"}]}
            :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]}
                   {:rel "triple" :args [{:var "subject"} "agent" {:var "agent"}]}]}]})

(defn- child-run-query [agent-id]
  {:find "north_child_run"
   :rules [{:head {:rel "north_child_run" :args [{:var "subject"}]}
            :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]}
                   {:rel "triple" :args [{:var "subject"} "agent" agent-id]}]}]})

(defn- short-id [subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn- malformed! [detail]
  (throw (ex-info (str "FRAMRPC server returned a malformed " detail)
                  {:type :malformed-settlement-row})))

(defn- rows-of-width [rows width detail]
  (when-not (every? #(and (= width (count %))
                          (every? string? %)
                          (not (str/blank? (first %))))
                    rows)
    (malformed! detail))
  rows)

(defn- row-limit? [error]
  (= :query-row-limit (:type (ex-data error))))

;; Runs live behind the telemetry partition (`run` is a telemetry subject token),
;; but the corpus path this replaces read the UNION of both origins, so ask both
;; and merge. With the partition disabled the two reads are the same query and
;; `distinct` collapses them.
(defn- both-domains [port query]
  (vec
   (distinct
    (concat
     (:rows (north.coord/bounded-query-in-domain
             port :coordination query max-rows))
     (:rows (north.coord/bounded-query-in-domain
             port :telemetry query max-rows))))))

(defn- child-subject? [subject]
  (str/starts-with? subject child-prefix))

(defn- subject-show-facts [port subject]
  (mapv (fn [[predicate value]] [subject predicate value])
        (:rows (show-envelope port subject))))

;; One join answers every child's complete fact set in a single round trip. A
;; database wide enough to overflow that join still gets an exact answer,
;; one indexed per-subject read at a time, instead of a refusal.
(defn- child-facts [port coordinator]
  (let [joined (try
                 (rows-of-width (:rows (north.coord/bounded-query
                                        port (child-fact-query coordinator) max-rows))
                                3 "child fact row")
                 (catch Exception error
                   (when-not (row-limit? error) (throw error))
                   nil))]
    (if joined
      (filterv (comp child-subject? first) joined)
      (let [subjects (filterv child-subject?
                              (mapv first
                                    (rows-of-width
                                     (:rows (north.coord/bounded-query
                                             port (child-subject-query coordinator) max-rows))
                                     1 "child subject row")))]
        (into [] (mapcat #(rows-of-width (subject-show-facts port %) 3 "child fact row"))
              subjects)))))

(defn- run-subjects [port agent-ids]
  (if (empty? agent-ids)
    []
    (let [tagged (try
                   (rows-of-width (both-domains port tagged-run-query) 2 "tagged run row")
                   (catch Exception error
                     (when-not (row-limit? error) (throw error))
                     nil))]
      (if tagged
        (mapv first (filterv (comp agent-ids second) tagged))
        (into []
              (mapcat (fn [agent-id]
                        (mapv first
                              (rows-of-width (both-domains port (child-run-query agent-id))
                                             1 "tagged run row"))))
              (sort agent-ids))))))

(defn- projection [rows]
  (mapv (fn [[subject predicate value]]
          {:subject (short-id subject) :predicate predicate :value value})
        (sort (distinct (mapv (fn [[subject predicate value]]
                                [(short-id subject) predicate value])
                              rows)))))

(defn settlement
  [port coordinator]
  (let [children (child-facts port coordinator)
        agent-ids (set (mapv #(subs (first %) (count child-prefix)) children))
        runs (run-subjects port agent-ids)
        run-rows (into [] (mapcat #(rows-of-width (subject-show-facts port %) 3 "run fact row"))
                       (sort (distinct runs)))]
    (array-map :protocol protocol
               :version envelope-version
               :coordinator coordinator
               :children (projection children)
               :runs (projection run-rows))))

(defn- refuse! [coordinator error]
  (binding [*out* *err*]
    (println
     (str "north: json child-settlement REFUSED — coordinator unavailable for "
          coordinator
          (when-let [detail (some-> error .getMessage not-empty)]
            (str ": " detail)))))
  (System/exit 4))

(defn -main [& args]
  (if (= 1 (count args))
    (try
      (println (json/generate-string (settlement (coordination-port) (first args))))
      (catch Exception error
        (refuse! (first args) error)))
    (do
      (binding [*out* *err*]
        (println "usage: json child-settlement <coordinator>"))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
