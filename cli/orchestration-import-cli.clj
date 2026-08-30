(ns beagle.user
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json]
            [store.rpc-limits :as rpc-limits]))

^{:line 39 :file "cli/orchestration-import-cli.bclj"} (def CLI-DIR ^{:line 39 :file "cli/orchestration-import-cli.bclj"} (.getParent ^{:line 39 :file "cli/orchestration-import-cli.bclj"} (io/file *file*)))

^{:line 40 :file "cli/orchestration-import-cli.bclj"} (load-file ^{:line 40 :file "cli/orchestration-import-cli.bclj"} (str CLI-DIR "/coord.clj"))

^{:line 41 :file "cli/orchestration-import-cli.bclj"} (load-file ^{:line 41 :file "cli/orchestration-import-cli.bclj"} (str CLI-DIR "/orchestration-selection.clj"))

^{:line 42 :file "cli/orchestration-import-cli.bclj"} (def enumerate-selection-rules north.orchestration-selection/enumerate-selection-rules)

^{:line 44 :file "cli/orchestration-import-cli.bclj"} (def rules-digest north.orchestration-selection/rules-digest)

^{:line 54 :file "cli/orchestration-import-cli.bclj"} (def this-root ^{:line 54 :file "cli/orchestration-import-cli.bclj"} (.getParent ^{:line 54 :file "cli/orchestration-import-cli.bclj"} (io/file CLI-DIR)))

^{:line 56 :file "cli/orchestration-import-cli.bclj"} (defn ^String orchestration-home [arg]
  ^{:line 57 :file "cli/orchestration-import-cli.bclj"} (or arg ^{:line 58 :file "cli/orchestration-import-cli.bclj"} (System/getenv "AGENT_MACHINERY_HOME") ^{:line 59 :file "cli/orchestration-import-cli.bclj"} (str ^{:line 59 :file "cli/orchestration-import-cli.bclj"} (System/getenv "HOME") "/code/agent-machinery/main")))

^{:line 61 :file "cli/orchestration-import-cli.bclj"} (defn ^String agent-runtime-home []
  ^{:line 62 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 62 :file "cli/orchestration-import-cli.bclj"} (System/getenv "NORTH_AGENT_RUNTIME_HOME") ^{:line 63 :file "cli/orchestration-import-cli.bclj"} (str ^{:line 63 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 63 :file "cli/orchestration-import-cli.bclj"} (System/getenv "NORTH_HOME") this-root ^{:line 63 :file "cli/orchestration-import-cli.bclj"} (System/getProperty "user.dir")) "/agent-runtime/orchestration")))

^{:line 66 :file "cli/orchestration-import-cli.bclj"} (defn read-json [^String root & $beagle$rest$host]
  (let [segs (vec $beagle$rest$host)]
  ^{:line 69 :file "cli/orchestration-import-cli.bclj"} (json/parse-string ^{:line 69 :file "cli/orchestration-import-cli.bclj"} (slurp ^{:line 69 :file "cli/orchestration-import-cli.bclj"} (apply io/file root segs)))))

^{:line 76 :file "cli/orchestration-import-cli.bclj"} (defn extract-section-fence [^String text ^String heading]
  ^{:line 79 :file "cli/orchestration-import-cli.bclj"} (let [lines ^{:line 79 :file "cli/orchestration-import-cli.bclj"} (str/split text #"\n" -1)
   want ^{:line 80 :file "cli/orchestration-import-cli.bclj"} (str "## " ^{:line 80 :file "cli/orchestration-import-cli.bclj"} (str/lower-case heading))
   start ^{:line 81 :file "cli/orchestration-import-cli.bclj"} (some ^{:line 81 :file "cli/orchestration-import-cli.bclj"} (fn [[i l]] ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (= want ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (str/lower-case ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (str/trim l))) ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (inc ^{:line 82 :file "cli/orchestration-import-cli.bclj"} (int i))))) ^{:line 83 :file "cli/orchestration-import-cli.bclj"} (map-indexed vector lines))]
  ^{:line 84 :file "cli/orchestration-import-cli.bclj"} (if start ^{:line 84 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 85 :file "cli/orchestration-import-cli.bclj"} (loop [i start
   open nil]
  ^{:line 87 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 87 :file "cli/orchestration-import-cli.bclj"} (< i ^{:line 87 :file "cli/orchestration-import-cli.bclj"} (count lines)) ^{:line 87 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 88 :file "cli/orchestration-import-cli.bclj"} (let [t ^{:line 88 :file "cli/orchestration-import-cli.bclj"} (str/trim ^{:line 88 :file "cli/orchestration-import-cli.bclj"} (nth lines i))]
  ^{:line 89 :file "cli/orchestration-import-cli.bclj"} (cond
  ^{:line 90 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 90 :file "cli/orchestration-import-cli.bclj"} (nil? open) ^{:line 90 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? t "## ")) nil
  ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (nil? open) ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? t "```")) ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (recur ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (inc i) ^{:line 91 :file "cli/orchestration-import-cli.bclj"} (inc i))
  ^{:line 92 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 92 :file "cli/orchestration-import-cli.bclj"} (some? open) ^{:line 92 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? t "```")) ^{:line 92 :file "cli/orchestration-import-cli.bclj"} (str/join "\n" ^{:line 92 :file "cli/orchestration-import-cli.bclj"} (subvec lines open i))
  :else ^{:line 93 :file "cli/orchestration-import-cli.bclj"} (recur ^{:line 93 :file "cli/orchestration-import-cli.bclj"} (inc i) open))))))))))

