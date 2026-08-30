(ns north.architecture-cli
  (:require [cheshire.core :as json]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [clojure.walk :as walk])
  (:import [java.math BigInteger]
           [java.security MessageDigest]))

(def script-file (.getCanonicalPath (io/file *file*)))

(def cli-dir (.getParent (io/file script-file)))

(def ^String repo-root (.getCanonicalPath (io/file cli-dir "..")))

(def ^String inventory-path (str repo-root "/architecture/system.edn"))

(def ^String catalog-sources-path (str repo-root "/agent-catalog/sources.json"))

(def ^String document-path (str repo-root "/docs/system-map.md"))

(def ^String sidecar-path (str document-path ".doc.edn"))

(def ^String schema-id "north.system-architecture/v1")

(def required-view-ids #{"ownership" "activation" "coordination" "migration"})

(def allowed-directions #{"LR" "RL" "TB" "BT"})

(def allowed-layers #{"source-authority" "selection" "live-runtime" "data-authority" "projection" "transitional" "foreign-runtime" "runtime-state"})

(def allowed-states #{"authoritative" "selected-generation" "runtime-observed" "derived" "migration-incomplete" "external"})

(def allowed-relation-kinds #{"activates" "admits" "composes" "configures" "coordinates" "generates" "hosts" "launches" "migrates-to" "owns" "persists" "projects" "records" "references" "replays" "requires" "selects"})

(def generation-source-paths ["architecture/system.edn" "cli/architecture-cli.bclj" "agent-catalog/sources.json"])

(defn- fail! [^String message data]
  (throw (ex-info message data)))

(defn- ^Boolean require! [condition ^String message data]
  (if condition true (fail! message data)))

(defn- ^Boolean nonblank-string? [value]
  (and (string? value) (not (str/blank? value))))

(defn- ^Boolean valid-id? [value]
  (and (nonblank-string? value) (some? (re-matches #"[a-z0-9][a-z0-9-]*" value))))

(defn- validate-id! [value ^String context]
  (do
  (require! (valid-id? value) (str context " must be a lowercase hyphenated id") {:context context :value value})
  value))

(defn- validate-string-vector! [value ^String context]
  (do
  (require! (and (vector? value) (seq value)) (str context " must be a non-empty vector") {:context context :value value})
  (doseq [item value]
  (require! (nonblank-string? item) (str context " entries must be nonblank strings") {:context context :value item}))
  value))

(defn- validate-evidence! [value ^String context]
  (do
  (validate-string-vector! value context)
  (doseq [item value]
  (require! (or (some? (re-matches #"[a-z0-9-]+:.+" item)) (str/starts-with? item "/")) (str context " entries must be repo:path or absolute paths") {:context context :value item}))
  value))

(defn- validate-catalog-source! [source]
  (do
  (require! (map? source) "catalog source must be a map" {:value source})
  (validate-id! (:id source) "catalog source id")
  (require! (contains? #{"source" "package" "operator"} (:role source)) "catalog source role is invalid" {:source source})
  (require! (map? (:owner source)) "catalog source owner must be a map" {:source source})
  (validate-id! (get-in source [:owner :repo]) "catalog source owner repo")
  (require! (nonblank-string? (get-in source [:owner :path])) "catalog source owner path must be nonblank" {:source source})
  source))

(defn- validate-view! [view]
  (do
  (require! (map? view) "architecture view must be a map" {:value view})
  (validate-id! (:id view) "view id")
  (require! (nonblank-string? (:title view)) "view title must be nonblank" {:view view})
  (require! (contains? allowed-directions (:direction view)) "view direction is invalid" {:view view})
  (require! (nonblank-string? (:purpose view)) "view purpose must be nonblank" {:view view})
  view))

(defn- validate-component! [component view-ids]
  (do
  (require! (map? component) "architecture component must be a map" {:value component})
  (validate-id! (:id component) "component id")
  (require! (nonblank-string? (:label component)) "component label must be nonblank" {:component component})
  (require! (contains? allowed-layers (:layer component)) "component layer is invalid" {:component component})
  (require! (contains? allowed-states (:state component)) "component state is invalid" {:component component})
  (require! (nonblank-string? (:owner component)) "component owner must be nonblank" {:component component})
  (validate-string-vector! (:responsibilities component) "component responsibilities")
  (validate-evidence! (:evidence component) "component evidence")
  (require! (nonblank-string? (:failure-effect component)) "component failure effect must be nonblank" {:component component})
  (validate-string-vector! (:views component) "component views")
  (require! (every? view-ids (:views component)) "component references an unknown view" {:component component})
  component))

(defn- validate-relation! [relation component-ids components-by-id view-ids]
  (do
  (require! (map? relation) "architecture relation must be a map" {:value relation})
  (validate-id! (:id relation) "relation id")
  (require! (contains? component-ids (:from relation)) "relation source is unknown" {:relation relation})
  (require! (contains? component-ids (:to relation)) "relation target is unknown" {:relation relation})
  (require! (not= (:from relation) (:to relation)) "relation cannot be a self edge" {:relation relation})
  (require! (contains? allowed-relation-kinds (:kind relation)) "relation kind is invalid" {:relation relation})
  (require! (nonblank-string? (:label relation)) "relation label must be nonblank" {:relation relation})
  (validate-evidence! (:evidence relation) "relation evidence")
  (require! (nonblank-string? (:failure-effect relation)) "relation failure effect must be nonblank" {:relation relation})
  (validate-string-vector! (:views relation) "relation views")
  (require! (every? view-ids (:views relation)) "relation references an unknown view" {:relation relation})
  (doseq [view-id (:views relation)]
  (require! (and (some #{view-id} (:views (get components-by-id (:from relation)))) (some #{view-id} (:views (get components-by-id (:to relation))))) "relation view must include both endpoint components" {:relation (:id relation) :view view-id}))
  relation))

(defn validate-inventory! [inventory]
  (do
  (require! (map? inventory) "architecture inventory must be an EDN map" {})
  (require! (= schema-id (:schema inventory)) "architecture schema is unsupported" {:actual (:schema inventory)})
  (require! (nonblank-string? (:title inventory)) "architecture title must be nonblank" {})
  (require! (nonblank-string? (:summary inventory)) "architecture summary must be nonblank" {})
  (require! (vector? (:catalog-sources inventory)) "catalog-sources must be a vector" {})
  (doseq [source (:catalog-sources inventory)]
  (validate-catalog-source! source))
  (require! (vector? (:views inventory)) "views must be a vector" {})
  (doseq [view (:views inventory)]
  (validate-view! view))
  (let [view-ids (set (map (fn [view] (:id view)) (:views inventory)))]
  (require! (= required-view-ids view-ids) "architecture must define exactly ownership, activation, coordination, and migration views" {:actual view-ids})
  (require! (= (count view-ids) (count (:views inventory))) "architecture view ids must be unique" {})
  (require! (and (vector? (:components inventory)) (seq (:components inventory))) "components must be a non-empty vector" {})
  (doseq [component (:components inventory)]
  (validate-component! component view-ids))
  (let [component-ids (set (map (fn [component] (:id component)) (:components inventory)))
   components-by-id (into {} (map (fn [component] [(:id component) component]) (:components inventory)))]
  (require! (= (count component-ids) (count (:components inventory))) "component ids must be unique" {})
  (require! (and (vector? (:relations inventory)) (seq (:relations inventory))) "relations must be a non-empty vector" {})
  (doseq [relation (:relations inventory)]
  (validate-relation! relation component-ids components-by-id view-ids))
  (require! (= (count (set (map (fn [relation] (:id relation)) (:relations inventory)))) (count (:relations inventory))) "relation ids must be unique" {})))
  (let [^String printed (pr-str inventory)]
  (doseq [forbidden [":generated-at" ":timestamp" ":verified-at"]]
  (require! (not (str/includes? printed forbidden)) "architecture authority must not contain timestamps" {:forbidden forbidden})))
  inventory))

(defn- read-edn-file! [^String path]
  (try
  (edn/read-string (slurp path))
  (catch Exception error
    (fail! (str "cannot read EDN " path ": " (.getMessage error)) {:path path}))))

(defn load-inventory! []
  (validate-inventory! (read-edn-file! inventory-path)))

(defn- actual-catalog-sources! []
  (try
  (:sources (json/parse-string (slurp catalog-sources-path) true))
  (catch Exception error
    (fail! (str "cannot read agent catalog sources: " (.getMessage error)) {:path catalog-sources-path}))))

(defn validate-catalog-agreement! [inventory]
  (let [expected (:catalog-sources inventory)
   actual (actual-catalog-sources!)]
  (require! (= expected actual) "architecture catalog sources disagree with agent-catalog/sources.json" {:expected expected :actual actual})
  inventory))

(defn- canonicalize [value]
  (walk/postwalk (fn [item] (if (map? item) (into (sorted-map) item) item)) value))

(defn ^String render-json [inventory]
  (str (json/generate-string (canonicalize inventory) {:pretty true}) "\n"))

(defn- ^String display-token [^String value]
  (str/replace value "-" " "))

(defn- ^String safe-mermaid-id [^String value]
  (str "n_" (str/replace value #"[^A-Za-z0-9_]" "_")))

(defn- ^String escape-mermaid [^String value]
  (-> value (str/replace "\\" "\\\\") (str/replace "\"" "&quot;") (str/replace "|" "/") (str/replace "\n" " ")))

(defn- ^String component-node-line [component]
  (str "  " (safe-mermaid-id (:id component)) "[\"" (escape-mermaid (:label component)) "<br/><small>" (display-token (:layer component)) " · " (display-token (:state component)) "</small>\"]:::" (str/replace (:layer component) "-" "_")))

(defn- ^String relation-line [relation]
  (str "  " (safe-mermaid-id (:from relation)) " -->|" (escape-mermaid (:label relation)) "| " (safe-mermaid-id (:to relation))))

(def mermaid-class-lines ["  classDef source_authority fill:#e8f0ff,stroke:#3559a8,color:#102044" "  classDef selection fill:#fff2cc,stroke:#9b7a00,color:#3f3100" "  classDef live_runtime fill:#d9ead3,stroke:#38761d,color:#15330c" "  classDef data_authority fill:#d0e0e3,stroke:#134f5c,color:#073038" "  classDef projection fill:#f3f3f3,stroke:#666,color:#222" "  classDef transitional fill:#fce5cd,stroke:#b45f06,color:#4c2500,stroke-dasharray: 5 5" "  classDef foreign_runtime fill:#eadcf8,stroke:#674ea7,color:#271b45" "  classDef runtime_state fill:#eeeeee,stroke:#555,color:#222"])

(defn ^String render-mermaid! [inventory ^String view-id]
  (let [view (first (filter (fn [%1] (= view-id (:id %1))) (:views inventory)))]
  (require! (some? view) "unknown architecture view" {:view view-id})
  (let [components (filter (fn [%1] (some #{view-id} (:views %1))) (:components inventory))
   relations (filter (fn [%1] (some #{view-id} (:views %1))) (:relations inventory))]
  (str (str/join "\n" (concat [(str "flowchart " (:direction view)) (str "  %% " (escape-mermaid (:purpose view)))] (map component-node-line components) [""] (map relation-line relations) [""] mermaid-class-lines)) "\n"))))

(defn- ^String markdown-cell [^String value]
  (-> value (str/replace "|" "\\|") (str/replace "\n" " ")))

(defn- ^String evidence-cell [evidence]
  (str/join "<br/>" (map (fn [%1] (str "`" (markdown-cell %1) "`")) evidence)))

(defn- ^String component-row [component]
  (str "| `" (:id component) "` | " (markdown-cell (:label component)) " | " (display-token (:layer component)) " | " (display-token (:state component)) " | " (markdown-cell (:owner component)) " | " (markdown-cell (str/join " " (:responsibilities component))) " | " (markdown-cell (:failure-effect component)) " | " (evidence-cell (:evidence component)) " |"))

(defn- ^String relation-row [relation]
  (str "| `" (:id relation) "` | `" (:from relation) "` → `" (:to relation) "` | " (markdown-cell (:label relation)) " | " (str/join ", " (:views relation)) " | " (markdown-cell (:failure-effect relation)) " | " (evidence-cell (:evidence relation)) " |"))

(defn ^String render-markdown! [inventory]
  (str (str/join "\n" (concat [(str "# " (:title inventory)) "" "Generated from `north:architecture/system.edn` by the typed `north:cli/architecture-cli.bclj` authority. Do not edit this file directly." "" (:summary inventory) "" "Source authority states what should exist. A selected generation states what activation resolved. A live runtime states what an actual process loaded. None is proof of either other layer; inspect runtime identity separately when diagnosing skew." "" "Git remains authority for code and specifications. Store is authority only for coordination referents and events. `threads/` is a derived projection, while todo and handoff files remain transitional recovery state until their work is linked to canonical North identities." "" "## Legend" "" "| Layer | Meaning |" "|---|---|" "| source authority | The repository that owns semantics and policy. |" "| selection | A generation or runtime chosen for one activation. |" "| live runtime | The process actually executing; inspect it rather than inferring it. |" "| data authority | Canonical durable coordination state. |" "| projection | Regenerable human or tool view. |" "| transitional | Recovery state or an explicit migration gap. |" "| foreign runtime | Provider-owned execution boundary. |" "| runtime state | Local or provider state that is not source authority. |"] (mapcat (fn [view] [(str "") (str "## " (:title view)) "" (:purpose view) "" "```mermaid" (str/replace (render-mermaid! inventory (:id view)) #"\n$" "") "```"]) (:views inventory)) ["" "## Component inventory" "" "| ID | Component | Layer | State | Owner | Responsibility | Failure effect | Evidence |" "|---|---|---|---|---|---|---|---|"] (map component-row (:components inventory)) ["" "## Relationship inventory" "" "| ID | Edge | Meaning | Views | Failure effect | Evidence |" "|---|---|---|---|---|---|"] (map relation-row (:relations inventory)))) "\n"))

(defn- ^String sha256-file [^String path]
  (let [bytes (.getBytes (slurp path) "UTF-8")
   digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
  (format "%064x" (BigInteger. 1 digest))))

(defn sidecar-value []
  (canonicalize {:kind :generated :sources (mapv (fn [^String path] {:path path :revision "content"}) generation-source-paths) :refresh-policy :on-change :source-digests (into (sorted-map) (map (fn [^String path] [path (sha256-file (str repo-root "/" path))]) generation-source-paths))}))

(defn ^String render-sidecar []
  (str (pr-str (sidecar-value)) "\n"))

(defn expected-artifacts! [inventory]
  {:document (render-markdown! inventory) :sidecar (render-sidecar)})

(defn generate! [inventory]
  (let [expected (expected-artifacts! inventory)]
  (spit document-path (:document expected))
  (spit sidecar-path (:sidecar expected))
  {:document document-path :sidecar sidecar-path}))

(defn check-generated! [inventory]
  (let [expected (expected-artifacts! inventory)]
  (require! (.isFile (io/file document-path)) "generated architecture document is missing" {:path document-path})
  (require! (.isFile (io/file sidecar-path)) "generated architecture sidecar is missing" {:path sidecar-path})
  (require! (= (:document expected) (slurp document-path)) "generated architecture document is stale; run north-architecture generate" {:path document-path})
  (require! (= (:sidecar expected) (slurp sidecar-path)) "generated architecture sidecar is stale; run north-architecture generate" {:path sidecar-path})
  true))

(defn- print-overview! [inventory]
  (do
  (println (:title inventory))
  (println (str "Canonical source: north:architecture/system.edn (" (count (:components inventory)) " components, " (count (:relations inventory)) " relationships)"))
  (println)
  (println (:summary inventory))
  (println)
  (println "Identity boundary: source authority != selected generation != actual live process.")
  (println "State boundary: Git owns code/specifications; Store owns coordination; files are projections or transitional recovery state.")
  (println)
  (doseq [view (:views inventory)]
  (println (format "  %-13s %s" (:id view) (:purpose view))))
  (println)
  (println "Use --mermaid <view>, --json, explain <component>, generate, or check.")))

(defn- explain-component! [inventory ^String component-id]
  (let [component (first (filter (fn [%1] (= component-id (:id %1))) (:components inventory)))]
  (require! (some? component) "unknown architecture component" {:component component-id})
  (println (str (:label component) " [" (:id component) "]"))
  (println (str "  layer: " (:layer component) " / " (:state component)))
  (println (str "  owner: " (:owner component)))
  (doseq [responsibility (:responsibilities component)]
  (println (str "  owns: " responsibility)))
  (println (str "  failure: " (:failure-effect component)))
  (println (str "  evidence: " (str/join ", " (:evidence component))))))

(defn usage []
  (do
  (println "usage: north-architecture [explain [COMPONENT] | --json | --mermaid VIEW | generate | check]")
  (println "")
  (println "  explain [COMPONENT]  summarize the system or explain one component")
  (println "  --json               print the validated canonical inventory")
  (println "  --mermaid VIEW       print ownership, activation, coordination, or migration")
  (println "  generate             regenerate docs/system-map.md and its docctl sidecar")
  (println "  check                validate inventory, catalog agreement, and generated freshness")))

(defn run-cli! [args]
  (cond
  (or (= args ["--help"]) (= args ["help"])) (usage)
  :else (let [inventory (validate-catalog-agreement! (load-inventory!))]
  (cond
  (empty? args) (print-overview! inventory)
  (= args ["explain"]) (print-overview! inventory)
  (and (= 2 (count args)) (= "explain" (first args))) (explain-component! inventory (second args))
  (= args ["--json"]) (print (render-json inventory))
  (and (= 2 (count args)) (= "--mermaid" (first args))) (print (render-mermaid! inventory (second args)))
  (= args ["generate"]) (let [written (generate! inventory)]
  (println (str "generated " (:document written)))
  (println (str "generated " (:sidecar written))))
  (= args ["check"]) (do
  (check-generated! inventory)
  (println (str "architecture check: passed (" (count (:components inventory)) " components, " (count (:relations inventory)) " relationships)")))
  :else (fail! "invalid arguments; run north-architecture --help" {:args args})))))

(defn- ^Boolean direct-invocation? []
  (let [bb-file (System/getProperty "babashka.file")]
  (and (string? bb-file) (= script-file (.getCanonicalPath (io/file bb-file))))))

(if (direct-invocation?) (try
  (run-cli! (vec *command-line-args*))
  (catch Exception error
    (do
  (binding [*out* *err*]
  (println (str "north architecture: " (.getMessage error))))
  (System/exit 2)))))
