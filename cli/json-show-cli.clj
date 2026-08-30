(ns north.json-show-cli
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]))

(def script-file (.getCanonicalPath (io/file *file*)))

(def cli-dir (.getParent (io/file script-file)))

(load-file (str cli-dir "/coord.clj"))

(defrecord JsonFact [predicate value])

(defn jsonfact-predicate [r] (:predicate r))

(defn jsonfact-value [r] (:value r))

(defn- coordination-port []
  (let [value (parse-long (or (System/getenv "NORTH_PORT") "7977"))]
  (if (int? value) value (throw (ex-info "NORTH_PORT is malformed" {:type :invalid-port})))))

(defn- ^String string-term [value]
  (if (string? value) value (throw (ex-info "Store RPC subject projection returned a non-string term" {:type :malformed-subject-projection}))))

(defn- ^JsonFact json-fact [row]
  (if (and (vector? row) (= 2 (count row))) (->JsonFact (string-term (nth row 0)) (string-term (nth row 1))) (throw (ex-info "Store RPC subject projection returned a malformed row" {:type :malformed-subject-projection}))))

(defn exact-facts! [port ^String id]
  (let [envelope (north.coord/show-envelope! port (str "@" id))
   rows (:rows envelope)]
  (if (vector? rows) (mapv (fn [row] (json-fact row)) rows) (throw (ex-info "Store RPC subject projection omitted its rows" {:type :malformed-subject-projection})))))

(defn- refuse! [^String id error]
  (binding [*out* *err*]
  (println (str "north: json show REFUSED — coordinator unavailable for @" id (let [detail (.getMessage error)]
  (if (and (string? detail) (not (empty? detail))) (str ": " detail) "")))))
  (System/exit 4))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (if (= 1 (count args)) (try
  (println (json/generate-string (exact-facts! (coordination-port) (first args))))
  (catch Exception error
    (refuse! (first args) error))) (do
  (binding [*out* *err*]
  (println "usage: json show <id>"))
  (System/exit 2)))))

(defn- ^Boolean direct-invocation? []
  (= script-file (.getCanonicalPath (io/file (System/getProperty "babashka.file")))))

(if (direct-invocation?) (do
  (apply -main *command-line-args*)))