^{:line 95 :file "cli/orchestration-import-cli.bclj"} (defn extract-first-fence [^String text]
  ^{:line 96 :file "cli/orchestration-import-cli.bclj"} (let [lines ^{:line 96 :file "cli/orchestration-import-cli.bclj"} (str/split text #"\n" -1)]
  ^{:line 97 :file "cli/orchestration-import-cli.bclj"} (loop [i 0
   open nil]
  ^{:line 99 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 99 :file "cli/orchestration-import-cli.bclj"} (< i ^{:line 99 :file "cli/orchestration-import-cli.bclj"} (count lines)) ^{:line 99 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 100 :file "cli/orchestration-import-cli.bclj"} (let [t ^{:line 100 :file "cli/orchestration-import-cli.bclj"} (str/trim ^{:line 100 :file "cli/orchestration-import-cli.bclj"} (nth lines i))]
  ^{:line 101 :file "cli/orchestration-import-cli.bclj"} (cond
  ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (nil? open) ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? t "```")) ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (recur ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (inc i) ^{:line 102 :file "cli/orchestration-import-cli.bclj"} (inc i))
  ^{:line 103 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 103 :file "cli/orchestration-import-cli.bclj"} (some? open) ^{:line 103 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? t "```")) ^{:line 103 :file "cli/orchestration-import-cli.bclj"} (str/join "\n" ^{:line 103 :file "cli/orchestration-import-cli.bclj"} (subvec lines open i))
  :else ^{:line 104 :file "cli/orchestration-import-cli.bclj"} (recur ^{:line 104 :file "cli/orchestration-import-cli.bclj"} (inc i) open))))))))

^{:line 106 :file "cli/orchestration-import-cli.bclj"} (defn ^String section-fence [^String root ^String doc ^String heading]
  ^{:line 110 :file "cli/orchestration-import-cli.bclj"} (let [text ^{:line 110 :file "cli/orchestration-import-cli.bclj"} (slurp ^{:line 110 :file "cli/orchestration-import-cli.bclj"} (io/file root "docs" doc))
   block ^{:line 111 :file "cli/orchestration-import-cli.bclj"} (extract-section-fence text heading)]
  ^{:line 112 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 112 :file "cli/orchestration-import-cli.bclj"} (and block ^{:line 112 :file "cli/orchestration-import-cli.bclj"} (seq ^{:line 112 :file "cli/orchestration-import-cli.bclj"} (str/trim block))) block ^{:line 114 :file "cli/orchestration-import-cli.bclj"} (throw ^{:line 114 :file "cli/orchestration-import-cli.bclj"} (ex-info ^{:line 114 :file "cli/orchestration-import-cli.bclj"} (str "no fenced block: " doc " ## " heading) ^{:line 114 :file "cli/orchestration-import-cli.bclj"} {})))))

^{:line 121 :file "cli/orchestration-import-cli.bclj"} (defn selection-signal-values []
  ^{:line 122 :file "cli/orchestration-import-cli.bclj"} {"decisionOwnership" ^{:line 122 :file "cli/orchestration-import-cli.bclj"} ["none" "bounded" "cross-boundary" "system-shaping" "open-solution-class"] "seamScope" ^{:line 123 :file "cli/orchestration-import-cli.bclj"} ["none" "established" "consequential" "system-wide"] "errorExposure" ^{:line 124 :file "cli/orchestration-import-cli.bclj"} ["contained-reversible" "material-recoverable" "high-or-hard-to-reverse"] "oracleStrength" ^{:line 125 :file "cli/orchestration-import-cli.bclj"} ["not-applicable" "objective-local" "objective-end-to-end" "partial" "judgment-only"] "foundationalImpact" ^{:line 126 :file "cli/orchestration-import-cli.bclj"} ["none" "implementation-only" "invariant-decision-owned"] "dependencyShape" ^{:line 127 :file "cli/orchestration-import-cli.bclj"} ["atomic-cohesive" "deterministic-workflow" "parallel-breadth" "dynamic-decomposition" "tightly-coupled-sequential"] "reasoningShape" ^{:line 128 :file "cli/orchestration-import-cli.bclj"} ["deterministic" "bounded-branching" "multi-hypothesis" "system-synthesis" "exceptional"]})

^{:line 133 :file "cli/orchestration-import-cli.bclj"} (def ^String POINTER "@catalog:current")

^{:line 135 :file "cli/orchestration-import-cli.bclj"} (defn exact-values! [port ^String subject ^String predicate]
  ^{:line 139 :file "cli/orchestration-import-cli.bclj"} (->> ^{:line 139 :file "cli/orchestration-import-cli.bclj"} (north.coord/query-rows! port ^{:line 141 :file "cli/orchestration-import-cli.bclj"} {:find "v" :rules ^{:line 141 :file "cli/orchestration-import-cli.bclj"} [^{:line 141 :file "cli/orchestration-import-cli.bclj"} {:head ^{:line 141 :file "cli/orchestration-import-cli.bclj"} {:rel "v" :args ^{:line 141 :file "cli/orchestration-import-cli.bclj"} [^{:line 141 :file "cli/orchestration-import-cli.bclj"} {:var "v"}]} :body ^{:line 142 :file "cli/orchestration-import-cli.bclj"} [^{:line 142 :file "cli/orchestration-import-cli.bclj"} {:rel "triple" :args ^{:line 142 :file "cli/orchestration-import-cli.bclj"} [subject predicate ^{:line 142 :file "cli/orchestration-import-cli.bclj"} {:var "v"}]}]}]}) ^{:line 143 :file "cli/orchestration-import-cli.bclj"} (map first)))

^{:line 145 :file "cli/orchestration-import-cli.bclj"} (defn current-version! [port]
  ^{:line 146 :file "cli/orchestration-import-cli.bclj"} (some-> ^{:line 146 :file "cli/orchestration-import-cli.bclj"} (first ^{:line 146 :file "cli/orchestration-import-cli.bclj"} (exact-values! port POINTER "catalog_version")) parse-long))

^{:line 148 :file "cli/orchestration-import-cli.bclj"} (defn ^String ns-subject [ver & $beagle$rest$host]
  (let [parts (vec $beagle$rest$host)]
  ^{:line 151 :file "cli/orchestration-import-cli.bclj"} (str "@catalog:v" ver ":" ^{:line 151 :file "cli/orchestration-import-cli.bclj"} (str/join ":" parts))))

