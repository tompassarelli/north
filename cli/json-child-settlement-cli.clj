(ns north.json-child-settlement-cli
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def script-file (.getCanonicalPath (io/file *file*)))

(def cli-dir (.getParent (io/file script-file)))

(load-file (str cli-dir "/coord.clj"))

(def max-rows 4096)

(def ^String protocol "north.child-settlement")

(def envelope-version 1)

(def ^String child-prefix "@agent:")

(defrecord SettlementFact [subject predicate value])

(defn settlementfact-subject [r] (:subject r))

(defn settlementfact-predicate [r] (:predicate r))

(defn settlementfact-value [r] (:value r))

(defrecord SettlementEnvelope [protocol version coordinator children runs])

(defn settlementenvelope-protocol [r] (:protocol r))

(defn settlementenvelope-version [r] (:version r))

(defn settlementenvelope-coordinator [r] (:coordinator r))

(defn settlementenvelope-children [r] (:children r))

(defn settlementenvelope-runs [r] (:runs r))

(defn- coordination-port []
  (let [value (parse-long (or (System/getenv "NORTH_PORT") "7977"))]
  (if (int? value) value (throw (ex-info "NORTH_PORT is malformed" {:type :invalid-port})))))

(defn- child-subject-query [^String coordinator]
  {:find "north_child" :rules [{:head {:rel "north_child" :args [{:var "subject"}]} :body [{:rel "triple" :args [{:var "subject"} "coordinator" coordinator]}]}]})

(defn- child-fact-query [^String coordinator]
  {:find "north_child_fact" :rules [{:head {:rel "north_child_fact" :args [{:var "subject"} {:var "predicate"} {:var "value"}]} :body [{:rel "triple" :args [{:var "subject"} "coordinator" coordinator]} {:rel "triple" :args [{:var "subject"} {:var "predicate"} {:var "value"}]}]}]})

(def tagged-run-query {:find "north_child_run" :rules [{:head {:rel "north_child_run" :args [{:var "subject"} {:var "agent"}]} :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]} {:rel "triple" :args [{:var "subject"} "agent" {:var "agent"}]}]}]})

(defn- child-run-query [^String agent-id]
  {:find "north_child_run" :rules [{:head {:rel "north_child_run" :args [{:var "subject"}]} :body [{:rel "triple" :args [{:var "subject"} "kind" "run"]} {:rel "triple" :args [{:var "subject"} "agent" agent-id]}]}]})

(defn- ^String short-id [^String subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn- malformed! [^String detail]
  (throw (ex-info (str "Store RPC server returned a malformed " detail) {:type :malformed-settlement-row})))

(defn- ^String string-term! [value ^String detail]
  (if (string? value) value (malformed! detail)))

(defn- row-of-width! [row width ^String detail]
  (if (and (vector? row) (= width (count row))) (let [decoded (mapv (fn [value] (string-term! value detail)) row)]
  (if (str/blank? (first decoded)) (malformed! detail) decoded)) (malformed! detail)))

(defn- rows-of-width! [rows width ^String detail]
  (if (vector? rows) (mapv (fn [row] (row-of-width! row width detail)) rows) (malformed! detail)))

(defn- ^Boolean row-limit? [error]
  (= :query-row-limit (:type (ex-data error))))

(defn- both-domains! [port query]
  (let [coordination (north.coord/bounded-query-in-domain! port :coordination query max-rows)
   telemetry (north.coord/bounded-query-in-domain! port :telemetry query max-rows)]
  (vec (distinct (concat (:rows coordination) (:rows telemetry))))))

(defn- ^Boolean child-subject? [^String subject]
  (str/starts-with? subject child-prefix))

(defn- subject-show-facts! [port ^String subject]
  (let [envelope (north.coord/show-envelope! port subject)
   rows (rows-of-width! (:rows envelope) 2 "subject fact row")]
  (mapv (fn [row] [subject (nth row 0) (nth row 1)]) rows)))

(defn- child-facts! [port ^String coordinator]
  (let [joined (try
  (let [response (north.coord/bounded-query! port (child-fact-query coordinator) max-rows)]
  (rows-of-width! (:rows response) 3 "child fact row"))
  (catch Exception error
    (if (row-limit? error) nil (throw error))))]
  (if (some? joined) (filterv (fn [row] (child-subject? (first row))) joined) (let [response (north.coord/bounded-query! port (child-subject-query coordinator) max-rows)
   subject-rows (rows-of-width! (:rows response) 1 "child subject row")
   subjects (filterv child-subject? (mapv first subject-rows))]
  (into [] (mapcat (fn [^String subject] (subject-show-facts! port subject)) subjects))))))

(defn- run-subjects! [port agent-ids]
  (if (empty? agent-ids) [] (let [tagged (try
  (rows-of-width! (both-domains! port tagged-run-query) 2 "tagged run row")
  (catch Exception error
    (if (row-limit? error) nil (throw error))))]
  (if (some? tagged) (mapv first (filterv (fn [row] (contains? agent-ids (nth row 1))) tagged)) (into [] (mapcat (fn [^String agent-id] (mapv first (rows-of-width! (both-domains! port (child-run-query agent-id)) 1 "tagged run row"))) (sort (vec agent-ids))))))))

(defn- projection [rows]
  (let [normalized (mapv (fn [row] [(short-id (nth row 0)) (nth row 1) (nth row 2)]) rows)]
  (mapv (fn [row] (->SettlementFact (nth row 0) (nth row 1) (nth row 2))) (vec (sort (distinct normalized))))))

(defn ^SettlementEnvelope settlement! [port ^String coordinator]
  (let [children (child-facts! port coordinator)
   agent-ids (set (mapv (fn [row] (subs (first row) (count child-prefix))) children))
   runs (run-subjects! port agent-ids)
   run-rows (into [] (mapcat (fn [^String subject] (subject-show-facts! port subject)) (sort (distinct runs))))]
  (->SettlementEnvelope protocol envelope-version coordinator (projection children) (projection run-rows))))

(defn- refuse! [^String coordinator error]
  (binding [*out* *err*]
  (println (str "north: json child-settlement REFUSED — coordinator unavailable for " coordinator (let [detail (.getMessage error)]
  (if (and (string? detail) (not (empty? detail))) (str ": " detail) "")))))
  (System/exit 4))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (if (= 1 (count args)) (try
  (println (json/generate-string (settlement! (coordination-port) (first args))))
  (catch Exception error
    (refuse! (first args) error))) (do
  (binding [*out* *err*]
  (println "usage: json child-settlement <coordinator>"))
  (System/exit 2)))))

(defn- ^Boolean direct-invocation? []
  (= script-file (.getCanonicalPath (io/file (System/getProperty "babashka.file")))))

(if (direct-invocation?) (do
  (apply -main *command-line-args*)))
