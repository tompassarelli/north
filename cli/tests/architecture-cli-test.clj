#!/usr/bin/env bb
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[babashka.process :as process])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/architecture-cli.clj"))

(def architecture-ns (find-ns 'north.architecture-cli))
(defn architecture-var [name]
  (or (ns-resolve architecture-ns name) (throw (ex-info (str name) {}))))
(defn architecture-call [name & args]
  (apply (architecture-var name) args))

(def checks (atom []))
(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)])
  (println (str (if ok? "ok   " "FAIL ") label)))
(defn throws? [f]
  (try (f) false (catch Exception _ true)))

(let [inventory (architecture-call 'load-inventory!)]
  (check "catalog source composition agrees exactly"
         (= inventory (architecture-call 'validate-catalog-agreement! inventory)))
  (check "inventory exposes exactly four canonical views"
         (= #{"ownership" "activation" "coordination" "migration"}
            (set (map :id (:views inventory)))))
  (check "duplicate component ids are rejected"
         (throws? #(architecture-call
                    'validate-inventory!
                    (update inventory :components conj (first (:components inventory))))))
  (check "unknown relation endpoints are rejected"
         (throws? #(architecture-call
                    'validate-inventory!
                    (assoc-in inventory [:relations 0 :to] "missing-component"))))
  (check "missing failure effects are rejected"
         (throws? #(architecture-call
                    'validate-inventory!
                    (assoc-in inventory [:components 0 :failure-effect] ""))))
  (check "missing evidence is rejected"
         (throws? #(architecture-call
                    'validate-inventory!
                    (assoc-in inventory [:relations 0 :evidence] []))))
  (doseq [view-id ["ownership" "activation" "coordination" "migration"]]
    (let [diagram (architecture-call 'render-mermaid! inventory view-id)]
      (check (str view-id " Mermaid is a flowchart")
             (str/starts-with? diagram "flowchart "))))
  (let [activation (architecture-call 'render-mermaid! inventory "activation")]
    (check "activation distinguishes source, selection, and actual runtime"
           (every? #(str/includes? activation %)
                   ["Beagle Store source" "Selected Store runtime" "Actual Store service process"])))
  (let [migration (architecture-call 'render-mermaid! inventory "migration")]
    (check "migration names projection and transitional recovery state"
           (every? #(str/includes? migration %)
                   ["threads/ file projection" "Todo and handoff files" "North coordination graph in Store"])))
  (check "unknown Mermaid views are rejected"
         (throws? #(architecture-call 'render-mermaid! inventory "unknown")))
  (check "committed generated artifacts match deterministic rendering"
         (true? (architecture-call 'check-generated! inventory)))
  (let [result (process/shell {:out :string :err :string
                               :env (assoc (into {} (System/getenv))
                                           "NORTH_PORT" "unreachable"
                                           "NORTH_STORE_HOME" "/definitely/not/a/store")}
                              (str root "/bin/north-architecture") "--json")
        parsed (json/parse-string (:out result) true)]
    (check "standalone JSON succeeds without Store runtime"
           (and (zero? (:exit result))
                (= "north.system-architecture/v1" (:schema parsed))))))

(let [failed (filter (comp not second) @checks)]
  (if (seq failed)
    (do
      (println (str "architecture-cli: " (count failed) " failure(s)"))
      (System/exit 1))
    (println (str "architecture-cli: passed " (count @checks) " checks"))))