^{:line 155 :file "cli/orchestration-import-cli.bclj"} (defn parse-version [arg]
  ^{:line 156 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 156 :file "cli/orchestration-import-cli.bclj"} (some? arg) ^{:line 156 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 157 :file "cli/orchestration-import-cli.bclj"} (let [bind__1 ^{:line 157 :file "cli/orchestration-import-cli.bclj"} (re-matches #"v?(\d+)" ^{:line 157 :file "cli/orchestration-import-cli.bclj"} (str arg))]
  ^{:line 157 :file "cli/orchestration-import-cli.bclj"} (if bind__1 ^{:line 157 :file "cli/orchestration-import-cli.bclj"} (let [[_ n] bind__1]
  ^{:line 158 :file "cli/orchestration-import-cli.bclj"} (parse-long n)) ^{:line 159 :file "cli/orchestration-import-cli.bclj"} (throw ^{:line 159 :file "cli/orchestration-import-cli.bclj"} (ex-info ^{:line 159 :file "cli/orchestration-import-cli.bclj"} (str "bad catalog version " ^{:line 159 :file "cli/orchestration-import-cli.bclj"} (pr-str arg) " — expected N or vN") ^{:line 160 :file "cli/orchestration-import-cli.bclj"} {:type :catalog-bad-version :arg arg})))))))

^{:line 162 :file "cli/orchestration-import-cli.bclj"} (defn version-arg! [port arg]
  ^{:line 165 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 165 :file "cli/orchestration-import-cli.bclj"} (parse-version arg) ^{:line 166 :file "cli/orchestration-import-cli.bclj"} (current-version! port) ^{:line 167 :file "cli/orchestration-import-cli.bclj"} (throw ^{:line 167 :file "cli/orchestration-import-cli.bclj"} (ex-info "no @catalog:current pointer — import first" ^{:line 167 :file "cli/orchestration-import-cli.bclj"} {}))))

^{:line 169 :file "cli/orchestration-import-cli.bclj"} (defn publish-actions! [port actions]
  ^{:line 172 :file "cli/orchestration-import-cli.bclj"} (let [result ^{:line 172 :file "cli/orchestration-import-cli.bclj"} (north.coord/publish! port ^{:line 172 :file "cli/orchestration-import-cli.bclj"} (vec actions))]
  ^{:line 173 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 173 :file "cli/orchestration-import-cli.bclj"} (:reject result) ^{:line 173 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 174 :file "cli/orchestration-import-cli.bclj"} (throw ^{:line 174 :file "cli/orchestration-import-cli.bclj"} (ex-info "Store RPC rejected delegation composition catalog publication" ^{:line 175 :file "cli/orchestration-import-cli.bclj"} {:type :catalog-publication-rejected :result result}))))
  result))

^{:line 178 :file "cli/orchestration-import-cli.bclj"} (defn staging-batches [actions]
  ^{:line 179 :file "cli/orchestration-import-cli.bclj"} (loop [remaining ^{:line 179 :file "cli/orchestration-import-cli.bclj"} (seq actions)
   batch ^{:line 180 :file "cli/orchestration-import-cli.bclj"} []
   batch-cost 0
   batches ^{:line 182 :file "cli/orchestration-import-cli.bclj"} []]
  ^{:line 183 :file "cli/orchestration-import-cli.bclj"} (let [action ^{:line 183 :file "cli/orchestration-import-cli.bclj"} (first remaining)]
  ^{:line 183 :file "cli/orchestration-import-cli.bclj"} (if action ^{:line 184 :file "cli/orchestration-import-cli.bclj"} (let [cost ^{:line 184 :file "cli/orchestration-import-cli.bclj"} (max 1 ^{:line 184 :file "cli/orchestration-import-cli.bclj"} (count ^{:line 184 :file "cli/orchestration-import-cli.bclj"} (:values action)))]
  ^{:line 185 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 185 :file "cli/orchestration-import-cli.bclj"} (> cost rpc-limits/rpc-v2-max-batch-actions) ^{:line 185 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 186 :file "cli/orchestration-import-cli.bclj"} (throw ^{:line 186 :file "cli/orchestration-import-cli.bclj"} (ex-info "one catalog staging action exceeds the Store mutation bound" ^{:line 187 :file "cli/orchestration-import-cli.bclj"} {:type :catalog-staging-action-too-large :cost cost :limit rpc-limits/rpc-v2-max-batch-actions :action action}))))
  ^{:line 191 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 191 :file "cli/orchestration-import-cli.bclj"} (and ^{:line 191 :file "cli/orchestration-import-cli.bclj"} (seq batch) ^{:line 192 :file "cli/orchestration-import-cli.bclj"} (> ^{:line 192 :file "cli/orchestration-import-cli.bclj"} (+ batch-cost cost) rpc-limits/rpc-v2-max-batch-actions)) ^{:line 193 :file "cli/orchestration-import-cli.bclj"} (recur remaining ^{:line 193 :file "cli/orchestration-import-cli.bclj"} [] 0 ^{:line 193 :file "cli/orchestration-import-cli.bclj"} (conj batches batch)) ^{:line 194 :file "cli/orchestration-import-cli.bclj"} (recur ^{:line 194 :file "cli/orchestration-import-cli.bclj"} (next remaining) ^{:line 194 :file "cli/orchestration-import-cli.bclj"} (conj batch action) ^{:line 194 :file "cli/orchestration-import-cli.bclj"} (+ batch-cost cost) batches))) ^{:line 195 :file "cli/orchestration-import-cli.bclj"} (cond-> batches ^{:line 195 :file "cli/orchestration-import-cli.bclj"} (seq batch) ^{:line 195 :file "cli/orchestration-import-cli.bclj"} (conj batch))))))

^{:line 197 :file "cli/orchestration-import-cli.bclj"} (defn publish-staging! [port actions]
  ^{:line 202 :file "cli/orchestration-import-cli.bclj"} (doseq [batch ^{:line 202 :file "cli/orchestration-import-cli.bclj"} (staging-batches actions)]
  ^{:line 203 :file "cli/orchestration-import-cli.bclj"} (publish-actions! port batch)))

