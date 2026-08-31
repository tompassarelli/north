(ns beagle.user
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json]
            [babashka.process :as p]))

(def port (or (some-> (first *command-line-args*) Integer/parseInt) 7977))

(def ^String root (or (second *command-line-args*) (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../.." "agent-machinery"))))

(def ^String runtime (or (nth *command-line-args* 2 nil) (System/getenv "NORTH_AGENT_RUNTIME_HOME") (str (or (System/getenv "NORTH_HOME") (System/getProperty "user.dir")) "/agent-runtime/orchestration")))

(def ^String cli-dir (.getParent (io/file (System/getProperty "babashka.file"))))

(def ^String project-cli (str (io/file (.getParentFile (io/file cli-dir)) "orchestration-project-cli.clj")))

(defn canon
  "Order- and knob-independent normal form: recursively sort object keys, sort\n   every array by its canonical string, drop $schema, and rename the deliberation\n   knob to the graph's canonical spelling." [x]
  (cond
  (sequential? x) (->> x (map canon) (sort-by json/generate-string) vec)
  (map? x) (->> (dissoc x "$schema") (map (fn [[k v]] [(case k
    "efforts" "reasoning"
    "defaultEffort" "defaultReasoning"
    k) (canon v)])) (into (sorted-map)))
  :else x))

(defn project [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [{:keys [exit out err]} (apply p/sh "bb" project-cli (str port) args)]
  (if (not (zero? exit)) (do
  (throw (ex-info (str "projector failed: " err) {}))))
  (json/parse-string out))))

(def results (atom []))

(defn check! [^String label graph-json file-json]
  (let [g (canon graph-json)
   f (canon file-json)]
  (if (= g f) (do
  (swap! results conj true)
  (println (format "  ✓ %s byte-parity (normalized)" label))) (do
  (swap! results conj false)
  (println (format "  ✗ %s DIVERGES" label))
  (doseq [k (sort (distinct (concat (keys g) (keys f))))]
  (if (not= (get g k) (get f k)) (do
  (println (format "      key %s: graph=%.180s" k (pr-str (get g k))))
  (println (format "               file=%.180s" (pr-str (get f k)))))))))))

(println (format "orchestration parity gate — port %d, root %s" port root))

(check! "delegation/catalog.json" (project "staffing") (json/parse-string (slurp (io/file root "delegation" "catalog.json"))))

(doseq [prov ["anthropic" "openai"]]
  (check! (str "providers/" prov ".json") (project "provider" prov) (json/parse-string (slurp (io/file runtime "providers" (str prov ".json"))))))

(let [rs (deref results)]
  (println (format "\n%d/%d parity checks passed" (count (filter true? rs)) (count rs)))
  (System/exit (if (every? true? rs) 0 1)))
