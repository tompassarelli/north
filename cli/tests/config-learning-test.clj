#!/usr/bin/env bb
;; Isolated cross-language round-trip and atomicity checks for the learning dial.
(require '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def scratch (.toFile (java.nio.file.Files/createTempDirectory
                       "north-learning-config-"
                       (make-array java.nio.file.attribute.FileAttribute 0))))
(def policy (.getPath (io/file scratch "learning-policy.json")))
(def checks (atom []))

(defn check! [label pass?]
  (swap! checks conj [label (boolean pass?)]))

(defn run! [& args]
  (apply process/shell
         {:out :string :err :string :continue true
          :extra-env {"NORTH_LEARNING_POLICY" policy}}
         (into ["bb" cli "learning"] args)))

(defn policy-data [] (json/parse-string (slurp policy) true))

(try
  (let [shown (run!)]
    (check! "missing policy shows frozen measured default"
            (and (zero? (:exit shown))
                 (str/includes? (:out shown) "learning: frozen · discovery")
                 (str/includes? (:out shown) "at most one eligible axis"))))

  (doseq [args [["mode" "learning"]
                ["intensity" "0.25"]
                ["axes" "model-tier" "effort" "prompt"]
                ["max-tier-delta" "2"]
                ["risk-ceiling" "p2"]
                ["seed" "ordinary-ops"]
                ["epoch" "2026-08"]
                ["evidence-mode" "evaluation"]]]
    (let [result (apply run! args)]
      (check! (str "command succeeds: " (str/join " " args))
              (zero? (:exit result)))))

  (let [value (policy-data)]
    (check! "all configured fields persist in the versioned document"
            (= {:version 1 :mode "learning" :intensity 0.25
                :axes ["model-tier" "effort" "prompt"]
                :maxTierDelta 2 :riskCeiling "p2" :seed "ordinary-ops"
                :epoch "2026-08" :evidenceMode "evaluation"}
               value)))

  (let [script (str "import { loadLearningPolicy } from '" root
                    "/sdk/src/learning-regime.ts';"
                    "const p=loadLearningPolicy();"
                    "if(p.mode!=='learning'||p.intensity!==0.25||p.maxTierDelta!==2"
                    "||p.axes.join(',')!=='model-tier,effort,prompt'"
                    "||p.riskCeiling!=='p2'||p.evidenceMode!=='evaluation')process.exit(9);")
        result (process/shell
                {:out :string :err :string :continue true
                 :extra-env {"NORTH_LEARNING_POLICY" policy}}
                "bun" "-e" script)]
    (check! "Clojure output is accepted by the TypeScript canonical loader"
            (zero? (:exit result))))

  (let [before (slurp policy)]
    (doseq [[label args]
            [["intensity above one" ["intensity" "1.01"]]
             ["duplicate axes" ["axes" "prompt" "prompt"]]
             ["unknown axis" ["axes" "provider"]]
             ["tier delta out of bounds" ["max-tier-delta" "4"]]
             ["bad seed" ["seed" "not portable"]]
             ["bad evidence mode" ["evidence-mode" "mixed"]]]]
      (let [result (apply run! args)]
        (check! (str label " is rejected") (not (zero? (:exit result))))
        (check! (str label " leaves policy bytes unchanged") (= before (slurp policy)))))
    (check! "atomic updates leave no staging files"
            (empty? (filter #(str/starts-with? (.getName %) ".learning-policy.")
                            (.listFiles scratch)))))

  (spit policy "{not-json\n")
  (let [result (run!)]
    (check! "malformed persisted policy fails closed"
            (and (not (zero? (:exit result)))
                 (str/includes? (:err result) "invalid learning policy")))
    (check! "failed read never replaces malformed operator state"
            (= "{not-json\n" (slurp policy))))

  (finally
    (doseq [file (reverse (file-seq scratch))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label pass?] results]
    (println (format "  [%s] %s" (if pass? "PASS" "FAIL") label)))
  (println (format "\nconfig learning: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