^{:line 205 :file "cli/orchestration-import-cli.bclj"} (defn flip! [port ver]
  ^{:line 208 :file "cli/orchestration-import-cli.bclj"} (publish-actions! port ^{:line 210 :file "cli/orchestration-import-cli.bclj"} [^{:line 210 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject "@catalog_version" :predicate "cardinality" :values ^{:line 211 :file "cli/orchestration-import-cli.bclj"} ["single"] :cardinality :one} ^{:line 212 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject POINTER :predicate "catalog_version" :values ^{:line 213 :file "cli/orchestration-import-cli.bclj"} [^{:line 213 :file "cli/orchestration-import-cli.bclj"} (str ver)] :cardinality :one}]))

^{:line 218 :file "cli/orchestration-import-cli.bclj"} (def ^:dynamic *publication-actions* nil)

^{:line 220 :file "cli/orchestration-import-cli.bclj"} (defn queue-set! [port ^String subject ^String predicate values cardinality]
  ^{:line 226 :file "cli/orchestration-import-cli.bclj"} (let [encoded-values ^{:line 226 :file "cli/orchestration-import-cli.bclj"} (vec ^{:line 226 :file "cli/orchestration-import-cli.bclj"} (map str values))]
  ^{:line 227 :file "cli/orchestration-import-cli.bclj"} (if *publication-actions* ^{:line 228 :file "cli/orchestration-import-cli.bclj"} (swap! *publication-actions* update ^{:line 229 :file "cli/orchestration-import-cli.bclj"} [subject predicate cardinality] ^{:line 230 :file "cli/orchestration-import-cli.bclj"} (fn [current] ^{:line 231 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject subject :predicate predicate :values ^{:line 232 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 232 :file "cli/orchestration-import-cli.bclj"} (= :one cardinality) encoded-values ^{:line 234 :file "cli/orchestration-import-cli.bclj"} (vec ^{:line 234 :file "cli/orchestration-import-cli.bclj"} (distinct ^{:line 234 :file "cli/orchestration-import-cli.bclj"} (concat ^{:line 234 :file "cli/orchestration-import-cli.bclj"} (:values current) encoded-values)))) :cardinality cardinality})) ^{:line 236 :file "cli/orchestration-import-cli.bclj"} (publish-actions! port ^{:line 238 :file "cli/orchestration-import-cli.bclj"} [^{:line 238 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject subject :predicate predicate :values encoded-values :cardinality cardinality}]))))

^{:line 241 :file "cli/orchestration-import-cli.bclj"} (defn s1! [port ^String subj ^String p v]
  ^{:line 246 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 246 :file "cli/orchestration-import-cli.bclj"} (some? v) ^{:line 246 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 247 :file "cli/orchestration-import-cli.bclj"} (queue-set! port subj p ^{:line 247 :file "cli/orchestration-import-cli.bclj"} [v] :one))))

^{:line 249 :file "cli/orchestration-import-cli.bclj"} (defn smulti! [port ^String subj ^String p vs]
  ^{:line 254 :file "cli/orchestration-import-cli.bclj"} (queue-set! port subj p vs :many))

^{:line 256 :file "cli/orchestration-import-cli.bclj"} (defn ^String emit-staffing! [port ver catalog]
  ^{:line 260 :file "cli/orchestration-import-cli.bclj"} (let [subj ^{:line 260 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "staffing")
   d ^{:line 261 :file "cli/orchestration-import-cli.bclj"} (get catalog "defaults")]
  ^{:line 262 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "staffing_catalog")
  ^{:line 263 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "catalog_version" ^{:line 263 :file "cli/orchestration-import-cli.bclj"} (get catalog "version"))
  ^{:line 264 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "default_task_grade" ^{:line 264 :file "cli/orchestration-import-cli.bclj"} (get d "taskGrade"))
  ^{:line 265 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "default_tier" ^{:line 265 :file "cli/orchestration-import-cli.bclj"} (get d "tier"))
  ^{:line 266 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "default_reasoning" ^{:line 266 :file "cli/orchestration-import-cli.bclj"} (get d "deliberation"))
  ^{:line 267 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "default_topology" ^{:line 267 :file "cli/orchestration-import-cli.bclj"} (get d "topology"))
  ^{:line 268 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "default_posture" ^{:line 268 :file "cli/orchestration-import-cli.bclj"} (get d "posture"))
  subj))

^{:line 272 :file "cli/orchestration-import-cli.bclj"} (def AXIS-DOC ^{:line 273 :file "cli/orchestration-import-cli.bclj"} {"taskGrades" ^{:line 273 :file "cli/orchestration-import-cli.bclj"} {:axis "task_grade" :doc "task-grades.md"} "topologies" ^{:line 274 :file "cli/orchestration-import-cli.bclj"} {:axis "topology" :doc "topologies.md"} "postures" ^{:line 275 :file "cli/orchestration-import-cli.bclj"} {:axis "posture" :doc "postures.md"}})

^{:line 277 :file "cli/orchestration-import-cli.bclj"} (def AXIS-PLAIN ^{:line 278 :file "cli/orchestration-import-cli.bclj"} {"semanticTiers" "tier" "deliberations" "reasoning" "capabilities" "capability"})

