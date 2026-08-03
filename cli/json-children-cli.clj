#!/usr/bin/env bb
;; Exact indexed reverse lookup for one parent's direct children.
;;
;; Same carve-out as `json show`: this pre-side-effect read stays on the
;; coordinator's index, because composing the whole live corpus through
;; north.main to keep one parent's `part_of` rows does not fit the SDK's 45s
;; child read budget (sdk/src/north-client.ts).
(require '[cheshire.core :as json]
         '[clojure.string :as str]
         '[clojure.java.io :as io])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def indexed-query (ns-resolve 'north.coord 'indexed-query))

;; The coordinator's per-query row ceiling. A parent with more direct children
;; than this REFUSES loudly rather than answering with a truncated child set —
;; a short list here reads as "those children settled" to the caller.
(def max-children 4096)

(defn- coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

;; Value-position literal, so route-for-operation still routes this to
;; coordination: `part_of` is a coordination-owned lifecycle predicate, exactly
;; like the curated ready/board projections partitioned-main already keeps
;; coordination-only.
(defn- children-query [parent]
  {:find "north_child"
   :rules [{:head {:rel "north_child" :args [{:var "subject"}]}
            :body [{:rel "triple"
                    :args [{:var "subject"} "part_of" (str "@" parent)]}]}]})

(defn- short-id [subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn children
  [port parent]
  (let [rows (:ok (indexed-query port (children-query parent) max-children))]
    (when-not (every? #(and (= 1 (count %)) (not (str/blank? (first %)))) rows)
      (throw (ex-info "coordinator returned a malformed child row"
                      {:type :malformed-child-row})))
    (vec (sort (distinct (mapv (comp short-id first) rows))))))

(defn- refuse! [id error]
  (binding [*out* *err*]
    (println
     (str "north: json children REFUSED — coordinator unavailable for @"
          id
          (when-let [detail (some-> error .getMessage not-empty)]
            (str ": " detail)))))
  (System/exit 4))

(defn -main [& args]
  (if (= 1 (count args))
    (try
      (println (json/generate-string (children (coordination-port) (first args))))
      (catch Exception error
        (refuse! (first args) error)))
    (do
      (binding [*out* *err*]
        (println "usage: json children <parent>"))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