^{:line 282 :file "cli/orchestration-import-cli.bclj"} (defn emit-axis-values! [port ver ^String root catalog]
  ^{:line 287 :file "cli/orchestration-import-cli.bclj"} (let [vocab ^{:line 287 :file "cli/orchestration-import-cli.bclj"} (get catalog "vocabulary")]
  ^{:line 288 :file "cli/orchestration-import-cli.bclj"} (doseq [[vkey {:keys [axis doc]}] AXIS-DOC
   [rank v] ^{:line 289 :file "cli/orchestration-import-cli.bclj"} (map-indexed vector ^{:line 289 :file "cli/orchestration-import-cli.bclj"} (get vocab vkey))]
  ^{:line 290 :file "cli/orchestration-import-cli.bclj"} (let [subj ^{:line 290 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "axis" axis v)]
  ^{:line 291 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "axis_value")
  ^{:line 292 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "axis" axis)
  ^{:line 293 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "rank" rank)
  ^{:line 294 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "prompt_block" ^{:line 294 :file "cli/orchestration-import-cli.bclj"} (section-fence root doc v))
  ^{:line 295 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "doctrine_source" ^{:line 295 :file "cli/orchestration-import-cli.bclj"} (str "docs/" doc "#" v))))
  ^{:line 296 :file "cli/orchestration-import-cli.bclj"} (doseq [[vkey axis] AXIS-PLAIN
   [rank v] ^{:line 297 :file "cli/orchestration-import-cli.bclj"} (map-indexed vector ^{:line 297 :file "cli/orchestration-import-cli.bclj"} (get vocab vkey))]
  ^{:line 298 :file "cli/orchestration-import-cli.bclj"} (let [subj ^{:line 298 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "axis" axis v)]
  ^{:line 299 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "axis_value")
  ^{:line 300 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "axis" axis)
  ^{:line 301 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "rank" rank)))))

^{:line 303 :file "cli/orchestration-import-cli.bclj"} (defn emit-comms! [port ver ^String root]
  ^{:line 307 :file "cli/orchestration-import-cli.bclj"} (let [subj ^{:line 307 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "comms" "universal")]
  ^{:line 308 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "doctrine_block")
  ^{:line 309 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "prompt_block" ^{:line 309 :file "cli/orchestration-import-cli.bclj"} (section-fence root "comms.md" "universal"))
  ^{:line 310 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "doctrine_source" "docs/comms.md#universal")))

^{:line 312 :file "cli/orchestration-import-cli.bclj"} (defn emit-templates! [port ver ^String root catalog]
  ^{:line 317 :file "cli/orchestration-import-cli.bclj"} (doseq [preset ^{:line 317 :file "cli/orchestration-import-cli.bclj"} (get catalog "presets")]
  ^{:line 318 :file "cli/orchestration-import-cli.bclj"} (let [name ^{:line 318 :file "cli/orchestration-import-cli.bclj"} (get preset "name")
   subj ^{:line 319 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "template" name)]
  ^{:line 320 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "template")
  ^{:line 321 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "task_grade" ^{:line 321 :file "cli/orchestration-import-cli.bclj"} (get preset "taskGrade"))
  ^{:line 322 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "topology" ^{:line 322 :file "cli/orchestration-import-cli.bclj"} (get preset "topology"))
  ^{:line 323 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "tier" ^{:line 323 :file "cli/orchestration-import-cli.bclj"} (get preset "tier"))
  ^{:line 324 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "reasoning" ^{:line 324 :file "cli/orchestration-import-cli.bclj"} (get preset "deliberation"))
  ^{:line 325 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "posture" ^{:line 325 :file "cli/orchestration-import-cli.bclj"} (get preset "posture"))
  ^{:line 326 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "tagline" ^{:line 326 :file "cli/orchestration-import-cli.bclj"} (get preset "tagline"))
  ^{:line 327 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "doc" ^{:line 327 :file "cli/orchestration-import-cli.bclj"} (get preset "description"))
  ^{:line 328 :file "cli/orchestration-import-cli.bclj"} (smulti! port subj "capability" ^{:line 328 :file "cli/orchestration-import-cli.bclj"} (get preset "capabilities"))
  ^{:line 329 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "prompt_block" ^{:line 329 :file "cli/orchestration-import-cli.bclj"} (section-fence root "roles.md" name))
  ^{:line 330 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "doctrine_source" ^{:line 330 :file "cli/orchestration-import-cli.bclj"} (str "docs/roles.md#" name)))))

^{:line 334 :file "cli/orchestration-import-cli.bclj"} (defn emit-provider! [port ver ^String _root ^String provider]
  ^{:line 339 :file "cli/orchestration-import-cli.bclj"} (let [runtime ^{:line 339 :file "cli/orchestration-import-cli.bclj"} (agent-runtime-home)
   cat ^{:line 340 :file "cli/orchestration-import-cli.bclj"} (read-json runtime "providers" ^{:line 340 :file "cli/orchestration-import-cli.bclj"} (str provider ".json"))
   psubj ^{:line 341 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "provider" provider)
   prov ^{:line 342 :file "cli/orchestration-import-cli.bclj"} (get cat "provenance")]
  ^{:line 343 :file "cli/orchestration-import-cli.bclj"} (s1! port psubj "kind" "provider_catalog")
  ^{:line 344 :file "cli/orchestration-import-cli.bclj"} (s1! port psubj "as_of" ^{:line 344 :file "cli/orchestration-import-cli.bclj"} (get prov "asOf"))
  ^{:line 345 :file "cli/orchestration-import-cli.bclj"} (s1! port psubj "review_after" ^{:line 345 :file "cli/orchestration-import-cli.bclj"} (get prov "reviewAfter"))
  ^{:line 346 :file "cli/orchestration-import-cli.bclj"} (smulti! port psubj "transport" ^{:line 346 :file "cli/orchestration-import-cli.bclj"} (get cat "transports"))
  ^{:line 347 :file "cli/orchestration-import-cli.bclj"} (smulti! port psubj "provenance_source" ^{:line 348 :file "cli/orchestration-import-cli.bclj"} (map json/generate-string ^{:line 348 :file "cli/orchestration-import-cli.bclj"} (get prov "sources")))
  ^{:line 350 :file "cli/orchestration-import-cli.bclj"} (let [aliases ^{:line 350 :file "cli/orchestration-import-cli.bclj"} (get cat "modelAliases")
   alias-of ^{:line 351 :file "cli/orchestration-import-cli.bclj"} (reduce ^{:line 351 :file "cli/orchestration-import-cli.bclj"} (fn [m [a model]] ^{:line 354 :file "cli/orchestration-import-cli.bclj"} (update m model ^{:line 354 :file "cli/orchestration-import-cli.bclj"} (fnil conj ^{:line 354 :file "cli/orchestration-import-cli.bclj"} []) a)) ^{:line 355 :file "cli/orchestration-import-cli.bclj"} {} aliases)
   deltas ^{:line 357 :file "cli/orchestration-import-cli.bclj"} (get cat "modelDeltas")]
  ^{:line 358 :file "cli/orchestration-import-cli.bclj"} (doseq [[model spec] ^{:line 358 :file "cli/orchestration-import-cli.bclj"} (get cat "models")]
  ^{:line 359 :file "cli/orchestration-import-cli.bclj"} (let [msubj ^{:line 359 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "model" provider model)
   levels ^{:line 360 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 360 :file "cli/orchestration-import-cli.bclj"} (get spec "efforts") ^{:line 360 :file "cli/orchestration-import-cli.bclj"} (get spec "reasoning"))
   routes ^{:line 361 :file "cli/orchestration-import-cli.bclj"} (get spec "routes")
   cw ^{:line 362 :file "cli/orchestration-import-cli.bclj"} (get spec "contextWindow")
   delta ^{:line 363 :file "cli/orchestration-import-cli.bclj"} (get deltas model)]
  ^{:line 364 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "kind" "model")
  ^{:line 365 :file "cli/orchestration-import-cli.bclj"} (smulti! port msubj "alias" ^{:line 365 :file "cli/orchestration-import-cli.bclj"} (sort ^{:line 365 :file "cli/orchestration-import-cli.bclj"} (get alias-of model)))
  ^{:line 366 :file "cli/orchestration-import-cli.bclj"} (smulti! port msubj "deliberation_support" levels)
  ^{:line 367 :file "cli/orchestration-import-cli.bclj"} (smulti! port msubj "calibrated_route" ^{:line 368 :file "cli/orchestration-import-cli.bclj"} (for [[tier ls] routes
   l ls]
  ^{:line 370 :file "cli/orchestration-import-cli.bclj"} (str tier "/" l)))
  ^{:line 371 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "context_window_tokens" ^{:line 371 :file "cli/orchestration-import-cli.bclj"} (get cw "tokens"))
  ^{:line 372 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "context_window_from" ^{:line 372 :file "cli/orchestration-import-cli.bclj"} (get cw "effectiveFrom"))
  ^{:line 373 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "delta_kind" ^{:line 373 :file "cli/orchestration-import-cli.bclj"} (get delta "kind"))
  ^{:line 374 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 374 :file "cli/orchestration-import-cli.bclj"} (= "none" ^{:line 374 :file "cli/orchestration-import-cli.bclj"} (get delta "kind")) ^{:line 374 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 375 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "delta_reason" ^{:line 375 :file "cli/orchestration-import-cli.bclj"} (get delta "reason"))))
  ^{:line 376 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 376 :file "cli/orchestration-import-cli.bclj"} (= "calibrated" ^{:line 376 :file "cli/orchestration-import-cli.bclj"} (get delta "kind")) ^{:line 376 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 377 :file "cli/orchestration-import-cli.bclj"} (let [path ^{:line 377 :file "cli/orchestration-import-cli.bclj"} (get delta "path")]
  ^{:line 378 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "doctrine_source" path)
  ^{:line 379 :file "cli/orchestration-import-cli.bclj"} (s1! port msubj "prompt_block" ^{:line 380 :file "cli/orchestration-import-cli.bclj"} (extract-first-fence ^{:line 380 :file "cli/orchestration-import-cli.bclj"} (slurp ^{:line 380 :file "cli/orchestration-import-cli.bclj"} (io/file runtime path))))))))))
  ^{:line 383 :file "cli/orchestration-import-cli.bclj"} (doseq [[tier spec] ^{:line 383 :file "cli/orchestration-import-cli.bclj"} (get cat "tiers")]
  ^{:line 384 :file "cli/orchestration-import-cli.bclj"} (let [tsubj ^{:line 384 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "tier-row" provider tier)]
  ^{:line 385 :file "cli/orchestration-import-cli.bclj"} (s1! port tsubj "kind" "tier_row")
  ^{:line 386 :file "cli/orchestration-import-cli.bclj"} (s1! port tsubj "tier" tier)
  ^{:line 387 :file "cli/orchestration-import-cli.bclj"} (s1! port tsubj "model" ^{:line 387 :file "cli/orchestration-import-cli.bclj"} (get spec "model"))
  ^{:line 388 :file "cli/orchestration-import-cli.bclj"} (smulti! port tsubj "level" ^{:line 388 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 388 :file "cli/orchestration-import-cli.bclj"} (get spec "efforts") ^{:line 388 :file "cli/orchestration-import-cli.bclj"} (get spec "reasoning")))
  ^{:line 389 :file "cli/orchestration-import-cli.bclj"} (s1! port tsubj "default_level" ^{:line 389 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 389 :file "cli/orchestration-import-cli.bclj"} (get spec "defaultEffort") ^{:line 389 :file "cli/orchestration-import-cli.bclj"} (get spec "defaultReasoning")))))))

^{:line 391 :file "cli/orchestration-import-cli.bclj"} (defn emit-selection! [port ver ^String root]
  ^{:line 395 :file "cli/orchestration-import-cli.bclj"} (let [signals ^{:line 395 :file "cli/orchestration-import-cli.bclj"} (selection-signal-values)
   rules ^{:line 396 :file "cli/orchestration-import-cli.bclj"} (enumerate-selection-rules root)
   policy ^{:line 397 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "selection-policy" "minimum-sufficient-v1")]
  ^{:line 398 :file "cli/orchestration-import-cli.bclj"} (doseq [[sig vals] signals]
  ^{:line 399 :file "cli/orchestration-import-cli.bclj"} (let [subj ^{:line 399 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "signal" sig)]
  ^{:line 400 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "selection_signal")
  ^{:line 401 :file "cli/orchestration-import-cli.bclj"} (smulti! port subj "one_of" vals)))
  ^{:line 402 :file "cli/orchestration-import-cli.bclj"} (s1! port policy "kind" "selection_policy")
  ^{:line 403 :file "cli/orchestration-import-cli.bclj"} (doseq [r rules]
  ^{:line 404 :file "cli/orchestration-import-cli.bclj"} (let [code ^{:line 404 :file "cli/orchestration-import-cli.bclj"} (get r "rule_code")
   subj ^{:line 405 :file "cli/orchestration-import-cli.bclj"} (ns-subject ver "rule" code)]
  ^{:line 406 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "kind" "selection_rule")
  ^{:line 407 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "signal" ^{:line 407 :file "cli/orchestration-import-cli.bclj"} (get r "signal"))
  ^{:line 408 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "signal_value" ^{:line 408 :file "cli/orchestration-import-cli.bclj"} (get r "signal_value"))
  ^{:line 409 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "min_tier" ^{:line 409 :file "cli/orchestration-import-cli.bclj"} (get r "min_tier"))
  ^{:line 410 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "min_reasoning" ^{:line 410 :file "cli/orchestration-import-cli.bclj"} (get r "min_reasoning"))
  ^{:line 411 :file "cli/orchestration-import-cli.bclj"} (s1! port subj "rule_code" code)
  ^{:line 412 :file "cli/orchestration-import-cli.bclj"} (smulti! port policy "rule" ^{:line 412 :file "cli/orchestration-import-cli.bclj"} [subj])))
  ^{:line 415 :file "cli/orchestration-import-cli.bclj"} (s1! port policy "policy_sha256" ^{:line 415 :file "cli/orchestration-import-cli.bclj"} (rules-digest rules))))

^{:line 420 :file "cli/orchestration-import-cli.bclj"} (defn import! [port ^String root]
  ^{:line 423 :file "cli/orchestration-import-cli.bclj"} (let [ver ^{:line 423 :file "cli/orchestration-import-cli.bclj"} (inc ^{:line 423 :file "cli/orchestration-import-cli.bclj"} (int ^{:line 423 :file "cli/orchestration-import-cli.bclj"} (or ^{:line 423 :file "cli/orchestration-import-cli.bclj"} (current-version! port) 0)))
   catalog ^{:line 424 :file "cli/orchestration-import-cli.bclj"} (read-json root "delegation" "catalog.json")
   actions ^{:line 425 :file "cli/orchestration-import-cli.bclj"} (atom ^{:line 425 :file "cli/orchestration-import-cli.bclj"} {})]
  ^{:line 426 :file "cli/orchestration-import-cli.bclj"} (binding [*publication-actions* actions]
  ^{:line 427 :file "cli/orchestration-import-cli.bclj"} (emit-staffing! port ver catalog)
  ^{:line 428 :file "cli/orchestration-import-cli.bclj"} (emit-axis-values! port ver root catalog)
  ^{:line 429 :file "cli/orchestration-import-cli.bclj"} (emit-comms! port ver root)
  ^{:line 430 :file "cli/orchestration-import-cli.bclj"} (emit-templates! port ver root catalog)
  ^{:line 431 :file "cli/orchestration-import-cli.bclj"} (emit-provider! port ver root "anthropic")
  ^{:line 432 :file "cli/orchestration-import-cli.bclj"} (emit-provider! port ver root "openai")
  ^{:line 433 :file "cli/orchestration-import-cli.bclj"} (emit-selection! port ver root))
  ^{:line 434 :file "cli/orchestration-import-cli.bclj"} (publish-staging! port ^{:line 436 :file "cli/orchestration-import-cli.bclj"} (map second ^{:line 436 :file "cli/orchestration-import-cli.bclj"} (sort-by first ^{:line 436 :file "cli/orchestration-import-cli.bclj"} (deref actions))))
  ^{:line 438 :file "cli/orchestration-import-cli.bclj"} (flip! port ver)
  ver))

^{:line 441 :file "cli/orchestration-import-cli.bclj"} (defn retract-version! [port ver]
  ^{:line 445 :file "cli/orchestration-import-cli.bclj"} (let [prefix ^{:line 445 :file "cli/orchestration-import-cli.bclj"} (str "@catalog:v" ver ":")
   rows ^{:line 446 :file "cli/orchestration-import-cli.bclj"} (north.coord/query-rows! port ^{:line 448 :file "cli/orchestration-import-cli.bclj"} {:find "s,p,o" :rules ^{:line 449 :file "cli/orchestration-import-cli.bclj"} [^{:line 449 :file "cli/orchestration-import-cli.bclj"} {:head ^{:line 449 :file "cli/orchestration-import-cli.bclj"} {:rel "s,p,o" :args ^{:line 449 :file "cli/orchestration-import-cli.bclj"} [^{:line 449 :file "cli/orchestration-import-cli.bclj"} {:var "s"} ^{:line 449 :file "cli/orchestration-import-cli.bclj"} {:var "p"} ^{:line 449 :file "cli/orchestration-import-cli.bclj"} {:var "o"}]} :body ^{:line 450 :file "cli/orchestration-import-cli.bclj"} [^{:line 450 :file "cli/orchestration-import-cli.bclj"} {:rel "triple" :args ^{:line 450 :file "cli/orchestration-import-cli.bclj"} [^{:line 450 :file "cli/orchestration-import-cli.bclj"} {:var "s"} ^{:line 450 :file "cli/orchestration-import-cli.bclj"} {:var "p"} ^{:line 450 :file "cli/orchestration-import-cli.bclj"} {:var "o"}]}]}]})
   mine ^{:line 451 :file "cli/orchestration-import-cli.bclj"} (filter ^{:line 451 :file "cli/orchestration-import-cli.bclj"} (fn [[s _predicate _value]] ^{:line 452 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? s prefix)) rows)]
  ^{:line 454 :file "cli/orchestration-import-cli.bclj"} (publish-actions! port ^{:line 456 :file "cli/orchestration-import-cli.bclj"} (concat ^{:line 457 :file "cli/orchestration-import-cli.bclj"} (for [[subject predicate] ^{:line 457 :file "cli/orchestration-import-cli.bclj"} (distinct ^{:line 457 :file "cli/orchestration-import-cli.bclj"} (map ^{:line 457 :file "cli/orchestration-import-cli.bclj"} (juxt first second) mine))]
  ^{:line 458 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject subject :predicate predicate :values ^{:line 458 :file "cli/orchestration-import-cli.bclj"} [] :cardinality :many}) ^{:line 460 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 460 :file "cli/orchestration-import-cli.bclj"} (= ver ^{:line 460 :file "cli/orchestration-import-cli.bclj"} (current-version! port)) ^{:line 461 :file "cli/orchestration-import-cli.bclj"} [^{:line 461 :file "cli/orchestration-import-cli.bclj"} {:op :set :subject POINTER :predicate "catalog_version" :values ^{:line 461 :file "cli/orchestration-import-cli.bclj"} [] :cardinality :one}] ^{:line 463 :file "cli/orchestration-import-cli.bclj"} [])))
  ^{:line 464 :file "cli/orchestration-import-cli.bclj"} (count mine)))

^{:line 466 :file "cli/orchestration-import-cli.bclj"} (defn show! [port ver]
  ^{:line 469 :file "cli/orchestration-import-cli.bclj"} (let [prefix ^{:line 469 :file "cli/orchestration-import-cli.bclj"} (str "@catalog:v" ver ":")
   rows ^{:line 470 :file "cli/orchestration-import-cli.bclj"} (north.coord/query-rows! port ^{:line 472 :file "cli/orchestration-import-cli.bclj"} {:find "s" :rules ^{:line 472 :file "cli/orchestration-import-cli.bclj"} [^{:line 472 :file "cli/orchestration-import-cli.bclj"} {:head ^{:line 472 :file "cli/orchestration-import-cli.bclj"} {:rel "s" :args ^{:line 472 :file "cli/orchestration-import-cli.bclj"} [^{:line 472 :file "cli/orchestration-import-cli.bclj"} {:var "s"}]} :body ^{:line 473 :file "cli/orchestration-import-cli.bclj"} [^{:line 473 :file "cli/orchestration-import-cli.bclj"} {:rel "triple" :args ^{:line 473 :file "cli/orchestration-import-cli.bclj"} [^{:line 473 :file "cli/orchestration-import-cli.bclj"} {:var "s"} "kind" ^{:line 473 :file "cli/orchestration-import-cli.bclj"} {:var "k"}]}]}]})
   mine ^{:line 474 :file "cli/orchestration-import-cli.bclj"} (sort ^{:line 475 :file "cli/orchestration-import-cli.bclj"} (distinct ^{:line 476 :file "cli/orchestration-import-cli.bclj"} (filter ^{:line 476 :file "cli/orchestration-import-cli.bclj"} (fn [subject] ^{:line 477 :file "cli/orchestration-import-cli.bclj"} (str/starts-with? subject prefix)) ^{:line 478 :file "cli/orchestration-import-cli.bclj"} (map first rows))))]
  ^{:line 479 :file "cli/orchestration-import-cli.bclj"} (println ^{:line 479 :file "cli/orchestration-import-cli.bclj"} (format "pointer @catalog:current -> v%s (%d subjects)" ver ^{:line 479 :file "cli/orchestration-import-cli.bclj"} (count mine)))
  ^{:line 480 :file "cli/orchestration-import-cli.bclj"} (doseq [s mine]
  ^{:line 480 :file "cli/orchestration-import-cli.bclj"} (println "  " s))))

^{:line 482 :file "cli/orchestration-import-cli.bclj"} (defn -main [& $beagle$rest$host]
  (let [argv (vec $beagle$rest$host)]
  ^{:line 483 :file "cli/orchestration-import-cli.bclj"} (let [[ps verb arg] argv
   port ^{:line 484 :file "cli/orchestration-import-cli.bclj"} (Integer/parseInt ^{:line 484 :file "cli/orchestration-import-cli.bclj"} (or ps "7977"))]
  ^{:line 485 :file "cli/orchestration-import-cli.bclj"} (case verb
    "import" ^{:line 487 :file "cli/orchestration-import-cli.bclj"} (let [ver ^{:line 487 :file "cli/orchestration-import-cli.bclj"} (import! port ^{:line 487 :file "cli/orchestration-import-cli.bclj"} (orchestration-home arg))]
  ^{:line 488 :file "cli/orchestration-import-cli.bclj"} (println ^{:line 488 :file "cli/orchestration-import-cli.bclj"} (format "✓ imported catalog v%d on :%d; @catalog:current -> v%d" ver port ver)))
    "retract" ^{:line 490 :file "cli/orchestration-import-cli.bclj"} (let [ver ^{:line 490 :file "cli/orchestration-import-cli.bclj"} (version-arg! port arg)
   n ^{:line 491 :file "cli/orchestration-import-cli.bclj"} (retract-version! port ver)]
  ^{:line 492 :file "cli/orchestration-import-cli.bclj"} (println ^{:line 492 :file "cli/orchestration-import-cli.bclj"} (format "✓ retracted %d facts under @catalog:v%d:" n ver)))
    "show" ^{:line 493 :file "cli/orchestration-import-cli.bclj"} (show! port ^{:line 493 :file "cli/orchestration-import-cli.bclj"} (version-arg! port arg))
    ^{:line 495 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 496 :file "cli/orchestration-import-cli.bclj"} (println "usage: orchestration-import-cli.clj <port> {import|retract <N|vN>|show [N|vN]} [orchestration-home]")
  ^{:line 497 :file "cli/orchestration-import-cli.bclj"} (System/exit 2))))))

^{:line 501 :file "cli/orchestration-import-cli.bclj"} (if ^{:line 501 :file "cli/orchestration-import-cli.bclj"} (= *file* ^{:line 501 :file "cli/orchestration-import-cli.bclj"} (System/getProperty "babashka.file")) ^{:line 501 :file "cli/orchestration-import-cli.bclj"} (do
  ^{:line 502 :file "cli/orchestration-import-cli.bclj"} (apply -main *command-line-args*)))
