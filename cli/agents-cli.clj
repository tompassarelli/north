(ns beagle.user
  (:require [babashka.process :as p]
            [clojure.string :as str]
            [clojure.set :as set]
            [clojure.java.io :as io]
            [clojure.walk :as walk]
            [cheshire.core :as json]))

^{:line 50 :file "cli/agents-cli.bclj"} (def HOME ^{:line 50 :file "cli/agents-cli.bclj"} (System/getenv "HOME"))

^{:line 51 :file "cli/agents-cli.bclj"} (def NORTH ^{:line 51 :file "cli/agents-cli.bclj"} (or ^{:line 51 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_HOME") ^{:line 52 :file "cli/agents-cli.bclj"} (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str)))

^{:line 53 :file "cli/agents-cli.bclj"} (def NORTH-CLI ^{:line 53 :file "cli/agents-cli.bclj"} (or ^{:line 53 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_BIN") ^{:line 53 :file "cli/agents-cli.bclj"} (str NORTH "/bin/north")))

^{:line 54 :file "cli/agents-cli.bclj"} (def AGENT-MACHINERY ^{:line 55 :file "cli/agents-cli.bclj"} (or ^{:line 55 :file "cli/agents-cli.bclj"} (System/getenv "AGENT_MACHINERY_HOME") ^{:line 56 :file "cli/agents-cli.bclj"} (str HOME "/code/agent-machinery/main")))

^{:line 57 :file "cli/agents-cli.bclj"} (def AGENT-RUNTIME ^{:line 58 :file "cli/agents-cli.bclj"} (or ^{:line 58 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_AGENT_RUNTIME_HOME") ^{:line 59 :file "cli/agents-cli.bclj"} (str NORTH "/agent-runtime/orchestration")))

^{:line 60 :file "cli/agents-cli.bclj"} (def AGENT-LOGDIR ^{:line 60 :file "cli/agents-cli.bclj"} (str HOME "/.local/state/north/agents"))

^{:line 61 :file "cli/agents-cli.bclj"} (def AGENT-STREAMDIR ^{:line 61 :file "cli/agents-cli.bclj"} (or ^{:line 61 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_STREAM_DIR") ^{:line 62 :file "cli/agents-cli.bclj"} (str HOME "/code/agent-data")))

^{:line 63 :file "cli/agents-cli.bclj"} (def ORCHESTRATION-STAFFING ^{:line 63 :file "cli/agents-cli.bclj"} (or ^{:line 63 :file "cli/agents-cli.bclj"} (System/getenv "ORCHESTRATION_STAFFING_CATALOG") ^{:line 64 :file "cli/agents-cli.bclj"} (str AGENT-MACHINERY "/staffing/catalog.json")))

^{:line 65 :file "cli/agents-cli.bclj"} (def PORT ^{:line 65 :file "cli/agents-cli.bclj"} (or ^{:line 65 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_PORT") "7977"))

^{:line 66 :file "cli/agents-cli.bclj"} (def ROSTER-CONTRACT-VERSION "north:agent-roster:v1")

^{:line 67 :file "cli/agents-cli.bclj"} (def CODEX-CENSUS-CLI ^{:line 67 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/codex-census-cli.ts"))

^{:line 68 :file "cli/agents-cli.bclj"} (def STRUGGLE-POLICY-CLI ^{:line 68 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/struggle.ts"))

^{:line 69 :file "cli/agents-cli.bclj"} (def PROVIDER-CAPABILITY-ADMISSION-CLI ^{:line 70 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/provider-capability-admission-cli.ts"))

^{:line 71 :file "cli/agents-cli.bclj"} (def ROUTING-ECONOMICS-PREFLIGHT-CLI ^{:line 72 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/routing-economics-preflight-cli.ts"))

^{:line 73 :file "cli/agents-cli.bclj"} (def DELEGATION-RUN-DESIGN-TRANSPORT ^{:line 74 :file "cli/agents-cli.bclj"} (or ^{:line 74 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_DELEGATION_RUN_DESIGN_TRANSPORT") ^{:line 75 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/providers/delegation-run-design-transport.ts")))

^{:line 76 :file "cli/agents-cli.bclj"} (def POLICY-BUN ^{:line 76 :file "cli/agents-cli.bclj"} (or ^{:line 76 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_POLICY_BUN") "bun"))

^{:line 77 :file "cli/agents-cli.bclj"} (def PROVIDER-CAPABILITY-ADMISSION-SCHEMA "north:provider-capability-admission:v1")

^{:line 82 :file "cli/agents-cli.bclj"} (def msg-admission-timeout-ms 30000)

^{:line 84 :file "cli/agents-cli.bclj"} (load-file ^{:line 84 :file "cli/agents-cli.bclj"} (str NORTH "/cli/spawn-process.clj"))

^{:line 85 :file "cli/agents-cli.bclj"} (load-file ^{:line 85 :file "cli/agents-cli.bclj"} (str NORTH "/cli/coord.clj"))

^{:line 86 :file "cli/agents-cli.bclj"} (load-file ^{:line 86 :file "cli/agents-cli.bclj"} (str NORTH "/cli/message-routing.clj"))

^{:line 87 :file "cli/agents-cli.bclj"} (load-file ^{:line 87 :file "cli/agents-cli.bclj"} (str NORTH "/cli/topology-authority.clj"))

^{:line 88 :file "cli/agents-cli.bclj"} (load-file ^{:line 88 :file "cli/agents-cli.bclj"} (str NORTH "/cli/managed-child-env.clj"))

^{:line 89 :file "cli/agents-cli.bclj"} (load-file ^{:line 89 :file "cli/agents-cli.bclj"} (str NORTH "/cli/orchestration-staffing.clj"))

^{:line 91 :file "cli/agents-cli.bclj"} (def color? ^{:line 91 :file "cli/agents-cli.bclj"} (and ^{:line 91 :file "cli/agents-cli.bclj"} (nil? ^{:line 91 :file "cli/agents-cli.bclj"} (System/getenv "NO_COLOR")) ^{:line 92 :file "cli/agents-cli.bclj"} (some? ^{:line 92 :file "cli/agents-cli.bclj"} (System/console))))

^{:line 93 :file "cli/agents-cli.bclj"} (defn- c [code s]
  ^{:line 96 :file "cli/agents-cli.bclj"} (if color? ^{:line 96 :file "cli/agents-cli.bclj"} (str "\u001b[" code "m" s "\u001b[0m") ^{:line 96 :file "cli/agents-cli.bclj"} (str s)))

^{:line 97 :file "cli/agents-cli.bclj"} (defn dim [s]
  ^{:line 97 :file "cli/agents-cli.bclj"} (c "2" s))

^{:line 98 :file "cli/agents-cli.bclj"} (defn bold [s]
  ^{:line 98 :file "cli/agents-cli.bclj"} (c "1" s))

^{:line 99 :file "cli/agents-cli.bclj"} (defn grn [s]
  ^{:line 99 :file "cli/agents-cli.bclj"} (c "32" s))

^{:line 100 :file "cli/agents-cli.bclj"} (defn red [s]
  ^{:line 100 :file "cli/agents-cli.bclj"} (c "31" s))

^{:line 101 :file "cli/agents-cli.bclj"} (defn ylw [s]
  ^{:line 101 :file "cli/agents-cli.bclj"} (c "33" s))

^{:line 102 :file "cli/agents-cli.bclj"} (defn cyn [s]
  ^{:line 102 :file "cli/agents-cli.bclj"} (c "36" s))

^{:line 104 :file "cli/agents-cli.bclj"} (defn run [argv & $beagle$rest$host]
  (let [{:keys [timeout in env] :or {timeout 4000}} $beagle$rest$host]
  ^{:line 107 :file "cli/agents-cli.bclj"} (try
  ^{:line 108 :file "cli/agents-cli.bclj"} (let [proc ^{:line 108 :file "cli/agents-cli.bclj"} (p/process argv ^{:line 108 :file "cli/agents-cli.bclj"} (cond-> ^{:line 108 :file "cli/agents-cli.bclj"} {:out :string :err :string} in ^{:line 109 :file "cli/agents-cli.bclj"} (assoc :in in) env ^{:line 110 :file "cli/agents-cli.bclj"} (assoc :env env)))
   res ^{:line 111 :file "cli/agents-cli.bclj"} (deref proc timeout ::timeout)]
  ^{:line 112 :file "cli/agents-cli.bclj"} (if ^{:line 112 :file "cli/agents-cli.bclj"} (= res ::timeout) ^{:line 113 :file "cli/agents-cli.bclj"} (do
  ^{:line 113 :file "cli/agents-cli.bclj"} (p/destroy-tree proc)
  ^{:line 113 :file "cli/agents-cli.bclj"} {:timeout true :ok false}) ^{:line 114 :file "cli/agents-cli.bclj"} {:out ^{:line 114 :file "cli/agents-cli.bclj"} (or ^{:line 114 :file "cli/agents-cli.bclj"} (:out res) "") :err ^{:line 114 :file "cli/agents-cli.bclj"} (or ^{:line 114 :file "cli/agents-cli.bclj"} (:err res) "") :exit ^{:line 114 :file "cli/agents-cli.bclj"} (:exit res) :ok ^{:line 115 :file "cli/agents-cli.bclj"} (zero? ^{:line 115 :file "cli/agents-cli.bclj"} (:exit res))}))
  (catch Exception e
    ^{:line 119 :file "cli/agents-cli.bclj"} {:error ^{:line 119 :file "cli/agents-cli.bclj"} (or ^{:line 119 :file "cli/agents-cli.bclj"} (not-empty ^{:line 119 :file "cli/agents-cli.bclj"} (str ^{:line 119 :file "cli/agents-cli.bclj"} (.getMessage e))) ^{:line 120 :file "cli/agents-cli.bclj"} (.getName ^{:line 120 :file "cli/agents-cli.bclj"} (class e))) :ok false}))))

^{:line 123 :file "cli/agents-cli.bclj"} (defn echo-cmd [& $beagle$rest$host]
  (let [parts (vec $beagle$rest$host)]
  ^{:line 123 :file "cli/agents-cli.bclj"} (println ^{:line 123 :file "cli/agents-cli.bclj"} (dim ^{:line 123 :file "cli/agents-cli.bclj"} (str "» " ^{:line 123 :file "cli/agents-cli.bclj"} (str/join " " parts))))))

^{:line 125 :file "cli/agents-cli.bclj"} (defn resolve-struggle-policy! [topology]
  ^{:line 126 :file "cli/agents-cli.bclj"} (let [result ^{:line 126 :file "cli/agents-cli.bclj"} (run ^{:line 126 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" STRUGGLE-POLICY-CLI "policy" topology])
   raw ^{:line 127 :file "cli/agents-cli.bclj"} (str/trim ^{:line 127 :file "cli/agents-cli.bclj"} (or ^{:line 127 :file "cli/agents-cli.bclj"} (:out result) ""))
   parsed ^{:line 128 :file "cli/agents-cli.bclj"} (try
  ^{:line 128 :file "cli/agents-cli.bclj"} (json/parse-string raw true)
  (catch Exception _
    nil))]
  ^{:line 129 :file "cli/agents-cli.bclj"} (if ^{:line 129 :file "cli/agents-cli.bclj"} (not ^{:line 129 :file "cli/agents-cli.bclj"} (and ^{:line 129 :file "cli/agents-cli.bclj"} (:ok result) ^{:line 129 :file "cli/agents-cli.bclj"} (map? parsed) ^{:line 130 :file "cli/agents-cli.bclj"} (= topology ^{:line 130 :file "cli/agents-cli.bclj"} (:topology parsed)) ^{:line 131 :file "cli/agents-cli.bclj"} (string? ^{:line 131 :file "cli/agents-cli.bclj"} (:version parsed)) ^{:line 132 :file "cli/agents-cli.bclj"} (every? pos-int? ^{:line 132 :file "cli/agents-cli.bclj"} (map parsed ^{:line 133 :file "cli/agents-cli.bclj"} [:errorStreak :loopRepeat :loopWindow :noProgressTurns])))) ^{:line 129 :file "cli/agents-cli.bclj"} (do
  ^{:line 135 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 136 :file "cli/agents-cli.bclj"} (println ^{:line 136 :file "cli/agents-cli.bclj"} (red ^{:line 136 :file "cli/agents-cli.bclj"} (or ^{:line 136 :file "cli/agents-cli.bclj"} (not-empty ^{:line 136 :file "cli/agents-cli.bclj"} (str/trim ^{:line 136 :file "cli/agents-cli.bclj"} (or ^{:line 136 :file "cli/agents-cli.bclj"} (:err result) ""))) "could not resolve struggle detector policy"))))
  ^{:line 138 :file "cli/agents-cli.bclj"} (System/exit 1)))
  ^{:line 139 :file "cli/agents-cli.bclj"} (assoc parsed :canonical raw)))

^{:line 141 :file "cli/agents-cli.bclj"} (defn- require-pinned-provider-capabilities!
  "Run the SDK's exact pinned-provider gate before delegate referent, identity,\n   account, or provider state can be touched. Auto remains intentionally open:\n   execution may select any capability-compatible provider." [provider target capabilities]
  ^{:line 148 :file "cli/agents-cli.bclj"} (if ^{:line 148 :file "cli/agents-cli.bclj"} (and provider ^{:line 148 :file "cli/agents-cli.bclj"} (not= provider "auto")) ^{:line 148 :file "cli/agents-cli.bclj"} (do
  ^{:line 149 :file "cli/agents-cli.bclj"} (let [argv ^{:line 149 :file "cli/agents-cli.bclj"} (cond-> ^{:line 149 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" PROVIDER-CAPABILITY-ADMISSION-CLI provider ^{:line 150 :file "cli/agents-cli.bclj"} (json/generate-string capabilities)] target ^{:line 151 :file "cli/agents-cli.bclj"} (conj target))
   result ^{:line 152 :file "cli/agents-cli.bclj"} (run argv)
   raw ^{:line 153 :file "cli/agents-cli.bclj"} (str/trim ^{:line 153 :file "cli/agents-cli.bclj"} (or ^{:line 153 :file "cli/agents-cli.bclj"} (:out result) ""))
   parsed ^{:line 154 :file "cli/agents-cli.bclj"} (try
  ^{:line 154 :file "cli/agents-cli.bclj"} (json/parse-string raw true)
  (catch Exception _
    nil))
   expected-base ^{:line 155 :file "cli/agents-cli.bclj"} (cond-> ^{:line 155 :file "cli/agents-cli.bclj"} #{:schema :provider :capabilities :status} target ^{:line 156 :file "cli/agents-cli.bclj"} (conj :requestedTarget))
   supported? ^{:line 157 :file "cli/agents-cli.bclj"} (and ^{:line 157 :file "cli/agents-cli.bclj"} (:ok result) ^{:line 158 :file "cli/agents-cli.bclj"} (map? parsed) ^{:line 159 :file "cli/agents-cli.bclj"} (= expected-base ^{:line 159 :file "cli/agents-cli.bclj"} (set ^{:line 159 :file "cli/agents-cli.bclj"} (keys parsed))) ^{:line 160 :file "cli/agents-cli.bclj"} (= PROVIDER-CAPABILITY-ADMISSION-SCHEMA ^{:line 160 :file "cli/agents-cli.bclj"} (:schema parsed)) ^{:line 161 :file "cli/agents-cli.bclj"} (= "supported" ^{:line 161 :file "cli/agents-cli.bclj"} (:status parsed)) ^{:line 162 :file "cli/agents-cli.bclj"} (= provider ^{:line 162 :file "cli/agents-cli.bclj"} (:provider parsed)) ^{:line 163 :file "cli/agents-cli.bclj"} (= capabilities ^{:line 163 :file "cli/agents-cli.bclj"} (:capabilities parsed)) ^{:line 164 :file "cli/agents-cli.bclj"} (= target ^{:line 164 :file "cli/agents-cli.bclj"} (:requestedTarget parsed)))
   unsupported-fields ^{:line 165 :file "cli/agents-cli.bclj"} (into expected-base ^{:line 166 :file "cli/agents-cli.bclj"} #{:code :processOutcome :reason :retrySafeBeforeAcceptance})
   unsupported? ^{:line 168 :file "cli/agents-cli.bclj"} (and ^{:line 168 :file "cli/agents-cli.bclj"} (= 3 ^{:line 168 :file "cli/agents-cli.bclj"} (:exit result)) ^{:line 169 :file "cli/agents-cli.bclj"} (map? parsed) ^{:line 170 :file "cli/agents-cli.bclj"} (= unsupported-fields ^{:line 170 :file "cli/agents-cli.bclj"} (set ^{:line 170 :file "cli/agents-cli.bclj"} (keys parsed))) ^{:line 171 :file "cli/agents-cli.bclj"} (= PROVIDER-CAPABILITY-ADMISSION-SCHEMA ^{:line 171 :file "cli/agents-cli.bclj"} (:schema parsed)) ^{:line 172 :file "cli/agents-cli.bclj"} (= "unsupported" ^{:line 172 :file "cli/agents-cli.bclj"} (:status parsed)) ^{:line 173 :file "cli/agents-cli.bclj"} (= provider ^{:line 173 :file "cli/agents-cli.bclj"} (:provider parsed)) ^{:line 174 :file "cli/agents-cli.bclj"} (= capabilities ^{:line 174 :file "cli/agents-cli.bclj"} (:capabilities parsed)) ^{:line 175 :file "cli/agents-cli.bclj"} (= target ^{:line 175 :file "cli/agents-cli.bclj"} (:requestedTarget parsed)) ^{:line 176 :file "cli/agents-cli.bclj"} (= true ^{:line 176 :file "cli/agents-cli.bclj"} (:retrySafeBeforeAcceptance parsed)) ^{:line 177 :file "cli/agents-cli.bclj"} (every? ^{:line 177 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 177 :file "cli/agents-cli.bclj"} (and ^{:line 177 :file "cli/agents-cli.bclj"} (string? %1) ^{:line 177 :file "cli/agents-cli.bclj"} (not ^{:line 177 :file "cli/agents-cli.bclj"} (str/blank? %1)))) ^{:line 178 :file "cli/agents-cli.bclj"} (map parsed ^{:line 178 :file "cli/agents-cli.bclj"} [:code :processOutcome :reason])))]
  ^{:line 179 :file "cli/agents-cli.bclj"} (cond
  supported? nil
  unsupported? ^{:line 182 :file "cli/agents-cli.bclj"} (do
  ^{:line 183 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 184 :file "cli/agents-cli.bclj"} (println ^{:line 184 :file "cli/agents-cli.bclj"} (red ^{:line 184 :file "cli/agents-cli.bclj"} (str "provider capability admission rejected before side effects: " ^{:line 185 :file "cli/agents-cli.bclj"} (:reason parsed)))))
  ^{:line 188 :file "cli/agents-cli.bclj"} (println raw)
  ^{:line 189 :file "cli/agents-cli.bclj"} (System/exit 1))
  :else ^{:line 191 :file "cli/agents-cli.bclj"} (do
  ^{:line 192 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 193 :file "cli/agents-cli.bclj"} (println ^{:line 193 :file "cli/agents-cli.bclj"} (red ^{:line 193 :file "cli/agents-cli.bclj"} (or ^{:line 193 :file "cli/agents-cli.bclj"} (not-empty ^{:line 193 :file "cli/agents-cli.bclj"} (str/trim ^{:line 193 :file "cli/agents-cli.bclj"} (or ^{:line 193 :file "cli/agents-cli.bclj"} (:err result) ""))) "provider capability admission unavailable"))))
  ^{:line 195 :file "cli/agents-cli.bclj"} (System/exit 1)))))))

^{:line 198 :file "cli/agents-cli.bclj"} (defn orchestration-catalog []
  ^{:line 199 :file "cli/agents-cli.bclj"} (let [f ^{:line 199 :file "cli/agents-cli.bclj"} (io/file ORCHESTRATION-STAFFING)]
  ^{:line 200 :file "cli/agents-cli.bclj"} (if ^{:line 200 :file "cli/agents-cli.bclj"} (.isFile f) ^{:line 200 :file "cli/agents-cli.bclj"} (do
  ^{:line 201 :file "cli/agents-cli.bclj"} (walk/keywordize-keys ^{:line 201 :file "cli/agents-cli.bclj"} (north.orchestration-staffing/load-catalog ^{:line 201 :file "cli/agents-cli.bclj"} (.getPath f)))))))

^{:line 203 :file "cli/agents-cli.bclj"} (defn orchestration-routing []
  ^{:line 204 :file "cli/agents-cli.bclj"} (let [bind__12 ^{:line 204 :file "cli/agents-cli.bclj"} (orchestration-catalog)]
  ^{:line 204 :file "cli/agents-cli.bclj"} (if bind__12 ^{:line 204 :file "cli/agents-cli.bclj"} (let [{:keys [presets defaults]} bind__12]
  ^{:line 204 :file "cli/agents-cli.bclj"} (do
  ^{:line 205 :file "cli/agents-cli.bclj"} (into ^{:line 205 :file "cli/agents-cli.bclj"} {} ^{:line 206 :file "cli/agents-cli.bclj"} (map ^{:line 206 :file "cli/agents-cli.bclj"} (fn [r] ^{:line 207 :file "cli/agents-cli.bclj"} (let [name ^{:line 207 :file "cli/agents-cli.bclj"} (:name r)]
  ^{:line 208 :file "cli/agents-cli.bclj"} [name ^{:line 208 :file "cli/agents-cli.bclj"} (-> ^{:line 208 :file "cli/agents-cli.bclj"} (merge defaults r) ^{:line 209 :file "cli/agents-cli.bclj"} (assoc :role name :orchestration-preset true :composition ^{:line 210 :file "cli/agents-cli.bclj"} {:kind "template" :id name :overrides ^{:line 210 :file "cli/agents-cli.bclj"} []}))]))) presets))))))

^{:line 213 :file "cli/agents-cli.bclj"} (defn orchestration-templates []
  ^{:line 214 :file "cli/agents-cli.bclj"} (let [bind__13 ^{:line 214 :file "cli/agents-cli.bclj"} (orchestration-catalog)]
  ^{:line 214 :file "cli/agents-cli.bclj"} (if bind__13 ^{:line 214 :file "cli/agents-cli.bclj"} (let [{:keys [presets defaults]} bind__13]
  ^{:line 214 :file "cli/agents-cli.bclj"} (do
  ^{:line 215 :file "cli/agents-cli.bclj"} (mapv ^{:line 215 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 215 :file "cli/agents-cli.bclj"} (merge defaults %1)) presets))))))

^{:line 217 :file "cli/agents-cli.bclj"} (defn cmd-templates [args]
  ^{:line 218 :file "cli/agents-cli.bclj"} (if ^{:line 218 :file "cli/agents-cli.bclj"} (some ^{:line 218 :file "cli/agents-cli.bclj"} #{"--help" "-h" "help"} args) ^{:line 218 :file "cli/agents-cli.bclj"} (do
  ^{:line 219 :file "cli/agents-cli.bclj"} (println "north agent templates — inspect Orchestration's reusable stock templates")
  ^{:line 220 :file "cli/agents-cli.bclj"} (println)
  ^{:line 221 :file "cli/agents-cli.bclj"} (println "Usage:")
  ^{:line 222 :file "cli/agents-cli.bclj"} (println "  north agent templates             compact template catalog")
  ^{:line 223 :file "cli/agents-cli.bclj"} (println "  north agent templates --verbose   include each template's selection boundary")
  ^{:line 224 :file "cli/agents-cli.bclj"} (System/exit 0)))
  ^{:line 225 :file "cli/agents-cli.bclj"} (let [unknown ^{:line 225 :file "cli/agents-cli.bclj"} (first ^{:line 225 :file "cli/agents-cli.bclj"} (remove ^{:line 225 :file "cli/agents-cli.bclj"} (fn [arg] ^{:line 226 :file "cli/agents-cli.bclj"} (contains? ^{:line 226 :file "cli/agents-cli.bclj"} #{"--verbose"} arg)) args))]
  ^{:line 225 :file "cli/agents-cli.bclj"} (if unknown ^{:line 225 :file "cli/agents-cli.bclj"} (do
  ^{:line 228 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 229 :file "cli/agents-cli.bclj"} (println ^{:line 229 :file "cli/agents-cli.bclj"} (red ^{:line 229 :file "cli/agents-cli.bclj"} (str "unknown templates option: " unknown)))
  ^{:line 230 :file "cli/agents-cli.bclj"} (println "usage: north agent templates [--verbose]"))
  ^{:line 231 :file "cli/agents-cli.bclj"} (System/exit 2))))
  ^{:line 232 :file "cli/agents-cli.bclj"} (let [verbose? ^{:line 232 :file "cli/agents-cli.bclj"} (some ^{:line 232 :file "cli/agents-cli.bclj"} #{"--verbose"} args)
   templates ^{:line 233 :file "cli/agents-cli.bclj"} (orchestration-templates)]
  ^{:line 234 :file "cli/agents-cli.bclj"} (if ^{:line 234 :file "cli/agents-cli.bclj"} (seq templates) ^{:line 239 :file "cli/agents-cli.bclj"} (do
  ^{:line 240 :file "cli/agents-cli.bclj"} (println ^{:line 240 :file "cli/agents-cli.bclj"} (bold "AGENT MACHINERY STOCK TEMPLATES — reusable starting points, not limits"))
  ^{:line 241 :file "cli/agents-cli.bclj"} (println ^{:line 241 :file "cli/agents-cli.bclj"} (dim "Selection ladder: exact template → justified axis override → bespoke composition."))
  ^{:line 242 :file "cli/agents-cli.bclj"} (println ^{:line 242 :file "cli/agents-cli.bclj"} (dim "Machine payloads retain composition.kind=template; this view uses the human word template."))
  ^{:line 243 :file "cli/agents-cli.bclj"} (doseq [{:keys [name tagline taskGrade tier deliberation topology posture capabilities description]} templates]
  ^{:line 245 :file "cli/agents-cli.bclj"} (println)
  ^{:line 246 :file "cli/agents-cli.bclj"} (println ^{:line 246 :file "cli/agents-cli.bclj"} (bold name) "—" tagline)
  ^{:line 247 :file "cli/agents-cli.bclj"} (println ^{:line 247 :file "cli/agents-cli.bclj"} (dim ^{:line 247 :file "cli/agents-cli.bclj"} (str "  grade " taskGrade " · " tier "/" deliberation " · " topology " · " posture)))
  ^{:line 249 :file "cli/agents-cli.bclj"} (println ^{:line 249 :file "cli/agents-cli.bclj"} (dim ^{:line 249 :file "cli/agents-cli.bclj"} (str "  capabilities " ^{:line 249 :file "cli/agents-cli.bclj"} (str/join " " capabilities))))
  ^{:line 250 :file "cli/agents-cli.bclj"} (if verbose? ^{:line 250 :file "cli/agents-cli.bclj"} (do
  ^{:line 250 :file "cli/agents-cli.bclj"} (println ^{:line 250 :file "cli/agents-cli.bclj"} (str "  " description)))))) ^{:line 235 :file "cli/agents-cli.bclj"} (do
  ^{:line 236 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 237 :file "cli/agents-cli.bclj"} (println ^{:line 237 :file "cli/agents-cli.bclj"} (red ^{:line 237 :file "cli/agents-cli.bclj"} (str "Delegation run-composition catalog unavailable: " ORCHESTRATION-STAFFING))))
  ^{:line 238 :file "cli/agents-cli.bclj"} (System/exit 1)))))

^{:line 256 :file "cli/agents-cli.bclj"} (defn dry-resolved-route [provider tier explicit-model reasoning]
  ^{:line 261 :file "cli/agents-cli.bclj"} (if ^{:line 261 :file "cli/agents-cli.bclj"} (and provider ^{:line 261 :file "cli/agents-cli.bclj"} (not= provider "auto")) ^{:line 261 :file "cli/agents-cli.bclj"} (do
  ^{:line 262 :file "cli/agents-cli.bclj"} (try
  ^{:line 263 :file "cli/agents-cli.bclj"} (let [entry ^{:line 263 :file "cli/agents-cli.bclj"} (get-in ^{:line 263 :file "cli/agents-cli.bclj"} (json/parse-string ^{:line 264 :file "cli/agents-cli.bclj"} (slurp ^{:line 264 :file "cli/agents-cli.bclj"} (io/file AGENT-RUNTIME "providers" ^{:line 264 :file "cli/agents-cli.bclj"} (str provider ".json"))) true) ^{:line 265 :file "cli/agents-cli.bclj"} [:tiers ^{:line 265 :file "cli/agents-cli.bclj"} (keyword tier)])]
  ^{:line 266 :file "cli/agents-cli.bclj"} {:provider provider :model ^{:line 267 :file "cli/agents-cli.bclj"} (or explicit-model ^{:line 267 :file "cli/agents-cli.bclj"} (:model entry)) :effort ^{:line 268 :file "cli/agents-cli.bclj"} (or reasoning ^{:line 268 :file "cli/agents-cli.bclj"} (:defaultEffort entry) ^{:line 268 :file "cli/agents-cli.bclj"} (:defaultReasoning entry))})
  (catch Exception _
    ^{:line 269 :file "cli/agents-cli.bclj"} {:provider provider :model explicit-model :effort reasoning})))))

^{:line 271 :file "cli/agents-cli.bclj"} (declare known semantic-handle)

^{:line 273 :file "cli/agents-cli.bclj"} (defn- agent-facts-one [id]
  ^{:line 274 :file "cli/agents-cli.bclj"} (try
  ^{:line 275 :file "cli/agents-cli.bclj"} (let [rows ^{:line 275 :file "cli/agents-cli.bclj"} (north.coord/show-rows! ^{:line 275 :file "cli/agents-cli.bclj"} (parse-long PORT) ^{:line 275 :file "cli/agents-cli.bclj"} (str "@agent:" id))]
  ^{:line 276 :file "cli/agents-cli.bclj"} (if ^{:line 276 :file "cli/agents-cli.bclj"} (and ^{:line 276 :file "cli/agents-cli.bclj"} (vector? rows) ^{:line 277 :file "cli/agents-cli.bclj"} (every? ^{:line 277 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 277 :file "cli/agents-cli.bclj"} (and ^{:line 277 :file "cli/agents-cli.bclj"} (vector? %1) ^{:line 277 :file "cli/agents-cli.bclj"} (= 2 ^{:line 277 :file "cli/agents-cli.bclj"} (count %1)) ^{:line 277 :file "cli/agents-cli.bclj"} (every? string? %1))) rows)) ^{:line 276 :file "cli/agents-cli.bclj"} (do
  ^{:line 280 :file "cli/agents-cli.bclj"} (reduce ^{:line 280 :file "cli/agents-cli.bclj"} (fn [acc [predicate value]] ^{:line 283 :file "cli/agents-cli.bclj"} (north.agent-provenance/fold-fact acc predicate value)) ^{:line 284 :file "cli/agents-cli.bclj"} {} rows))))
  (catch Exception _
    nil)))

^{:line 287 :file "cli/agents-cli.bclj"} (def control-id-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

^{:line 288 :file "cli/agents-cli.bclj"} (def max-control-id-bytes 256)

^{:line 289 :file "cli/agents-cli.bclj"} (def max-live-controls 256)

^{:line 290 :file "cli/agents-cli.bclj"} (def max-roster-fact-rows 32768)

^{:line 291 :file "cli/agents-cli.bclj"} (def max-roster-run-candidates 4096)

^{:line 292 :file "cli/agents-cli.bclj"} (def roster-conflict-key "__roster_conflicts")

^{:line 293 :file "cli/agents-cli.bclj"} (def lane-resolution-key ::lane-resolution)

^{:line 295 :file "cli/agents-cli.bclj"} (defn- valid-control-id? [value]
  ^{:line 296 :file "cli/agents-cli.bclj"} (and ^{:line 296 :file "cli/agents-cli.bclj"} (string? value) ^{:line 297 :file "cli/agents-cli.bclj"} (<= ^{:line 297 :file "cli/agents-cli.bclj"} (alength ^{:line 297 :file "cli/agents-cli.bclj"} (.getBytes value java.nio.charset.StandardCharsets/UTF_8)) max-control-id-bytes) ^{:line 299 :file "cli/agents-cli.bclj"} (boolean ^{:line 299 :file "cli/agents-cli.bclj"} (re-matches control-id-pattern value))))

^{:line 301 :file "cli/agents-cli.bclj"} (defn- fold-roster-fact [facts predicate value]
  ^{:line 305 :file "cli/agents-cli.bclj"} (let [prior-present? ^{:line 305 :file "cli/agents-cli.bclj"} (contains? facts predicate)
   prior ^{:line 306 :file "cli/agents-cli.bclj"} (get facts predicate)
   next ^{:line 307 :file "cli/agents-cli.bclj"} (north.agent-provenance/fold-fact facts predicate value)
   prior-has-value? ^{:line 308 :file "cli/agents-cli.bclj"} (cond
  ^{:line 309 :file "cli/agents-cli.bclj"} (not prior-present?) false
  ^{:line 310 :file "cli/agents-cli.bclj"} (set? prior) ^{:line 310 :file "cli/agents-cli.bclj"} (contains? prior value)
  ^{:line 311 :file "cli/agents-cli.bclj"} (and ^{:line 311 :file "cli/agents-cli.bclj"} (sequential? prior) ^{:line 311 :file "cli/agents-cli.bclj"} (not ^{:line 311 :file "cli/agents-cli.bclj"} (string? prior))) ^{:line 312 :file "cli/agents-cli.bclj"} (boolean ^{:line 312 :file "cli/agents-cli.bclj"} (some ^{:line 312 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 312 :file "cli/agents-cli.bclj"} (= value %1)) prior))
  :else ^{:line 313 :file "cli/agents-cli.bclj"} (= prior value))]
  ^{:line 314 :file "cli/agents-cli.bclj"} (if ^{:line 314 :file "cli/agents-cli.bclj"} (or ^{:line 314 :file "cli/agents-cli.bclj"} (= predicate "holds") ^{:line 315 :file "cli/agents-cli.bclj"} (not prior-present?) prior-has-value?) next ^{:line 318 :file "cli/agents-cli.bclj"} (update next roster-conflict-key ^{:line 318 :file "cli/agents-cli.bclj"} (fnil conj ^{:line 318 :file "cli/agents-cli.bclj"} #{}) predicate))))

^{:line 320 :file "cli/agents-cli.bclj"} (defn- fold-roster-subjects [rows-by-subject allowed-subjects]
  ^{:line 323 :file "cli/agents-cli.bclj"} (if ^{:line 323 :file "cli/agents-cli.bclj"} (not ^{:line 324 :file "cli/agents-cli.bclj"} (and ^{:line 324 :file "cli/agents-cli.bclj"} (map? rows-by-subject) ^{:line 325 :file "cli/agents-cli.bclj"} (every? ^{:line 326 :file "cli/agents-cli.bclj"} (fn [[subject rows]] ^{:line 327 :file "cli/agents-cli.bclj"} (and ^{:line 327 :file "cli/agents-cli.bclj"} (string? subject) ^{:line 328 :file "cli/agents-cli.bclj"} (contains? allowed-subjects subject) ^{:line 329 :file "cli/agents-cli.bclj"} (vector? rows) ^{:line 330 :file "cli/agents-cli.bclj"} (every? ^{:line 330 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 330 :file "cli/agents-cli.bclj"} (and ^{:line 330 :file "cli/agents-cli.bclj"} (vector? %1) ^{:line 330 :file "cli/agents-cli.bclj"} (= 2 ^{:line 330 :file "cli/agents-cli.bclj"} (count %1)) ^{:line 330 :file "cli/agents-cli.bclj"} (every? string? %1))) rows))) rows-by-subject) ^{:line 335 :file "cli/agents-cli.bclj"} (<= ^{:line 335 :file "cli/agents-cli.bclj"} (reduce + 0 ^{:line 335 :file "cli/agents-cli.bclj"} (map ^{:line 335 :file "cli/agents-cli.bclj"} (comp count val) rows-by-subject)) max-roster-fact-rows))) ^{:line 323 :file "cli/agents-cli.bclj"} (do
  ^{:line 337 :file "cli/agents-cli.bclj"} (throw ^{:line 337 :file "cli/agents-cli.bclj"} (ex-info "agent subject projection was malformed" ^{:line 337 :file "cli/agents-cli.bclj"} {}))))
  ^{:line 338 :file "cli/agents-cli.bclj"} (reduce ^{:line 339 :file "cli/agents-cli.bclj"} (fn [out [subject rows]] ^{:line 342 :file "cli/agents-cli.bclj"} (assoc-in out ^{:line 342 :file "cli/agents-cli.bclj"} [:agents ^{:line 342 :file "cli/agents-cli.bclj"} (subs subject ^{:line 342 :file "cli/agents-cli.bclj"} (count "@agent:"))] ^{:line 343 :file "cli/agents-cli.bclj"} (reduce ^{:line 343 :file "cli/agents-cli.bclj"} (fn [facts [predicate value]] ^{:line 346 :file "cli/agents-cli.bclj"} (fold-roster-fact facts predicate value)) ^{:line 347 :file "cli/agents-cli.bclj"} {} rows))) ^{:line 348 :file "cli/agents-cli.bclj"} {:agents ^{:line 348 :file "cli/agents-cli.bclj"} {} :sessions ^{:line 348 :file "cli/agents-cli.bclj"} {}} rows-by-subject))

^{:line 351 :file "cli/agents-cli.bclj"} (defn roster-facts
  "Read exact live @agent subjects from the coordination origin in one bounded\n  query. Historical telemetry @session descriptors are not live identity and\n  never enter the machine roster." [ids]
  ^{:line 355 :file "cli/agents-cli.bclj"} (let [ids ^{:line 355 :file "cli/agents-cli.bclj"} (vec ^{:line 355 :file "cli/agents-cli.bclj"} (distinct ids))]
  ^{:line 356 :file "cli/agents-cli.bclj"} (cond
  ^{:line 359 :file "cli/agents-cli.bclj"} (empty? ids) ^{:line 359 :file "cli/agents-cli.bclj"} {:agents ^{:line 359 :file "cli/agents-cli.bclj"} {} :sessions ^{:line 359 :file "cli/agents-cli.bclj"} {}}
  ^{:line 361 :file "cli/agents-cli.bclj"} (or ^{:line 361 :file "cli/agents-cli.bclj"} (> ^{:line 361 :file "cli/agents-cli.bclj"} (count ids) max-live-controls) ^{:line 362 :file "cli/agents-cli.bclj"} (not-every? valid-control-id? ids)) ^{:line 363 :file "cli/agents-cli.bclj"} {:err "liveness lease query returned an invalid or over-broad control set"}
  :else ^{:line 366 :file "cli/agents-cli.bclj"} (let [subjects ^{:line 366 :file "cli/agents-cli.bclj"} (mapv ^{:line 366 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 366 :file "cli/agents-cli.bclj"} (str "@agent:" %1)) ids)
   allowed-subjects ^{:line 367 :file "cli/agents-cli.bclj"} (set subjects)]
  ^{:line 368 :file "cli/agents-cli.bclj"} (let [response ^{:line 368 :file "cli/agents-cli.bclj"} (try
  ^{:line 369 :file "cli/agents-cli.bclj"} (north.coord/show-many-in-domain! ^{:line 370 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) :coordination subjects)
  (catch Exception _
    ::unavailable))]
  ^{:line 372 :file "cli/agents-cli.bclj"} (cond
  ^{:line 373 :file "cli/agents-cli.bclj"} (= ::unavailable response) ^{:line 374 :file "cli/agents-cli.bclj"} {:err "agent subject projection unavailable"}
  ^{:line 376 :file "cli/agents-cli.bclj"} (not ^{:line 376 :file "cli/agents-cli.bclj"} (and ^{:line 376 :file "cli/agents-cli.bclj"} (map? response) ^{:line 377 :file "cli/agents-cli.bclj"} (integer? ^{:line 377 :file "cli/agents-cli.bclj"} (:version response)) ^{:line 378 :file "cli/agents-cli.bclj"} (not ^{:line 378 :file "cli/agents-cli.bclj"} (neg? ^{:line 378 :file "cli/agents-cli.bclj"} (:version response))) ^{:line 379 :file "cli/agents-cli.bclj"} (map? ^{:line 379 :file "cli/agents-cli.bclj"} (:rows response)))) ^{:line 380 :file "cli/agents-cli.bclj"} {:err "agent subject projection was malformed"}
  :else ^{:line 383 :file "cli/agents-cli.bclj"} (try
  ^{:line 384 :file "cli/agents-cli.bclj"} (fold-roster-subjects ^{:line 384 :file "cli/agents-cli.bclj"} (:rows response) allowed-subjects)
  (catch Exception _
    ^{:line 386 :file "cli/agents-cli.bclj"} {:err "agent subject projection was malformed"}))))))))

^{:line 388 :file "cli/agents-cli.bclj"} (defn- roster-run-entries-attempt
  "Resolve run candidates for IDS with one bounded telemetry query and one\n  batched exact-subject projection." [ids]
  ^{:line 391 :file "cli/agents-cli.bclj"} (try
  ^{:line 392 :file "cli/agents-cli.bclj"} (let [rules ^{:line 392 :file "cli/agents-cli.bclj"} (mapv ^{:line 393 :file "cli/agents-cli.bclj"} (fn [control] ^{:line 394 :file "cli/agents-cli.bclj"} {:head ^{:line 394 :file "cli/agents-cli.bclj"} {:rel "roster_run_candidate" :args ^{:line 395 :file "cli/agents-cli.bclj"} [^{:line 395 :file "cli/agents-cli.bclj"} {:var "e"}]} :body ^{:line 396 :file "cli/agents-cli.bclj"} [^{:line 396 :file "cli/agents-cli.bclj"} {:rel "triple" :args ^{:line 397 :file "cli/agents-cli.bclj"} [^{:line 397 :file "cli/agents-cli.bclj"} {:var "e"} "agent" control]}]}) ids)
   response ^{:line 399 :file "cli/agents-cli.bclj"} (north.coord/bounded-query-in-domain! ^{:line 400 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) :telemetry ^{:line 402 :file "cli/agents-cli.bclj"} {:find "roster_run_candidate" :rules rules} max-roster-run-candidates)
   rows ^{:line 404 :file "cli/agents-cli.bclj"} (:rows response)]
  ^{:line 405 :file "cli/agents-cli.bclj"} (if ^{:line 406 :file "cli/agents-cli.bclj"} (and ^{:line 406 :file "cli/agents-cli.bclj"} (map? response) ^{:line 406 :file "cli/agents-cli.bclj"} (vector? rows) ^{:line 407 :file "cli/agents-cli.bclj"} (<= ^{:line 407 :file "cli/agents-cli.bclj"} (count rows) max-roster-run-candidates) ^{:line 408 :file "cli/agents-cli.bclj"} (every? ^{:line 408 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 408 :file "cli/agents-cli.bclj"} (and ^{:line 408 :file "cli/agents-cli.bclj"} (vector? %1) ^{:line 408 :file "cli/agents-cli.bclj"} (= 1 ^{:line 408 :file "cli/agents-cli.bclj"} (count %1)) ^{:line 408 :file "cli/agents-cli.bclj"} (every? string? %1))) rows)) ^{:line 412 :file "cli/agents-cli.bclj"} (let [subjects ^{:line 412 :file "cli/agents-cli.bclj"} (->> rows ^{:line 413 :file "cli/agents-cli.bclj"} (map first) ^{:line 414 :file "cli/agents-cli.bclj"} (filter north.terminal-projection/valid-run-entity?) distinct sort vec)
   projected ^{:line 418 :file "cli/agents-cli.bclj"} (if ^{:line 418 :file "cli/agents-cli.bclj"} (seq subjects) ^{:line 419 :file "cli/agents-cli.bclj"} (north.coord/show-many-in-domain! ^{:line 420 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) :telemetry subjects) ^{:line 421 :file "cli/agents-cli.bclj"} {:version ^{:line 421 :file "cli/agents-cli.bclj"} (:served-version response) :rows ^{:line 421 :file "cli/agents-cli.bclj"} {}})
   rows-by-subject ^{:line 422 :file "cli/agents-cli.bclj"} (:rows projected)
   _ ^{:line 423 :file "cli/agents-cli.bclj"} (if ^{:line 423 :file "cli/agents-cli.bclj"} (not ^{:line 424 :file "cli/agents-cli.bclj"} (and ^{:line 424 :file "cli/agents-cli.bclj"} (map? projected) ^{:line 425 :file "cli/agents-cli.bclj"} (integer? ^{:line 425 :file "cli/agents-cli.bclj"} (:version projected)) ^{:line 426 :file "cli/agents-cli.bclj"} (not ^{:line 426 :file "cli/agents-cli.bclj"} (neg? ^{:line 426 :file "cli/agents-cli.bclj"} (:version projected))) ^{:line 427 :file "cli/agents-cli.bclj"} (map? rows-by-subject) ^{:line 428 :file "cli/agents-cli.bclj"} (every? ^{:line 429 :file "cli/agents-cli.bclj"} (fn [[subject fact-rows]] ^{:line 430 :file "cli/agents-cli.bclj"} (and ^{:line 430 :file "cli/agents-cli.bclj"} (contains? ^{:line 430 :file "cli/agents-cli.bclj"} (set subjects) subject) ^{:line 431 :file "cli/agents-cli.bclj"} (vector? fact-rows) ^{:line 432 :file "cli/agents-cli.bclj"} (every? ^{:line 432 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 432 :file "cli/agents-cli.bclj"} (and ^{:line 432 :file "cli/agents-cli.bclj"} (vector? %1) ^{:line 432 :file "cli/agents-cli.bclj"} (= 2 ^{:line 432 :file "cli/agents-cli.bclj"} (count %1)) ^{:line 432 :file "cli/agents-cli.bclj"} (every? string? %1))) fact-rows))) rows-by-subject))) ^{:line 423 :file "cli/agents-cli.bclj"} (do
  ^{:line 437 :file "cli/agents-cli.bclj"} (throw ^{:line 437 :file "cli/agents-cli.bclj"} (ex-info "run subject projection was malformed" ^{:line 437 :file "cli/agents-cli.bclj"} {}))))
   entries ^{:line 438 :file "cli/agents-cli.bclj"} (mapv ^{:line 439 :file "cli/agents-cli.bclj"} (fn [subject] ^{:line 440 :file "cli/agents-cli.bclj"} {:subject subject :facts ^{:line 442 :file "cli/agents-cli.bclj"} (reduce ^{:line 442 :file "cli/agents-cli.bclj"} (fn [facts [predicate value]] ^{:line 445 :file "cli/agents-cli.bclj"} (if ^{:line 445 :file "cli/agents-cli.bclj"} (contains? ^{:line 446 :file "cli/agents-cli.bclj"} (set north.terminal-projection/run-resolution-predicates) predicate) ^{:line 448 :file "cli/agents-cli.bclj"} (update facts predicate ^{:line 448 :file "cli/agents-cli.bclj"} (fnil conj ^{:line 448 :file "cli/agents-cli.bclj"} #{}) value) facts)) ^{:line 450 :file "cli/agents-cli.bclj"} {} ^{:line 451 :file "cli/agents-cli.bclj"} (get rows-by-subject subject ^{:line 451 :file "cli/agents-cli.bclj"} []))}) subjects)]
  ^{:line 453 :file "cli/agents-cli.bclj"} {:ok true :by-agent ^{:line 455 :file "cli/agents-cli.bclj"} (into ^{:line 455 :file "cli/agents-cli.bclj"} {} ^{:line 456 :file "cli/agents-cli.bclj"} (map ^{:line 456 :file "cli/agents-cli.bclj"} (fn [control] ^{:line 457 :file "cli/agents-cli.bclj"} [control ^{:line 458 :file "cli/agents-cli.bclj"} (filterv ^{:line 459 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 459 :file "cli/agents-cli.bclj"} (contains? ^{:line 459 :file "cli/agents-cli.bclj"} (get-in %1 ^{:line 459 :file "cli/agents-cli.bclj"} [:facts "agent"] ^{:line 459 :file "cli/agents-cli.bclj"} #{}) control)) entries)])) ids)}) ^{:line 411 :file "cli/agents-cli.bclj"} {:ok false :reason :run-projection-malformed}))
  (catch Exception _
    ^{:line 463 :file "cli/agents-cli.bclj"} {:ok false :reason :run-projection-unavailable})))

^{:line 465 :file "cli/agents-cli.bclj"} (defn roster-run-entries
  "Read run candidates for every live control in one bounded attempt." [ids]
  ^{:line 467 :file "cli/agents-cli.bclj"} (let [ids ^{:line 467 :file "cli/agents-cli.bclj"} (vec ^{:line 467 :file "cli/agents-cli.bclj"} (distinct ids))]
  ^{:line 468 :file "cli/agents-cli.bclj"} (if ^{:line 468 :file "cli/agents-cli.bclj"} (empty? ids) ^{:line 469 :file "cli/agents-cli.bclj"} {:ok true :by-agent ^{:line 469 :file "cli/agents-cli.bclj"} {}} ^{:line 470 :file "cli/agents-cli.bclj"} (roster-run-entries-attempt ids))))

^{:line 472 :file "cli/agents-cli.bclj"} (defn attach-lane-resolutions [ids agents run-projection]
  ^{:line 476 :file "cli/agents-cli.bclj"} (into ^{:line 476 :file "cli/agents-cli.bclj"} {} ^{:line 477 :file "cli/agents-cli.bclj"} (map ^{:line 478 :file "cli/agents-cli.bclj"} (fn [control] ^{:line 479 :file "cli/agents-cli.bclj"} (let [facts ^{:line 479 :file "cli/agents-cli.bclj"} (get agents control ^{:line 479 :file "cli/agents-cli.bclj"} {})
   managed? ^{:line 480 :file "cli/agents-cli.bclj"} (= "lane" ^{:line 480 :file "cli/agents-cli.bclj"} (get facts "kind"))
   agent-projection ^{:line 481 :file "cli/agents-cli.bclj"} (get-in run-projection ^{:line 481 :file "cli/agents-cli.bclj"} [:by-agent control])]
  ^{:line 482 :file "cli/agents-cli.bclj"} [control ^{:line 483 :file "cli/agents-cli.bclj"} (if managed? ^{:line 485 :file "cli/agents-cli.bclj"} (let [resolution ^{:line 485 :file "cli/agents-cli.bclj"} (cond
  ^{:line 486 :file "cli/agents-cli.bclj"} (not ^{:line 486 :file "cli/agents-cli.bclj"} (:ok run-projection)) ^{:line 487 :file "cli/agents-cli.bclj"} {:status :indeterminate :reason ^{:line 487 :file "cli/agents-cli.bclj"} (:reason run-projection)}
  ^{:line 489 :file "cli/agents-cli.bclj"} (map? agent-projection) ^{:line 490 :file "cli/agents-cli.bclj"} {:status :indeterminate :reason ^{:line 490 :file "cli/agents-cli.bclj"} (:err agent-projection)}
  :else ^{:line 493 :file "cli/agents-cli.bclj"} (north.terminal-projection/lane-resolution control facts ^{:line 494 :file "cli/agents-cli.bclj"} (or agent-projection ^{:line 494 :file "cli/agents-cli.bclj"} [])))]
  ^{:line 495 :file "cli/agents-cli.bclj"} (assoc facts lane-resolution-key resolution)) facts)]))) ids))

^{:line 498 :file "cli/agents-cli.bclj"} (defn current-repo []
  ^{:line 499 :file "cli/agents-cli.bclj"} (let [r ^{:line 499 :file "cli/agents-cli.bclj"} (run ^{:line 499 :file "cli/agents-cli.bclj"} ["git" "remote" "get-url" "origin"] :timeout 1500)]
  ^{:line 500 :file "cli/agents-cli.bclj"} (if ^{:line 500 :file "cli/agents-cli.bclj"} (:ok r) ^{:line 501 :file "cli/agents-cli.bclj"} (some-> ^{:line 501 :file "cli/agents-cli.bclj"} (:out r) str/trim ^{:line 501 :file "cli/agents-cli.bclj"} (str/split #"[/:]") last ^{:line 501 :file "cli/agents-cli.bclj"} (str/replace #"\.git$" "")) ^{:line 502 :file "cli/agents-cli.bclj"} (some-> ^{:line 502 :file "cli/agents-cli.bclj"} (System/getProperty "user.dir") ^{:line 502 :file "cli/agents-cli.bclj"} (str/split #"/") last))))

^{:line 504 :file "cli/agents-cli.bclj"} (defn- known [value]
  ^{:line 505 :file "cli/agents-cli.bclj"} (let [s ^{:line 505 :file "cli/agents-cli.bclj"} (some-> value str str/trim)]
  ^{:line 505 :file "cli/agents-cli.bclj"} (if ^{:line 505 :file "cli/agents-cli.bclj"} (seq s) ^{:line 505 :file "cli/agents-cli.bclj"} (do
  s))))

^{:line 507 :file "cli/agents-cli.bclj"} (defn- fact-one [facts predicate]
  ^{:line 510 :file "cli/agents-cli.bclj"} (if ^{:line 510 :file "cli/agents-cli.bclj"} (not ^{:line 510 :file "cli/agents-cli.bclj"} (or ^{:line 510 :file "cli/agents-cli.bclj"} (contains? ^{:line 510 :file "cli/agents-cli.bclj"} (get facts north.agent-provenance/conflict-key ^{:line 510 :file "cli/agents-cli.bclj"} #{}) predicate) ^{:line 512 :file "cli/agents-cli.bclj"} (contains? ^{:line 512 :file "cli/agents-cli.bclj"} (get facts roster-conflict-key ^{:line 512 :file "cli/agents-cli.bclj"} #{}) predicate))) ^{:line 510 :file "cli/agents-cli.bclj"} (do
  ^{:line 513 :file "cli/agents-cli.bclj"} (known ^{:line 513 :file "cli/agents-cli.bclj"} (get facts predicate)))))

^{:line 515 :file "cli/agents-cli.bclj"} (defn- slug [value]
  ^{:line 516 :file "cli/agents-cli.bclj"} (or ^{:line 516 :file "cli/agents-cli.bclj"} (some-> ^{:line 516 :file "cli/agents-cli.bclj"} (known value) str/lower-case ^{:line 517 :file "cli/agents-cli.bclj"} (str/replace #"[^a-z0-9]+" "-") ^{:line 518 :file "cli/agents-cli.bclj"} (str/replace #"(^-|-$)" "") known) "unknown"))

^{:line 522 :file "cli/agents-cli.bclj"} (defn- model-display [model]
  ^{:line 523 :file "cli/agents-cli.bclj"} (let [m ^{:line 523 :file "cli/agents-cli.bclj"} (slug model)
   parts ^{:line 524 :file "cli/agents-cli.bclj"} (set ^{:line 524 :file "cli/agents-cli.bclj"} (str/split m #"-"))]
  ^{:line 525 :file "cli/agents-cli.bclj"} (or ^{:line 525 :file "cli/agents-cli.bclj"} (some ^{:line 525 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 525 :file "cli/agents-cli.bclj"} (if ^{:line 525 :file "cli/agents-cli.bclj"} (parts %1) ^{:line 525 :file "cli/agents-cli.bclj"} (do
  %1))) ^{:line 525 :file "cli/agents-cli.bclj"} ["opus" "sonnet" "haiku" "fable" "sol" "terra" "luna"]) m)))

^{:line 528 :file "cli/agents-cli.bclj"} (defn- meaningful-task [value]
  ^{:line 529 :file "cli/agents-cli.bclj"} (let [task ^{:line 529 :file "cli/agents-cli.bclj"} (known value)]
  ^{:line 530 :file "cli/agents-cli.bclj"} (if ^{:line 530 :file "cli/agents-cli.bclj"} (not ^{:line 530 :file "cli/agents-cli.bclj"} (^{:line 530 :file "cli/agents-cli.bclj"} #{"CONTEXT BRIEF:" "DELEGATE TASK:" "TASK:"} task)) ^{:line 530 :file "cli/agents-cli.bclj"} (do
  task))))

^{:line 532 :file "cli/agents-cli.bclj"} (defn- axis-observation [facts predicate]
  ^{:line 535 :file "cli/agents-cli.bclj"} (if ^{:line 535 :file "cli/agents-cli.bclj"} (= "session" ^{:line 535 :file "cli/agents-cli.bclj"} (fact-one facts "kind")) ^{:line 536 :file "cli/agents-cli.bclj"} (north.agent-provenance/native-axis facts predicate) ^{:line 537 :file "cli/agents-cli.bclj"} {:value ^{:line 537 :file "cli/agents-cli.bclj"} (fact-one facts predicate) :conflict ^{:line 538 :file "cli/agents-cli.bclj"} (or ^{:line 539 :file "cli/agents-cli.bclj"} (contains? ^{:line 539 :file "cli/agents-cli.bclj"} (get facts north.agent-provenance/conflict-key ^{:line 539 :file "cli/agents-cli.bclj"} #{}) predicate) ^{:line 541 :file "cli/agents-cli.bclj"} (contains? ^{:line 541 :file "cli/agents-cli.bclj"} (get facts roster-conflict-key ^{:line 541 :file "cli/agents-cli.bclj"} #{}) predicate))}))

^{:line 543 :file "cli/agents-cli.bclj"} (defn- composition-overrides [facts]
  ^{:line 544 :file "cli/agents-cli.bclj"} (north.agent-provenance/composition-overrides facts))

^{:line 546 :file "cli/agents-cli.bclj"} (defn- orchestration-provenance [facts]
  ^{:line 547 :file "cli/agents-cli.bclj"} (north.agent-provenance/orchestration-provenance facts))

^{:line 549 :file "cli/agents-cli.bclj"} (defn- provider-target-label [facts]
  ^{:line 550 :file "cli/agents-cli.bclj"} (let [provider-observation ^{:line 550 :file "cli/agents-cli.bclj"} (axis-observation facts "provider")
   vendor-observation ^{:line 551 :file "cli/agents-cli.bclj"} (axis-observation facts "vendor")
   target-observation ^{:line 552 :file "cli/agents-cli.bclj"} (axis-observation facts "provider_target")
   provider ^{:line 553 :file "cli/agents-cli.bclj"} (or ^{:line 553 :file "cli/agents-cli.bclj"} (:value provider-observation) ^{:line 553 :file "cli/agents-cli.bclj"} (:value vendor-observation) "unknown")
   target ^{:line 554 :file "cli/agents-cli.bclj"} (:value target-observation)]
  ^{:line 555 :file "cli/agents-cli.bclj"} (cond
  ^{:line 556 :file "cli/agents-cli.bclj"} (or ^{:line 556 :file "cli/agents-cli.bclj"} (:conflict provider-observation) ^{:line 557 :file "cli/agents-cli.bclj"} (and ^{:line 557 :file "cli/agents-cli.bclj"} (nil? ^{:line 557 :file "cli/agents-cli.bclj"} (:value provider-observation)) ^{:line 557 :file "cli/agents-cli.bclj"} (:conflict vendor-observation))) "provider:conflict"
  ^{:line 560 :file "cli/agents-cli.bclj"} (:conflict target-observation) ^{:line 560 :file "cli/agents-cli.bclj"} (str provider ":target-conflict")
  target ^{:line 561 :file "cli/agents-cli.bclj"} (str provider ":" ^{:line 561 :file "cli/agents-cli.bclj"} (if ^{:line 561 :file "cli/agents-cli.bclj"} (or ^{:line 561 :file "cli/agents-cli.bclj"} (= target provider) ^{:line 561 :file "cli/agents-cli.bclj"} (= target "ambient")) "ambient" target))
  :else provider)))

^{:line 565 :file "cli/agents-cli.bclj"} (defn- provider-axis-label [facts]
  ^{:line 566 :file "cli/agents-cli.bclj"} (let [native? ^{:line 566 :file "cli/agents-cli.bclj"} (= "session" ^{:line 566 :file "cli/agents-cli.bclj"} (fact-one facts "kind"))
   provider-observation ^{:line 567 :file "cli/agents-cli.bclj"} (axis-observation facts "provider")
   vendor-observation ^{:line 568 :file "cli/agents-cli.bclj"} (axis-observation facts "vendor")
   provider-conflict? ^{:line 569 :file "cli/agents-cli.bclj"} (or ^{:line 569 :file "cli/agents-cli.bclj"} (:conflict provider-observation) ^{:line 570 :file "cli/agents-cli.bclj"} (and ^{:line 570 :file "cli/agents-cli.bclj"} (nil? ^{:line 570 :file "cli/agents-cli.bclj"} (:value provider-observation)) ^{:line 571 :file "cli/agents-cli.bclj"} (:conflict vendor-observation)))
   provider-value ^{:line 572 :file "cli/agents-cli.bclj"} (or ^{:line 572 :file "cli/agents-cli.bclj"} (:value provider-observation) ^{:line 572 :file "cli/agents-cli.bclj"} (:value vendor-observation))]
  ^{:line 573 :file "cli/agents-cli.bclj"} (cond
  provider-conflict? "provider:conflict"
  ^{:line 575 :file "cli/agents-cli.bclj"} (and native? ^{:line 575 :file "cli/agents-cli.bclj"} (nil? provider-value)) "provider:historical-unrecorded"
  ^{:line 576 :file "cli/agents-cli.bclj"} (= provider-value "unobserved") "provider:unobserved"
  :else ^{:line 577 :file "cli/agents-cli.bclj"} (provider-target-label facts))))

^{:line 579 :file "cli/agents-cli.bclj"} (defn- model-axis-label [facts]
  ^{:line 580 :file "cli/agents-cli.bclj"} (let [native? ^{:line 580 :file "cli/agents-cli.bclj"} (= "session" ^{:line 580 :file "cli/agents-cli.bclj"} (fact-one facts "kind"))
   observation ^{:line 581 :file "cli/agents-cli.bclj"} (axis-observation facts "model")
   value ^{:line 582 :file "cli/agents-cli.bclj"} (:value observation)]
  ^{:line 583 :file "cli/agents-cli.bclj"} (cond
  ^{:line 584 :file "cli/agents-cli.bclj"} (:conflict observation) "model:conflict"
  ^{:line 585 :file "cli/agents-cli.bclj"} (and native? ^{:line 585 :file "cli/agents-cli.bclj"} (nil? value)) "model:historical-unrecorded"
  ^{:line 586 :file "cli/agents-cli.bclj"} (= value "unobserved") "model:unobserved"
  :else ^{:line 587 :file "cli/agents-cli.bclj"} (model-display ^{:line 587 :file "cli/agents-cli.bclj"} (or value "unknown")))))

^{:line 589 :file "cli/agents-cli.bclj"} (defn- effort-axis-label [facts]
  ^{:line 590 :file "cli/agents-cli.bclj"} (let [native? ^{:line 590 :file "cli/agents-cli.bclj"} (= "session" ^{:line 590 :file "cli/agents-cli.bclj"} (fact-one facts "kind"))
   observation ^{:line 591 :file "cli/agents-cli.bclj"} (axis-observation facts "effort")
   value ^{:line 592 :file "cli/agents-cli.bclj"} (:value observation)]
  ^{:line 593 :file "cli/agents-cli.bclj"} (cond
  ^{:line 594 :file "cli/agents-cli.bclj"} (:conflict observation) "effort:conflict"
  ^{:line 595 :file "cli/agents-cli.bclj"} (and native? ^{:line 595 :file "cli/agents-cli.bclj"} (nil? value)) "effort:historical-unrecorded"
  ^{:line 596 :file "cli/agents-cli.bclj"} (= value "unobserved") "effort:unobserved"
  :else ^{:line 597 :file "cli/agents-cli.bclj"} (slug ^{:line 597 :file "cli/agents-cli.bclj"} (or value "unknown")))))

^{:line 599 :file "cli/agents-cli.bclj"} (defn- raw-provider [facts]
  ^{:line 600 :file "cli/agents-cli.bclj"} (let [provider ^{:line 600 :file "cli/agents-cli.bclj"} (axis-observation facts "provider")
   vendor ^{:line 601 :file "cli/agents-cli.bclj"} (axis-observation facts "vendor")]
  ^{:line 602 :file "cli/agents-cli.bclj"} (if ^{:line 602 :file "cli/agents-cli.bclj"} (or ^{:line 602 :file "cli/agents-cli.bclj"} (:conflict provider) ^{:line 603 :file "cli/agents-cli.bclj"} (and ^{:line 603 :file "cli/agents-cli.bclj"} (nil? ^{:line 603 :file "cli/agents-cli.bclj"} (:value provider)) ^{:line 603 :file "cli/agents-cli.bclj"} (:conflict vendor))) "conflict" ^{:line 605 :file "cli/agents-cli.bclj"} (or ^{:line 605 :file "cli/agents-cli.bclj"} (:value provider) ^{:line 605 :file "cli/agents-cli.bclj"} (:value vendor) ""))))

^{:line 607 :file "cli/agents-cli.bclj"} (defn- raw-provider-target [facts]
  ^{:line 608 :file "cli/agents-cli.bclj"} (let [observation ^{:line 608 :file "cli/agents-cli.bclj"} (axis-observation facts "provider_target")]
  ^{:line 609 :file "cli/agents-cli.bclj"} (if ^{:line 609 :file "cli/agents-cli.bclj"} (:conflict observation) "conflict" ^{:line 609 :file "cli/agents-cli.bclj"} (or ^{:line 609 :file "cli/agents-cli.bclj"} (:value observation) ""))))

^{:line 611 :file "cli/agents-cli.bclj"} (defn- raw-model [facts]
  ^{:line 612 :file "cli/agents-cli.bclj"} (let [observation ^{:line 612 :file "cli/agents-cli.bclj"} (axis-observation facts "model")]
  ^{:line 613 :file "cli/agents-cli.bclj"} (if ^{:line 613 :file "cli/agents-cli.bclj"} (:conflict observation) "conflict" ^{:line 613 :file "cli/agents-cli.bclj"} (or ^{:line 613 :file "cli/agents-cli.bclj"} (:value observation) ""))))

^{:line 615 :file "cli/agents-cli.bclj"} (defn- live-input-label [facts]
  ^{:line 616 :file "cli/agents-cli.bclj"} (let [observation ^{:line 616 :file "cli/agents-cli.bclj"} (axis-observation facts "live_input")]
  ^{:line 617 :file "cli/agents-cli.bclj"} (cond
  ^{:line 618 :file "cli/agents-cli.bclj"} (:conflict observation) "conflict"
  ^{:line 619 :file "cli/agents-cli.bclj"} (^{:line 619 :file "cli/agents-cli.bclj"} #{"streaming" "turn-messages" "unsupported"} ^{:line 619 :file "cli/agents-cli.bclj"} (:value observation)) ^{:line 619 :file "cli/agents-cli.bclj"} (:value observation)
  :else "unrecorded")))

^{:line 622 :file "cli/agents-cli.bclj"} (defn- live-input-state-label [facts]
  ^{:line 623 :file "cli/agents-cli.bclj"} (let [observation ^{:line 623 :file "cli/agents-cli.bclj"} (axis-observation facts "live_input_state")]
  ^{:line 624 :file "cli/agents-cli.bclj"} (cond
  ^{:line 625 :file "cli/agents-cli.bclj"} (:conflict observation) "conflict"
  ^{:line 626 :file "cli/agents-cli.bclj"} (^{:line 626 :file "cli/agents-cli.bclj"} #{"pending" "armed" "frozen"} ^{:line 626 :file "cli/agents-cli.bclj"} (:value observation)) ^{:line 626 :file "cli/agents-cli.bclj"} (:value observation)
  :else "unrecorded")))

^{:line 629 :file "cli/agents-cli.bclj"} (defn- task-of [presence facts session]
  ^{:line 633 :file "cli/agents-cli.bclj"} (or ^{:line 633 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 633 :file "cli/agents-cli.bclj"} (fact-one session "current_referent")) ^{:line 634 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 634 :file "cli/agents-cli.bclj"} (fact-one session "active_workflow")) ^{:line 635 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 635 :file "cli/agents-cli.bclj"} (fact-one session "task")) ^{:line 636 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 636 :file "cli/agents-cli.bclj"} (fact-one facts "current_referent")) ^{:line 637 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 637 :file "cli/agents-cli.bclj"} (fact-one facts "active_workflow")) ^{:line 638 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 638 :file "cli/agents-cli.bclj"} (fact-one facts "task")) ^{:line 639 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 639 :file "cli/agents-cli.bclj"} (fact-one facts "goal")) ^{:line 640 :file "cli/agents-cli.bclj"} (meaningful-task ^{:line 640 :file "cli/agents-cli.bclj"} (:focus presence)) ^{:line 641 :file "cli/agents-cli.bclj"} (if ^{:line 641 :file "cli/agents-cli.bclj"} (and ^{:line 641 :file "cli/agents-cli.bclj"} (= "session" ^{:line 641 :file "cli/agents-cli.bclj"} (fact-one facts "kind")) ^{:line 642 :file "cli/agents-cli.bclj"} (fact-one facts "repo")) ^{:line 641 :file "cli/agents-cli.bclj"} (do
  ^{:line 643 :file "cli/agents-cli.bclj"} (str "native session in " ^{:line 643 :file "cli/agents-cli.bclj"} (fact-one facts "repo")))) "unknown"))

^{:line 646 :file "cli/agents-cli.bclj"} (defn- terminal-state [presence facts]
  ^{:line 649 :file "cli/agents-cli.bclj"} (let [resolution ^{:line 649 :file "cli/agents-cli.bclj"} (or ^{:line 649 :file "cli/agents-cli.bclj"} (get facts lane-resolution-key) ^{:line 650 :file "cli/agents-cli.bclj"} (north.terminal-projection/lane-resolution ^{:line 651 :file "cli/agents-cli.bclj"} (:id presence) facts ^{:line 651 :file "cli/agents-cli.bclj"} []))
   process-outcome ^{:line 652 :file "cli/agents-cli.bclj"} (if ^{:line 652 :file "cli/agents-cli.bclj"} (= :resolved ^{:line 652 :file "cli/agents-cli.bclj"} (:status resolution)) ^{:line 652 :file "cli/agents-cli.bclj"} (do
  ^{:line 653 :file "cli/agents-cli.bclj"} (:outcome resolution)))
   delivery-outcome ^{:line 654 :file "cli/agents-cli.bclj"} (if ^{:line 654 :file "cli/agents-cli.bclj"} (= :resolved ^{:line 654 :file "cli/agents-cli.bclj"} (:status resolution)) ^{:line 654 :file "cli/agents-cli.bclj"} (do
  ^{:line 655 :file "cli/agents-cli.bclj"} (:delivery-outcome resolution)))
   delivery-label ^{:line 656 :file "cli/agents-cli.bclj"} (or delivery-outcome "unrecorded")
   state ^{:line 657 :file "cli/agents-cli.bclj"} (cond
  process-outcome "finished"
  ^{:line 659 :file "cli/agents-cli.bclj"} (= :indeterminate ^{:line 659 :file "cli/agents-cli.bclj"} (:status resolution)) "inconsistent"
  ^{:line 660 :file "cli/agents-cli.bclj"} (fact-one facts "stalled") "stalled"
  ^{:line 661 :file "cli/agents-cli.bclj"} (:online presence) "working"
  :else "offline")
   state-label ^{:line 663 :file "cli/agents-cli.bclj"} (cond
  process-outcome ^{:line 665 :file "cli/agents-cli.bclj"} (str "finished(process:" process-outcome ", delivery:" delivery-label ")")
  ^{:line 667 :file "cli/agents-cli.bclj"} (= :indeterminate ^{:line 667 :file "cli/agents-cli.bclj"} (:status resolution)) ^{:line 668 :file "cli/agents-cli.bclj"} (str "inconsistent(lifecycle:" ^{:line 668 :file "cli/agents-cli.bclj"} (name ^{:line 668 :file "cli/agents-cli.bclj"} (:reason resolution)) ")")
  :else state)]
  ^{:line 671 :file "cli/agents-cli.bclj"} {:process-outcome ^{:line 671 :file "cli/agents-cli.bclj"} (or process-outcome "") :delivery-outcome ^{:line 672 :file "cli/agents-cli.bclj"} (or delivery-outcome "") :resolution-status ^{:line 673 :file "cli/agents-cli.bclj"} (:status resolution) :resolution-reason ^{:line 674 :file "cli/agents-cli.bclj"} (:reason resolution) :state state :state-label state-label}))

^{:line 678 :file "cli/agents-cli.bclj"} (defn- role-axis [facts]
  ^{:line 679 :file "cli/agents-cli.bclj"} (if ^{:line 679 :file "cli/agents-cli.bclj"} (and ^{:line 679 :file "cli/agents-cli.bclj"} (fact-one facts "role") ^{:line 680 :file "cli/agents-cli.bclj"} (not ^{:line 680 :file "cli/agents-cli.bclj"} (contains? ^{:line 680 :file "cli/agents-cli.bclj"} #{"template" "bespoke"} ^{:line 681 :file "cli/agents-cli.bclj"} (fact-one facts "composition_kind")))) ^{:line 679 :file "cli/agents-cli.bclj"} (do
  ^{:line 682 :file "cli/agents-cli.bclj"} (str " · role:" ^{:line 682 :file "cli/agents-cli.bclj"} (slug ^{:line 682 :file "cli/agents-cli.bclj"} (fact-one facts "role"))))))

^{:line 684 :file "cli/agents-cli.bclj"} (defn semantic-handle [id facts]
  ^{:line 687 :file "cli/agents-cli.bclj"} (let [provider-axis ^{:line 687 :file "cli/agents-cli.bclj"} (provider-target-label facts)
   composition ^{:line 688 :file "cli/agents-cli.bclj"} (orchestration-provenance facts)
   model-observation ^{:line 689 :file "cli/agents-cli.bclj"} (axis-observation facts "model")
   effort-observation ^{:line 690 :file "cli/agents-cli.bclj"} (axis-observation facts "effort")
   model ^{:line 691 :file "cli/agents-cli.bclj"} (if ^{:line 691 :file "cli/agents-cli.bclj"} (:conflict model-observation) "model:conflict" ^{:line 692 :file "cli/agents-cli.bclj"} (:value model-observation))
   effort ^{:line 693 :file "cli/agents-cli.bclj"} (if ^{:line 693 :file "cli/agents-cli.bclj"} (:conflict effort-observation) "effort:conflict" ^{:line 694 :file "cli/agents-cli.bclj"} (:value effort-observation))
   suffix ^{:line 695 :file "cli/agents-cli.bclj"} (last ^{:line 695 :file "cli/agents-cli.bclj"} (str/split ^{:line 695 :file "cli/agents-cli.bclj"} (str id) #"-"))]
  ^{:line 698 :file "cli/agents-cli.bclj"} (str/join "-" ^{:line 698 :file "cli/agents-cli.bclj"} [^{:line 698 :file "cli/agents-cli.bclj"} (slug provider-axis) ^{:line 698 :file "cli/agents-cli.bclj"} (model-display model) ^{:line 699 :file "cli/agents-cli.bclj"} (slug effort) ^{:line 699 :file "cli/agents-cli.bclj"} (slug composition) ^{:line 699 :file "cli/agents-cli.bclj"} (slug suffix)])))

^{:line 701 :file "cli/agents-cli.bclj"} (defn render-display-name [id facts]
  ^{:line 704 :file "cli/agents-cli.bclj"} (let [goal ^{:line 704 :file "cli/agents-cli.bclj"} (known ^{:line 704 :file "cli/agents-cli.bclj"} (get facts "goal"))
   g ^{:line 705 :file "cli/agents-cli.bclj"} (if goal ^{:line 705 :file "cli/agents-cli.bclj"} (do
  ^{:line 705 :file "cli/agents-cli.bclj"} (str " — " ^{:line 705 :file "cli/agents-cli.bclj"} (if ^{:line 705 :file "cli/agents-cli.bclj"} (> ^{:line 705 :file "cli/agents-cli.bclj"} (count goal) 40) ^{:line 705 :file "cli/agents-cli.bclj"} (str ^{:line 705 :file "cli/agents-cli.bclj"} (subs goal 0 37) "…") goal))))]
  ^{:line 706 :file "cli/agents-cli.bclj"} (str ^{:line 706 :file "cli/agents-cli.bclj"} (semantic-handle id facts) g)))

^{:line 708 :file "cli/agents-cli.bclj"} (defn agent-primary-line
  ([presence facts]
    ^{:line 711 :file "cli/agents-cli.bclj"} (agent-primary-line presence facts ^{:line 711 :file "cli/agents-cli.bclj"} {}))
  ([presence facts session]
    ^{:line 715 :file "cli/agents-cli.bclj"} (let [task ^{:line 715 :file "cli/agents-cli.bclj"} (task-of presence facts session)
   state ^{:line 716 :file "cli/agents-cli.bclj"} (:state-label ^{:line 716 :file "cli/agents-cli.bclj"} (terminal-state presence facts))]
  ^{:line 717 :file "cli/agents-cli.bclj"} (str ^{:line 717 :file "cli/agents-cli.bclj"} (provider-axis-label facts) " · " ^{:line 717 :file "cli/agents-cli.bclj"} (model-axis-label facts) " · " ^{:line 718 :file "cli/agents-cli.bclj"} (effort-axis-label facts) " · " ^{:line 718 :file "cli/agents-cli.bclj"} (orchestration-provenance facts) ^{:line 719 :file "cli/agents-cli.bclj"} (role-axis facts) " · " state ": " task))))

^{:line 721 :file "cli/agents-cli.bclj"} (defn roster-json-row [presence facts session]
  ^{:line 725 :file "cli/agents-cli.bclj"} (let [{:keys [process-outcome delivery-outcome state state-label resolution-status resolution-reason]} ^{:line 725 :file "cli/agents-cli.bclj"} (terminal-state presence facts)
   task ^{:line 726 :file "cli/agents-cli.bclj"} (task-of presence facts session)
   control ^{:line 727 :file "cli/agents-cli.bclj"} (:id presence)]
  ^{:line 728 :file "cli/agents-cli.bclj"} {"uuid" control "control_id" control "display_name" ^{:line 730 :file "cli/agents-cli.bclj"} (agent-primary-line presence facts session) "display_handle" ^{:line 731 :file "cli/agents-cli.bclj"} (semantic-handle control facts) "kind" ^{:line 732 :file "cli/agents-cli.bclj"} (or ^{:line 732 :file "cli/agents-cli.bclj"} (fact-one facts "kind") "unclassified") "provider" ^{:line 733 :file "cli/agents-cli.bclj"} (raw-provider facts) "provider_target" ^{:line 734 :file "cli/agents-cli.bclj"} (raw-provider-target facts) "provider_label" ^{:line 735 :file "cli/agents-cli.bclj"} (provider-axis-label facts) "live_input" ^{:line 736 :file "cli/agents-cli.bclj"} (live-input-label facts) "live_input_state" ^{:line 737 :file "cli/agents-cli.bclj"} (live-input-state-label facts) "live_input_epoch" ^{:line 738 :file "cli/agents-cli.bclj"} (or ^{:line 738 :file "cli/agents-cli.bclj"} (fact-one facts "live_input_epoch") "") "model" ^{:line 739 :file "cli/agents-cli.bclj"} (raw-model facts) "model_display" ^{:line 740 :file "cli/agents-cli.bclj"} (model-axis-label facts) "effort" ^{:line 741 :file "cli/agents-cli.bclj"} (effort-axis-label facts) "orchestration_provenance" ^{:line 742 :file "cli/agents-cli.bclj"} (orchestration-provenance facts) "goal" ^{:line 743 :file "cli/agents-cli.bclj"} (or ^{:line 743 :file "cli/agents-cli.bclj"} (fact-one facts "goal") "") "task" task "state" state "state_label" state-label "lifecycle" state "lifecycle_resolution" ^{:line 748 :file "cli/agents-cli.bclj"} (name resolution-status) "lifecycle_reason" ^{:line 749 :file "cli/agents-cli.bclj"} (if resolution-reason ^{:line 749 :file "cli/agents-cli.bclj"} (name resolution-reason) "") "process_outcome" process-outcome "delivery_outcome" delivery-outcome "online" ^{:line 752 :file "cli/agents-cli.bclj"} (boolean ^{:line 752 :file "cli/agents-cli.bclj"} (:online presence)) "expires_s" ^{:line 753 :file "cli/agents-cli.bclj"} (:expires-s presence)}))

^{:line 755 :file "cli/agents-cli.bclj"} (defn roster-category [facts]
  ^{:line 756 :file "cli/agents-cli.bclj"} (let [resolution ^{:line 756 :file "cli/agents-cli.bclj"} (or ^{:line 756 :file "cli/agents-cli.bclj"} (get facts lane-resolution-key) ^{:line 757 :file "cli/agents-cli.bclj"} (north.terminal-projection/lane-resolution nil facts ^{:line 757 :file "cli/agents-cli.bclj"} []))]
  ^{:line 758 :file "cli/agents-cli.bclj"} (cond
  ^{:line 759 :file "cli/agents-cli.bclj"} (= :resolved ^{:line 759 :file "cli/agents-cli.bclj"} (:status resolution)) :recently-finished
  ^{:line 760 :file "cli/agents-cli.bclj"} (= :indeterminate ^{:line 760 :file "cli/agents-cli.bclj"} (:status resolution)) :inconsistent
  ^{:line 761 :file "cli/agents-cli.bclj"} (= "lane" ^{:line 761 :file "cli/agents-cli.bclj"} (fact-one facts "kind")) :active-agent
  ^{:line 762 :file "cli/agents-cli.bclj"} (= "session" ^{:line 762 :file "cli/agents-cli.bclj"} (fact-one facts "kind")) :native-session
  :else :unclassified)))

^{:line 766 :file "cli/agents-cli.bclj"} (defn presence-rows []
  ^{:line 767 :file "cli/agents-cli.bclj"} (try
  ^{:line 768 :file "cli/agents-cli.bclj"} (let [port ^{:line 768 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT)
   now ^{:line 769 :file "cli/agents-cli.bclj"} (System/currentTimeMillis)
   sessions ^{:line 770 :file "cli/agents-cli.bclj"} (north.coord/online-session-leases! port now)
   valid? ^{:line 771 :file "cli/agents-cli.bclj"} (and ^{:line 771 :file "cli/agents-cli.bclj"} (vector? sessions) ^{:line 772 :file "cli/agents-cli.bclj"} (<= ^{:line 772 :file "cli/agents-cli.bclj"} (count sessions) max-live-controls) ^{:line 773 :file "cli/agents-cli.bclj"} (every? ^{:line 773 :file "cli/agents-cli.bclj"} (fn [session] ^{:line 774 :file "cli/agents-cli.bclj"} (and ^{:line 774 :file "cli/agents-cli.bclj"} (= ^{:line 774 :file "cli/agents-cli.bclj"} #{:handle :exp} ^{:line 774 :file "cli/agents-cli.bclj"} (set ^{:line 774 :file "cli/agents-cli.bclj"} (keys session))) ^{:line 775 :file "cli/agents-cli.bclj"} (valid-control-id? ^{:line 775 :file "cli/agents-cli.bclj"} (:handle session)) ^{:line 776 :file "cli/agents-cli.bclj"} (integer? ^{:line 776 :file "cli/agents-cli.bclj"} (:exp session)) ^{:line 777 :file "cli/agents-cli.bclj"} (> ^{:line 777 :file "cli/agents-cli.bclj"} (:exp session) now))) sessions) ^{:line 779 :file "cli/agents-cli.bclj"} (= ^{:line 779 :file "cli/agents-cli.bclj"} (count sessions) ^{:line 780 :file "cli/agents-cli.bclj"} (count ^{:line 780 :file "cli/agents-cli.bclj"} (set ^{:line 780 :file "cli/agents-cli.bclj"} (map ^{:line 780 :file "cli/agents-cli.bclj"} (fn [session] ^{:line 780 :file "cli/agents-cli.bclj"} (:handle session)) sessions)))))]
  ^{:line 782 :file "cli/agents-cli.bclj"} (if valid? ^{:line 784 :file "cli/agents-cli.bclj"} {:agents ^{:line 785 :file "cli/agents-cli.bclj"} (mapv ^{:line 785 :file "cli/agents-cli.bclj"} (fn [{:keys [handle exp]}] ^{:line 786 :file "cli/agents-cli.bclj"} (let [expires-s ^{:line 786 :file "cli/agents-cli.bclj"} (quot ^{:line 786 :file "cli/agents-cli.bclj"} (- ^{:line 786 :file "cli/agents-cli.bclj"} (long exp) now) 1000)]
  ^{:line 787 :file "cli/agents-cli.bclj"} {:id handle :online true :expires-s expires-s :expires ^{:line 788 :file "cli/agents-cli.bclj"} (str expires-s "s")})) sessions)} ^{:line 783 :file "cli/agents-cli.bclj"} {:err "liveness lease projection was malformed"}))
  (catch Exception _
    ^{:line 790 :file "cli/agents-cli.bclj"} {:err "liveness lease projection unavailable"})))

^{:line 792 :file "cli/agents-cli.bclj"} (defn agent-online? [id]
  ^{:line 793 :file "cli/agents-cli.bclj"} (try
  ^{:line 794 :file "cli/agents-cli.bclj"} (north.coord/session-online?! ^{:line 794 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) id)
  (catch Exception _
    false)))

^{:line 798 :file "cli/agents-cli.bclj"} (defn agents-usage []
  ^{:line 799 :file "cli/agents-cli.bclj"} (println "north agent list — provider-neutral live roster")
  ^{:line 800 :file "cli/agents-cli.bclj"} (println)
  ^{:line 801 :file "cli/agents-cli.bclj"} (println "Usage:")
  ^{:line 802 :file "cli/agents-cli.bclj"} (println "  north agent list")
  ^{:line 803 :file "cli/agents-cli.bclj"} (println "  north agent list --verbose")
  ^{:line 804 :file "cli/agents-cli.bclj"} (println "  north agent list --json")
  ^{:line 805 :file "cli/agents-cli.bclj"} (println)
  ^{:line 806 :file "cli/agents-cli.bclj"} (println "--json emits the versioned north:agent-roster:v1 machine contract."))

^{:line 809 :file "cli/agents-cli.bclj"} (defn- agents-error! [message]
  ^{:line 810 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 811 :file "cli/agents-cli.bclj"} (println ^{:line 811 :file "cli/agents-cli.bclj"} (str "north agent list: " message))
  ^{:line 812 :file "cli/agents-cli.bclj"} (println "run 'north agent list --help' for usage"))
  ^{:line 813 :file "cli/agents-cli.bclj"} (System/exit 1))

^{:line 815 :file "cli/agents-cli.bclj"} (defn- parse-agents-options! [args]
  ^{:line 816 :file "cli/agents-cli.bclj"} (loop [remaining ^{:line 816 :file "cli/agents-cli.bclj"} (vec args)
   options ^{:line 817 :file "cli/agents-cli.bclj"} {:mode :human :verbose false}]
  ^{:line 818 :file "cli/agents-cli.bclj"} (if ^{:line 818 :file "cli/agents-cli.bclj"} (empty? remaining) options ^{:line 820 :file "cli/agents-cli.bclj"} (let [[arg & more] remaining]
  ^{:line 821 :file "cli/agents-cli.bclj"} (cond
  ^{:line 822 :file "cli/agents-cli.bclj"} (^{:line 822 :file "cli/agents-cli.bclj"} #{"--help" "-h" "help"} arg) ^{:line 823 :file "cli/agents-cli.bclj"} (if ^{:line 823 :file "cli/agents-cli.bclj"} (empty? more) ^{:line 823 :file "cli/agents-cli.bclj"} (assoc options :help true) ^{:line 824 :file "cli/agents-cli.bclj"} (agents-error! "help cannot be combined with other options"))
  ^{:line 826 :file "cli/agents-cli.bclj"} (^{:line 826 :file "cli/agents-cli.bclj"} #{"--verbose" "--debug"} arg) ^{:line 827 :file "cli/agents-cli.bclj"} (if ^{:line 827 :file "cli/agents-cli.bclj"} (or ^{:line 827 :file "cli/agents-cli.bclj"} (:verbose options) ^{:line 827 :file "cli/agents-cli.bclj"} (not= :human ^{:line 827 :file "cli/agents-cli.bclj"} (:mode options))) ^{:line 828 :file "cli/agents-cli.bclj"} (agents-error! ^{:line 828 :file "cli/agents-cli.bclj"} (str "conflicting or duplicate option " arg)) ^{:line 829 :file "cli/agents-cli.bclj"} (recur ^{:line 829 :file "cli/agents-cli.bclj"} (vec more) ^{:line 829 :file "cli/agents-cli.bclj"} (assoc options :verbose true)))
  ^{:line 831 :file "cli/agents-cli.bclj"} (= "--json" arg) ^{:line 832 :file "cli/agents-cli.bclj"} (if ^{:line 832 :file "cli/agents-cli.bclj"} (or ^{:line 832 :file "cli/agents-cli.bclj"} (:verbose options) ^{:line 832 :file "cli/agents-cli.bclj"} (not= :human ^{:line 832 :file "cli/agents-cli.bclj"} (:mode options))) ^{:line 833 :file "cli/agents-cli.bclj"} (agents-error! "conflicting or duplicate option --json") ^{:line 834 :file "cli/agents-cli.bclj"} (recur ^{:line 834 :file "cli/agents-cli.bclj"} (vec more) ^{:line 834 :file "cli/agents-cli.bclj"} (assoc options :mode :json)))
  :else ^{:line 836 :file "cli/agents-cli.bclj"} (agents-error! ^{:line 836 :file "cli/agents-cli.bclj"} (str "unknown option " arg)))))))

^{:line 838 :file "cli/agents-cli.bclj"} (defn- roster-row-key [row]
  ^{:line 839 :file "cli/agents-cli.bclj"} [^{:line 839 :file "cli/agents-cli.bclj"} (case ^{:line 839 :file "cli/agents-cli.bclj"} (get row "state")
    "finished" 3
    ^{:line 841 :file "cli/agents-cli.bclj"} (case ^{:line 841 :file "cli/agents-cli.bclj"} (get row "kind")
    "lane" 0
    "session" 1
    2)) ^{:line 845 :file "cli/agents-cli.bclj"} (get row "display_name") ^{:line 846 :file "cli/agents-cli.bclj"} (get row "control_id")])

^{:line 848 :file "cli/agents-cli.bclj"} (defn- roster-contract [presence agents sessions]
  ^{:line 852 :file "cli/agents-cli.bclj"} {"version" ROSTER-CONTRACT-VERSION "agents" ^{:line 854 :file "cli/agents-cli.bclj"} (->> presence ^{:line 855 :file "cli/agents-cli.bclj"} (mapv ^{:line 855 :file "cli/agents-cli.bclj"} (fn [row] ^{:line 856 :file "cli/agents-cli.bclj"} (roster-json-row row ^{:line 857 :file "cli/agents-cli.bclj"} (get agents ^{:line 857 :file "cli/agents-cli.bclj"} (:id row) ^{:line 857 :file "cli/agents-cli.bclj"} {}) ^{:line 858 :file "cli/agents-cli.bclj"} (get sessions ^{:line 858 :file "cli/agents-cli.bclj"} (:id row) ^{:line 858 :file "cli/agents-cli.bclj"} {})))) ^{:line 859 :file "cli/agents-cli.bclj"} (sort-by roster-row-key) vec)})

^{:line 862 :file "cli/agents-cli.bclj"} (defn- configured-codex-accounts []
  ^{:line 863 :file "cli/agents-cli.bclj"} (let [result ^{:line 863 :file "cli/agents-cli.bclj"} (run ^{:line 863 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" CODEX-CENSUS-CLI] :timeout 4000)
   parsed ^{:line 864 :file "cli/agents-cli.bclj"} (try
  ^{:line 864 :file "cli/agents-cli.bclj"} (json/parse-string ^{:line 864 :file "cli/agents-cli.bclj"} (str/trim ^{:line 864 :file "cli/agents-cli.bclj"} (:out result)) false)
  (catch Exception _
    nil))]
  ^{:line 866 :file "cli/agents-cli.bclj"} (if ^{:line 866 :file "cli/agents-cli.bclj"} (and ^{:line 866 :file "cli/agents-cli.bclj"} (:ok result) ^{:line 866 :file "cli/agents-cli.bclj"} (sequential? parsed) ^{:line 867 :file "cli/agents-cli.bclj"} (every? ^{:line 867 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 867 :file "cli/agents-cli.bclj"} (and ^{:line 867 :file "cli/agents-cli.bclj"} (string? %1) ^{:line 867 :file "cli/agents-cli.bclj"} (not ^{:line 867 :file "cli/agents-cli.bclj"} (str/blank? %1)))) parsed)) ^{:line 868 :file "cli/agents-cli.bclj"} (vec ^{:line 868 :file "cli/agents-cli.bclj"} (sort ^{:line 868 :file "cli/agents-cli.bclj"} (distinct parsed))) ^{:line 868 :file "cli/agents-cli.bclj"} [])))

^{:line 870 :file "cli/agents-cli.bclj"} (defn- census-fact [facts predicate]
  ^{:line 873 :file "cli/agents-cli.bclj"} (if ^{:line 873 :file "cli/agents-cli.bclj"} (not ^{:line 873 :file "cli/agents-cli.bclj"} (contains? ^{:line 873 :file "cli/agents-cli.bclj"} (get facts roster-conflict-key ^{:line 873 :file "cli/agents-cli.bclj"} #{}) predicate)) ^{:line 873 :file "cli/agents-cli.bclj"} (do
  ^{:line 874 :file "cli/agents-cli.bclj"} (known ^{:line 874 :file "cli/agents-cli.bclj"} (get facts predicate)))))

^{:line 876 :file "cli/agents-cli.bclj"} (defn- codex-census [rows agents]
  ^{:line 879 :file "cli/agents-cli.bclj"} (let [parent-of ^{:line 879 :file "cli/agents-cli.bclj"} (fn [facts] ^{:line 879 :file "cli/agents-cli.bclj"} (or ^{:line 879 :file "cli/agents-cli.bclj"} (census-fact facts "coordinator") ^{:line 880 :file "cli/agents-cli.bclj"} (census-fact facts "supervisor")))
   children ^{:line 881 :file "cli/agents-cli.bclj"} (reduce ^{:line 881 :file "cli/agents-cli.bclj"} (fn [out row] ^{:line 884 :file "cli/agents-cli.bclj"} (let [facts ^{:line 884 :file "cli/agents-cli.bclj"} (get agents ^{:line 884 :file "cli/agents-cli.bclj"} (:id row) ^{:line 884 :file "cli/agents-cli.bclj"} {})
   parent ^{:line 885 :file "cli/agents-cli.bclj"} (parent-of facts)]
  ^{:line 886 :file "cli/agents-cli.bclj"} (if ^{:line 886 :file "cli/agents-cli.bclj"} (and ^{:line 886 :file "cli/agents-cli.bclj"} (= "openai" ^{:line 886 :file "cli/agents-cli.bclj"} (census-fact facts "provider")) parent) ^{:line 887 :file "cli/agents-cli.bclj"} (update out parent ^{:line 887 :file "cli/agents-cli.bclj"} (fnil conj ^{:line 887 :file "cli/agents-cli.bclj"} []) ^{:line 887 :file "cli/agents-cli.bclj"} (:id row)) out))) ^{:line 887 :file "cli/agents-cli.bclj"} {} rows)]
  ^{:line 888 :file "cli/agents-cli.bclj"} {"configured_accounts" ^{:line 888 :file "cli/agents-cli.bclj"} (configured-codex-accounts) "sessions" ^{:line 890 :file "cli/agents-cli.bclj"} (->> rows ^{:line 891 :file "cli/agents-cli.bclj"} (keep ^{:line 891 :file "cli/agents-cli.bclj"} (fn [row] ^{:line 892 :file "cli/agents-cli.bclj"} (let [facts ^{:line 892 :file "cli/agents-cli.bclj"} (get agents ^{:line 892 :file "cli/agents-cli.bclj"} (:id row) ^{:line 892 :file "cli/agents-cli.bclj"} {})
   parent ^{:line 893 :file "cli/agents-cli.bclj"} (parent-of facts)]
  ^{:line 894 :file "cli/agents-cli.bclj"} (if ^{:line 894 :file "cli/agents-cli.bclj"} (= "openai" ^{:line 894 :file "cli/agents-cli.bclj"} (census-fact facts "provider")) ^{:line 894 :file "cli/agents-cli.bclj"} (do
  ^{:line 895 :file "cli/agents-cli.bclj"} {"control_id" ^{:line 895 :file "cli/agents-cli.bclj"} (:id row) "account_id" ^{:line 896 :file "cli/agents-cli.bclj"} (or ^{:line 896 :file "cli/agents-cli.bclj"} (census-fact facts "provider_target") "ambient") "session_identity" ^{:line 897 :file "cli/agents-cli.bclj"} (:id row) "parent_control_id" ^{:line 898 :file "cli/agents-cli.bclj"} (or parent "") "child_control_ids" ^{:line 899 :file "cli/agents-cli.bclj"} (vec ^{:line 899 :file "cli/agents-cli.bclj"} (sort ^{:line 899 :file "cli/agents-cli.bclj"} (get children ^{:line 899 :file "cli/agents-cli.bclj"} (:id row) ^{:line 899 :file "cli/agents-cli.bclj"} []))) "activity_at" ^{:line 900 :file "cli/agents-cli.bclj"} (or ^{:line 900 :file "cli/agents-cli.bclj"} (census-fact facts "started_at") ^{:line 901 :file "cli/agents-cli.bclj"} (census-fact facts "spawned_at") "") "freshness" ^{:line 902 :file "cli/agents-cli.bclj"} (if ^{:line 902 :file "cli/agents-cli.bclj"} (:online row) "fresh" "stale") "freshness_evidence" ^{:line 903 :file "cli/agents-cli.bclj"} (if ^{:line 903 :file "cli/agents-cli.bclj"} (:online row) "Store liveness lease" "Store liveness lease absent")}))))) ^{:line 906 :file "cli/agents-cli.bclj"} (sort-by ^{:line 906 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 906 :file "cli/agents-cli.bclj"} (get %1 "control_id"))) vec)}))

^{:line 908 :file "cli/agents-cli.bclj"} (def comparable-roster-fields ^{:line 912 :file "cli/agents-cli.bclj"} ["control_id" "display_handle" "kind" "provider" "provider_target" "provider_label" "model" "model_display" "effort" "orchestration_provenance"])

^{:line 916 :file "cli/agents-cli.bclj"} (defn- read-roster-snapshot []
  ^{:line 917 :file "cli/agents-cli.bclj"} (let [presence ^{:line 917 :file "cli/agents-cli.bclj"} (presence-rows)]
  ^{:line 918 :file "cli/agents-cli.bclj"} (if ^{:line 918 :file "cli/agents-cli.bclj"} (:err presence) ^{:line 919 :file "cli/agents-cli.bclj"} {:err ^{:line 919 :file "cli/agents-cli.bclj"} (:err presence)} ^{:line 920 :file "cli/agents-cli.bclj"} (let [rows ^{:line 920 :file "cli/agents-cli.bclj"} (vec ^{:line 920 :file "cli/agents-cli.bclj"} (filter ^{:line 920 :file "cli/agents-cli.bclj"} (fn [row] ^{:line 920 :file "cli/agents-cli.bclj"} (:online row)) ^{:line 921 :file "cli/agents-cli.bclj"} (:agents presence)))
   ids ^{:line 922 :file "cli/agents-cli.bclj"} (mapv ^{:line 922 :file "cli/agents-cli.bclj"} (fn [row] ^{:line 922 :file "cli/agents-cli.bclj"} (:id row)) rows)
   facts ^{:line 923 :file "cli/agents-cli.bclj"} (roster-facts ids)]
  ^{:line 924 :file "cli/agents-cli.bclj"} (if ^{:line 924 :file "cli/agents-cli.bclj"} (:err facts) ^{:line 925 :file "cli/agents-cli.bclj"} {:err ^{:line 925 :file "cli/agents-cli.bclj"} (:err facts)} ^{:line 926 :file "cli/agents-cli.bclj"} (let [managed-ids ^{:line 926 :file "cli/agents-cli.bclj"} (filterv ^{:line 926 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 926 :file "cli/agents-cli.bclj"} (= "lane" ^{:line 926 :file "cli/agents-cli.bclj"} (get-in facts ^{:line 926 :file "cli/agents-cli.bclj"} [:agents %1 "kind"]))) ids)
   run-projection ^{:line 927 :file "cli/agents-cli.bclj"} (roster-run-entries managed-ids)
   agents ^{:line 928 :file "cli/agents-cli.bclj"} (attach-lane-resolutions ids ^{:line 929 :file "cli/agents-cli.bclj"} (:agents facts) run-projection)
   sessions ^{:line 930 :file "cli/agents-cli.bclj"} (:sessions facts)]
  ^{:line 931 :file "cli/agents-cli.bclj"} {:rows rows :agents agents :sessions sessions :snapshot ^{:line 934 :file "cli/agents-cli.bclj"} (assoc ^{:line 934 :file "cli/agents-cli.bclj"} (roster-contract rows agents sessions) "codex_census" ^{:line 935 :file "cli/agents-cli.bclj"} (codex-census rows agents))}))))))

^{:line 937 :file "cli/agents-cli.bclj"} (defn- comparable-roster [snapshot]
  ^{:line 938 :file "cli/agents-cli.bclj"} (if ^{:line 938 :file "cli/agents-cli.bclj"} (and ^{:line 938 :file "cli/agents-cli.bclj"} (= ROSTER-CONTRACT-VERSION ^{:line 938 :file "cli/agents-cli.bclj"} (get snapshot "version")) ^{:line 939 :file "cli/agents-cli.bclj"} (vector? ^{:line 939 :file "cli/agents-cli.bclj"} (get snapshot "agents"))) ^{:line 938 :file "cli/agents-cli.bclj"} (do
  ^{:line 940 :file "cli/agents-cli.bclj"} (let [rows ^{:line 940 :file "cli/agents-cli.bclj"} (get snapshot "agents")]
  ^{:line 941 :file "cli/agents-cli.bclj"} (if ^{:line 941 :file "cli/agents-cli.bclj"} (and ^{:line 941 :file "cli/agents-cli.bclj"} (every? ^{:line 941 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 941 :file "cli/agents-cli.bclj"} (and ^{:line 941 :file "cli/agents-cli.bclj"} (map? %1) ^{:line 941 :file "cli/agents-cli.bclj"} (every? ^{:line 941 :file "cli/agents-cli.bclj"} (fn [field] ^{:line 941 :file "cli/agents-cli.bclj"} (contains? %1 field)) comparable-roster-fields))) rows) ^{:line 945 :file "cli/agents-cli.bclj"} (= ^{:line 945 :file "cli/agents-cli.bclj"} (count rows) ^{:line 945 :file "cli/agents-cli.bclj"} (count ^{:line 945 :file "cli/agents-cli.bclj"} (set ^{:line 945 :file "cli/agents-cli.bclj"} (map ^{:line 945 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 945 :file "cli/agents-cli.bclj"} (get %1 "control_id")) rows))))) ^{:line 941 :file "cli/agents-cli.bclj"} (do
  ^{:line 946 :file "cli/agents-cli.bclj"} (->> rows ^{:line 947 :file "cli/agents-cli.bclj"} (mapv ^{:line 947 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 947 :file "cli/agents-cli.bclj"} (select-keys %1 comparable-roster-fields))) ^{:line 948 :file "cli/agents-cli.bclj"} (sort-by ^{:line 948 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 948 :file "cli/agents-cli.bclj"} (get %1 "control_id"))) vec)))))))

^{:line 951 :file "cli/agents-cli.bclj"} (defn cmd-agents! [args]
  ^{:line 952 :file "cli/agents-cli.bclj"} (let [{:keys [mode verbose help]} ^{:line 952 :file "cli/agents-cli.bclj"} (parse-agents-options! args)]
  ^{:line 953 :file "cli/agents-cli.bclj"} (if help ^{:line 954 :file "cli/agents-cli.bclj"} (agents-usage) ^{:line 955 :file "cli/agents-cli.bclj"} (do
  ^{:line 956 :file "cli/agents-cli.bclj"} (if verbose ^{:line 956 :file "cli/agents-cli.bclj"} (do
  ^{:line 956 :file "cli/agents-cli.bclj"} (println ^{:line 956 :file "cli/agents-cli.bclj"} (dim ^{:line 956 :file "cli/agents-cli.bclj"} (str "Store RPC liveness lease projection :" PORT)))))
  ^{:line 957 :file "cli/agents-cli.bclj"} (let [loaded ^{:line 957 :file "cli/agents-cli.bclj"} (read-roster-snapshot)]
  ^{:line 958 :file "cli/agents-cli.bclj"} (if ^{:line 958 :file "cli/agents-cli.bclj"} (:err loaded) ^{:line 959 :file "cli/agents-cli.bclj"} (if ^{:line 959 :file "cli/agents-cli.bclj"} (= mode :human) ^{:line 960 :file "cli/agents-cli.bclj"} (println ^{:line 960 :file "cli/agents-cli.bclj"} (ylw ^{:line 960 :file "cli/agents-cli.bclj"} (:err loaded))) ^{:line 961 :file "cli/agents-cli.bclj"} (agents-error! ^{:line 961 :file "cli/agents-cli.bclj"} (:err loaded))) ^{:line 962 :file "cli/agents-cli.bclj"} (let [rows ^{:line 962 :file "cli/agents-cli.bclj"} (:rows loaded)
   af ^{:line 963 :file "cli/agents-cli.bclj"} (:agents loaded)
   sf ^{:line 964 :file "cli/agents-cli.bclj"} (:sessions loaded)
   snapshot ^{:line 965 :file "cli/agents-cli.bclj"} (:snapshot loaded)]
  ^{:line 966 :file "cli/agents-cli.bclj"} (if ^{:line 966 :file "cli/agents-cli.bclj"} (= mode :json) ^{:line 967 :file "cli/agents-cli.bclj"} (println ^{:line 967 :file "cli/agents-cli.bclj"} (json/generate-string snapshot)) ^{:line 968 :file "cli/agents-cli.bclj"} (let [categorized ^{:line 968 :file "cli/agents-cli.bclj"} (group-by ^{:line 968 :file "cli/agents-cli.bclj"} (fn [a] ^{:line 968 :file "cli/agents-cli.bclj"} (roster-category ^{:line 968 :file "cli/agents-cli.bclj"} (get af ^{:line 968 :file "cli/agents-cli.bclj"} (:id a) ^{:line 968 :file "cli/agents-cli.bclj"} {}))) rows)
   active-agents ^{:line 969 :file "cli/agents-cli.bclj"} (vec ^{:line 969 :file "cli/agents-cli.bclj"} (:active-agent categorized ^{:line 969 :file "cli/agents-cli.bclj"} []))
   native-sessions ^{:line 970 :file "cli/agents-cli.bclj"} (vec ^{:line 970 :file "cli/agents-cli.bclj"} (:native-session categorized ^{:line 970 :file "cli/agents-cli.bclj"} []))
   unclassified ^{:line 971 :file "cli/agents-cli.bclj"} (vec ^{:line 971 :file "cli/agents-cli.bclj"} (:unclassified categorized ^{:line 971 :file "cli/agents-cli.bclj"} []))
   inconsistent ^{:line 972 :file "cli/agents-cli.bclj"} (vec ^{:line 972 :file "cli/agents-cli.bclj"} (:inconsistent categorized ^{:line 972 :file "cli/agents-cli.bclj"} []))
   finished ^{:line 973 :file "cli/agents-cli.bclj"} (vec ^{:line 973 :file "cli/agents-cli.bclj"} (:recently-finished categorized ^{:line 973 :file "cli/agents-cli.bclj"} []))
   active ^{:line 974 :file "cli/agents-cli.bclj"} (+ ^{:line 974 :file "cli/agents-cli.bclj"} (count active-agents) ^{:line 975 :file "cli/agents-cli.bclj"} (count native-sessions) ^{:line 976 :file "cli/agents-cli.bclj"} (count unclassified))
   render-section ^{:line 977 :file "cli/agents-cli.bclj"} (fn [title note section] ^{:line 981 :file "cli/agents-cli.bclj"} (if ^{:line 981 :file "cli/agents-cli.bclj"} (seq section) ^{:line 981 :file "cli/agents-cli.bclj"} (do
  ^{:line 982 :file "cli/agents-cli.bclj"} (println)
  ^{:line 983 :file "cli/agents-cli.bclj"} (if note ^{:line 984 :file "cli/agents-cli.bclj"} (println ^{:line 984 :file "cli/agents-cli.bclj"} (bold ^{:line 984 :file "cli/agents-cli.bclj"} (str title " (" ^{:line 984 :file "cli/agents-cli.bclj"} (count section) ")")) ^{:line 984 :file "cli/agents-cli.bclj"} (dim note)) ^{:line 985 :file "cli/agents-cli.bclj"} (println ^{:line 985 :file "cli/agents-cli.bclj"} (bold ^{:line 985 :file "cli/agents-cli.bclj"} (str title " (" ^{:line 985 :file "cli/agents-cli.bclj"} (count section) ")"))))
  ^{:line 986 :file "cli/agents-cli.bclj"} (doseq [a section]
  ^{:line 987 :file "cli/agents-cli.bclj"} (let [facts ^{:line 987 :file "cli/agents-cli.bclj"} (get af ^{:line 987 :file "cli/agents-cli.bclj"} (:id a) ^{:line 987 :file "cli/agents-cli.bclj"} {})
   session ^{:line 988 :file "cli/agents-cli.bclj"} (get sf ^{:line 988 :file "cli/agents-cli.bclj"} (:id a) ^{:line 988 :file "cli/agents-cli.bclj"} {})
   handle ^{:line 989 :file "cli/agents-cli.bclj"} (semantic-handle ^{:line 989 :file "cli/agents-cli.bclj"} (:id a) facts)]
  ^{:line 990 :file "cli/agents-cli.bclj"} (println ^{:line 990 :file "cli/agents-cli.bclj"} (str "  " ^{:line 990 :file "cli/agents-cli.bclj"} (grn "●") " " ^{:line 991 :file "cli/agents-cli.bclj"} (agent-primary-line a facts session)))
  ^{:line 992 :file "cli/agents-cli.bclj"} (println ^{:line 992 :file "cli/agents-cli.bclj"} (dim ^{:line 992 :file "cli/agents-cli.bclj"} (str "    " handle " · control " ^{:line 993 :file "cli/agents-cli.bclj"} (:id a) " · live-input " ^{:line 994 :file "cli/agents-cli.bclj"} (live-input-label facts) " · ttl " ^{:line 995 :file "cli/agents-cli.bclj"} (:expires a)))))))))]
  ^{:line 996 :file "cli/agents-cli.bclj"} (println ^{:line 996 :file "cli/agents-cli.bclj"} (bold ^{:line 996 :file "cli/agents-cli.bclj"} (str ^{:line 996 :file "cli/agents-cli.bclj"} (count rows) " roster entries")) ^{:line 997 :file "cli/agents-cli.bclj"} (dim ^{:line 997 :file "cli/agents-cli.bclj"} (str "· " active " active · " ^{:line 998 :file "cli/agents-cli.bclj"} (count inconsistent) " inconsistent · " ^{:line 999 :file "cli/agents-cli.bclj"} (count finished) " recently finished")))
  ^{:line 1000 :file "cli/agents-cli.bclj"} (render-section "active agents" nil active-agents)
  ^{:line 1001 :file "cli/agents-cli.bclj"} (render-section "native sessions" "(active provider CLI sessions)" native-sessions)
  ^{:line 1003 :file "cli/agents-cli.bclj"} (render-section "unclassified lease" "(missing identity facts)" unclassified)
  ^{:line 1005 :file "cli/agents-cli.bclj"} (render-section "inconsistent lifecycle" "(terminal/run projection is incomplete, conflicting, or unavailable)" inconsistent)
  ^{:line 1008 :file "cli/agents-cli.bclj"} (render-section "recently finished" "(process is terminal; delivery evidence is shown separately; liveness lease has not lapsed)" finished)
  ^{:line 1024 :file "cli/agents-cli.bclj"} (let [known ^{:line 1024 :file "cli/agents-cli.bclj"} (set ^{:line 1024 :file "cli/agents-cli.bclj"} (map ^{:line 1024 :file "cli/agents-cli.bclj"} (fn [row] ^{:line 1024 :file "cli/agents-cli.bclj"} (:id row)) rows))
   orphans ^{:line 1025 :file "cli/agents-cli.bclj"} (->> ^{:line 1025 :file "cli/agents-cli.bclj"} (or ^{:line 1025 :file "cli/agents-cli.bclj"} (.listFiles ^{:line 1025 :file "cli/agents-cli.bclj"} (java.io.File. "/proc")) ^{:line 1025 :file "cli/agents-cli.bclj"} []) ^{:line 1026 :file "cli/agents-cli.bclj"} (filter ^{:line 1026 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1026 :file "cli/agents-cli.bclj"} (re-matches #"\d+" ^{:line 1026 :file "cli/agents-cli.bclj"} (.getName %1)))) ^{:line 1027 :file "cli/agents-cli.bclj"} (keep ^{:line 1027 :file "cli/agents-cli.bclj"} (fn [d] ^{:line 1028 :file "cli/agents-cli.bclj"} (let [env ^{:line 1028 :file "cli/agents-cli.bclj"} (try
  ^{:line 1028 :file "cli/agents-cli.bclj"} (slurp ^{:line 1028 :file "cli/agents-cli.bclj"} (java.io.File. d "environ"))
  (catch Exception _
    ""))
   kv ^{:line 1030 :file "cli/agents-cli.bclj"} (into ^{:line 1030 :file "cli/agents-cli.bclj"} {} ^{:line 1030 :file "cli/agents-cli.bclj"} (keep ^{:line 1030 :file "cli/agents-cli.bclj"} (fn [e] ^{:line 1031 :file "cli/agents-cli.bclj"} (let [[k v] ^{:line 1031 :file "cli/agents-cli.bclj"} (str/split e #"=" 2)]
  ^{:line 1032 :file "cli/agents-cli.bclj"} (if v ^{:line 1032 :file "cli/agents-cli.bclj"} (do
  ^{:line 1032 :file "cli/agents-cli.bclj"} [k v])))) ^{:line 1033 :file "cli/agents-cli.bclj"} (str/split env ^{:line 1033 :file "cli/agents-cli.bclj"} (re-pattern "\u0000"))))
   id ^{:line 1034 :file "cli/agents-cli.bclj"} (get kv "AGENT_ID")]
  ^{:line 1035 :file "cli/agents-cli.bclj"} (if ^{:line 1035 :file "cli/agents-cli.bclj"} (and id ^{:line 1035 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 1035 :file "cli/agents-cli.bclj"} (str id) "lane-") ^{:line 1036 :file "cli/agents-cli.bclj"} (not ^{:line 1036 :file "cli/agents-cli.bclj"} (contains? known id))) ^{:line 1035 :file "cli/agents-cli.bclj"} (do
  ^{:line 1037 :file "cli/agents-cli.bclj"} {:id id :pid ^{:line 1037 :file "cli/agents-cli.bclj"} (.getName d) :referent ^{:line 1038 :file "cli/agents-cli.bclj"} (get kv "AGENT_REFERENT")}))))) ^{:line 1039 :file "cli/agents-cli.bclj"} (reduce ^{:line 1039 :file "cli/agents-cli.bclj"} (fn [m o] ^{:line 1042 :file "cli/agents-cli.bclj"} (assoc m ^{:line 1042 :file "cli/agents-cli.bclj"} (:id o) o)) ^{:line 1042 :file "cli/agents-cli.bclj"} {}) ^{:line 1043 :file "cli/agents-cli.bclj"} (^{:line 1043 :file "cli/agents-cli.bclj"} (fn [orphans-by-id] ^{:line 1044 :file "cli/agents-cli.bclj"} (mapv ^{:line 1044 :file "cli/agents-cli.bclj"} (fn [id] ^{:line 1044 :file "cli/agents-cli.bclj"} (get orphans-by-id id)) ^{:line 1045 :file "cli/agents-cli.bclj"} (sort ^{:line 1045 :file "cli/agents-cli.bclj"} (keys orphans-by-id))))))]
  ^{:line 1046 :file "cli/agents-cli.bclj"} (if ^{:line 1046 :file "cli/agents-cli.bclj"} (seq orphans) ^{:line 1046 :file "cli/agents-cli.bclj"} (do
  ^{:line 1047 :file "cli/agents-cli.bclj"} (println)
  ^{:line 1048 :file "cli/agents-cli.bclj"} (println ^{:line 1048 :file "cli/agents-cli.bclj"} (bold ^{:line 1048 :file "cli/agents-cli.bclj"} (str "orphaned processes (" ^{:line 1048 :file "cli/agents-cli.bclj"} (count orphans) ")")) ^{:line 1049 :file "cli/agents-cli.bclj"} (dim "(live AGENT_ID with no roster lease — reap or investigate)"))
  ^{:line 1050 :file "cli/agents-cli.bclj"} (doseq [o orphans]
  ^{:line 1051 :file "cli/agents-cli.bclj"} (println ^{:line 1051 :file "cli/agents-cli.bclj"} (str "  " ^{:line 1051 :file "cli/agents-cli.bclj"} (red "●") " pid " ^{:line 1051 :file "cli/agents-cli.bclj"} (:pid o) " · " ^{:line 1051 :file "cli/agents-cli.bclj"} (:id o)))
  ^{:line 1052 :file "cli/agents-cli.bclj"} (println ^{:line 1052 :file "cli/agents-cli.bclj"} (dim ^{:line 1052 :file "cli/agents-cli.bclj"} (str "    referent " ^{:line 1052 :file "cli/agents-cli.bclj"} (or ^{:line 1052 :file "cli/agents-cli.bclj"} (:referent o) "(unbound)") " · not in the roster: its lease lapsed while the process lived"))))))))))))))))

^{:line 1055 :file "cli/agents-cli.bclj"} (def spawn-flags ^{:line 1056 :file "cli/agents-cli.bclj"} {"--notify" :notify "--provider" :provider "--target" :target "--taskGrade" :taskGrade "--task-grade" :taskGrade "--domain" :domain "--topology" :topology "--tier" :tier "--reasoning" :reasoning "--deliberation" :reasoning "--posture" :posture "--composition" :composition "--rationale" :rationale "--nearest" :nearest "--contract" :contract "--override-reason" :overrideReason "--model" :model "--assessment" :assessment "--routing-assessment" :assessment "--pin-evidence" :pinEvidence "--referent" :referent})

^{:line 1069 :file "cli/agents-cli.bclj"} (defn cmd-spawn-help []
  ^{:line 1070 :file "cli/agents-cli.bclj"} (let [roles ^{:line 1070 :file "cli/agents-cli.bclj"} (sort ^{:line 1070 :file "cli/agents-cli.bclj"} (keys ^{:line 1070 :file "cli/agents-cli.bclj"} (or ^{:line 1070 :file "cli/agents-cli.bclj"} (orchestration-routing) ^{:line 1070 :file "cli/agents-cli.bclj"} {})))]
  ^{:line 1071 :file "cli/agents-cli.bclj"} (println "north agent spawn — start one managed lane with an explicit Orchestration composition")
  ^{:line 1072 :file "cli/agents-cli.bclj"} (println)
  ^{:line 1073 :file "cli/agents-cli.bclj"} (println "Usage:")
  ^{:line 1074 :file "cli/agents-cli.bclj"} (println "  north agent spawn <role> \"<prompt>\" [routing options] [--composition JSON|@file] [--dry-run]")
  ^{:line 1075 :file "cli/agents-cli.bclj"} (println)
  ^{:line 1076 :file "cli/agents-cli.bclj"} (println "Role and composition:")
  ^{:line 1077 :file "cli/agents-cli.bclj"} (println "  Role is functional identity, independent of composition kind and id.")
  ^{:line 1078 :file "cli/agents-cli.bclj"} (println "  Catalogued and novel roles may use either a template or bespoke composition.")
  ^{:line 1079 :file "cli/agents-cli.bclj"} (println "  A template composition hydrates task grade, tier, reasoning, topology, posture, and capabilities.")
  ^{:line 1080 :file "cli/agents-cli.bclj"} (println "  Override an axis with --task-grade, --domain, --tier, --reasoning, or --posture;")
  ^{:line 1081 :file "cli/agents-cli.bclj"} (println "  any changed template axis requires --override-reason WHY. Exact templates carry no override reason.")
  ^{:line 1082 :file "cli/agents-cli.bclj"} (println "  Stock topology is fixed; --topology applies only to bespoke compositions.")
  ^{:line 1083 :file "cli/agents-cli.bclj"} (println "  Available templates:" ^{:line 1083 :file "cli/agents-cli.bclj"} (if ^{:line 1083 :file "cli/agents-cli.bclj"} (seq roles) ^{:line 1083 :file "cli/agents-cli.bclj"} (str/join " " roles) "(catalog unavailable)"))
  ^{:line 1084 :file "cli/agents-cli.bclj"} (println "  Inspect their full routing defaults with: north agent templates")
  ^{:line 1085 :file "cli/agents-cli.bclj"} (println)
  ^{:line 1086 :file "cli/agents-cli.bclj"} (println "Bespoke composition:")
  ^{:line 1087 :file "cli/agents-cli.bclj"} (println "  A bespoke composition requires a rationale and structured contract regardless of role identity.")
  ^{:line 1088 :file "cli/agents-cli.bclj"} (println "  Contract JSON contains exactly: responsibility, deliverable, capabilities, mayDecide,")
  ^{:line 1089 :file "cli/agents-cli.bclj"} (println "  mustEscalate, doneWhen, report. Text fields are nonblank; list fields are nonempty.")
  ^{:line 1090 :file "cli/agents-cli.bclj"} (println "  Canonical capabilities: filesystem.read filesystem.search filesystem.write shell")
  ^{:line 1091 :file "cli/agents-cli.bclj"} (println "                          shell.readonly web coordination")
  ^{:line 1092 :file "cli/agents-cli.bclj"} (println "  --nearest TEMPLATE is optional reference provenance, not inheritance.")
  ^{:line 1093 :file "cli/agents-cli.bclj"} (println "  Without --nearest, explicitly set task grade, topology, tier, reasoning, and posture.")
  ^{:line 1094 :file "cli/agents-cli.bclj"} (println "  Domain requirements remain an explicit empty list when --domain is omitted.")
  ^{:line 1095 :file "cli/agents-cli.bclj"} (println "  --promotion-candidate nominates recurrence for human review; default is false.")
  ^{:line 1096 :file "cli/agents-cli.bclj"} (println "  --composition JSON|@file is the advanced full payload form (machine kinds: template|bespoke).")
  ^{:line 1097 :file "cli/agents-cli.bclj"} (println)
  ^{:line 1098 :file "cli/agents-cli.bclj"} (println "Routing and control:")
  ^{:line 1099 :file "cli/agents-cli.bclj"} (println "  Mutation-capable compositions default to a managed worktree lane.")
  ^{:line 1100 :file "cli/agents-cli.bclj"} (println "  SDK worktree=false is an explicit read-only opt-out; AGENT_WORKTREE=1 remains an explicit override.")
  ^{:line 1101 :file "cli/agents-cli.bclj"} (println "  --provider auto|anthropic|openai   provider preference (default auto)")
  ^{:line 1102 :file "cli/agents-cli.bclj"} (println "  --target ACCOUNT                  exact account pin; unavailable means no fallback")
  ^{:line 1103 :file "cli/agents-cli.bclj"} (println "  --model MODEL                     exact model pin")
  ^{:line 1104 :file "cli/agents-cli.bclj"} (println "  --assessment JSON|@file           canonical Orchestration selection-assessment sidecar")
  ^{:line 1105 :file "cli/agents-cli.bclj"} (println "  --pin-evidence JSON|@file         typed reason + <=24h expiry for provider/account/model pins")
  ^{:line 1106 :file "cli/agents-cli.bclj"} (println "  New explicit pins fail closed without --pin-evidence; reasoning=max requires --assessment.")
  ^{:line 1107 :file "cli/agents-cli.bclj"} (println "  --domain D[,D...]                 repeatable domain requirement")
  ^{:line 1108 :file "cli/agents-cli.bclj"} (println "  --reasoning low|medium|high|xhigh|max  (--deliberation is an alias)")
  ^{:line 1109 :file "cli/agents-cli.bclj"} (println "  --notify PEER                     completion/stall notifications")
  ^{:line 1110 :file "cli/agents-cli.bclj"} (println "  --dry-run                         validate pinned-provider capability authority; show identity only when supported")
  ^{:line 1111 :file "cli/agents-cli.bclj"} (println "  --doctor [--json]                 test every dispatch invariant at once; one PASS/FAIL row + fix per wall")
  ^{:line 1112 :file "cli/agents-cli.bclj"} (println "  --doctor --canary                 spawn one tiny read-only managed lane end to end and report its lifecycle")))

^{:line 1114 :file "cli/agents-cli.bclj"} (defn- parse-spawn-args [args]
  ^{:line 1115 :file "cli/agents-cli.bclj"} (loop [xs args
   positionals ^{:line 1116 :file "cli/agents-cli.bclj"} []
   opts ^{:line 1117 :file "cli/agents-cli.bclj"} {:domains ^{:line 1117 :file "cli/agents-cli.bclj"} [] :seen ^{:line 1117 :file "cli/agents-cli.bclj"} #{}}]
  ^{:line 1118 :file "cli/agents-cli.bclj"} (let [x ^{:line 1118 :file "cli/agents-cli.bclj"} (first xs)]
  ^{:line 1118 :file "cli/agents-cli.bclj"} (if x ^{:line 1119 :file "cli/agents-cli.bclj"} (cond
  ^{:line 1120 :file "cli/agents-cli.bclj"} (= x "--dry-run") ^{:line 1120 :file "cli/agents-cli.bclj"} (recur ^{:line 1120 :file "cli/agents-cli.bclj"} (rest xs) positionals ^{:line 1120 :file "cli/agents-cli.bclj"} (assoc opts :dry? true))
  ^{:line 1123 :file "cli/agents-cli.bclj"} (= x "--ad-hoc") ^{:line 1123 :file "cli/agents-cli.bclj"} (recur ^{:line 1123 :file "cli/agents-cli.bclj"} (rest xs) positionals ^{:line 1123 :file "cli/agents-cli.bclj"} (assoc opts :ad-hoc? true))
  ^{:line 1124 :file "cli/agents-cli.bclj"} (^{:line 1124 :file "cli/agents-cli.bclj"} #{"--promotion-candidate" "--nominate" "--no-promotion-candidate"} x) ^{:line 1125 :file "cli/agents-cli.bclj"} (if ^{:line 1125 :file "cli/agents-cli.bclj"} (:promotion-specified? opts) ^{:line 1126 :file "cli/agents-cli.bclj"} (do
  ^{:line 1126 :file "cli/agents-cli.bclj"} (println ^{:line 1126 :file "cli/agents-cli.bclj"} (red "choose exactly one promotion decision"))
  ^{:line 1126 :file "cli/agents-cli.bclj"} (System/exit 1)) ^{:line 1127 :file "cli/agents-cli.bclj"} (recur ^{:line 1127 :file "cli/agents-cli.bclj"} (rest xs) positionals ^{:line 1128 :file "cli/agents-cli.bclj"} (assoc opts :promotion-specified? true :promotionCandidate ^{:line 1129 :file "cli/agents-cli.bclj"} (not= x "--no-promotion-candidate"))))
  ^{:line 1130 :file "cli/agents-cli.bclj"} (spawn-flags x) ^{:line 1130 :file "cli/agents-cli.bclj"} (let [v ^{:line 1130 :file "cli/agents-cli.bclj"} (second xs)
   field ^{:line 1131 :file "cli/agents-cli.bclj"} (spawn-flags x)]
  ^{:line 1132 :file "cli/agents-cli.bclj"} (if ^{:line 1132 :file "cli/agents-cli.bclj"} (or ^{:line 1132 :file "cli/agents-cli.bclj"} (nil? v) ^{:line 1132 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 1132 :file "cli/agents-cli.bclj"} (str v) "--")) ^{:line 1132 :file "cli/agents-cli.bclj"} (do
  ^{:line 1133 :file "cli/agents-cli.bclj"} (println ^{:line 1133 :file "cli/agents-cli.bclj"} (red ^{:line 1133 :file "cli/agents-cli.bclj"} (str x " requires a value")))
  ^{:line 1133 :file "cli/agents-cli.bclj"} (System/exit 1)))
  ^{:line 1134 :file "cli/agents-cli.bclj"} (if ^{:line 1134 :file "cli/agents-cli.bclj"} (and ^{:line 1134 :file "cli/agents-cli.bclj"} (not= field :domain) ^{:line 1134 :file "cli/agents-cli.bclj"} (contains? ^{:line 1134 :file "cli/agents-cli.bclj"} (:seen opts) field)) ^{:line 1134 :file "cli/agents-cli.bclj"} (do
  ^{:line 1135 :file "cli/agents-cli.bclj"} (println ^{:line 1135 :file "cli/agents-cli.bclj"} (red ^{:line 1135 :file "cli/agents-cli.bclj"} (str "duplicate spawn option for " ^{:line 1135 :file "cli/agents-cli.bclj"} (name field) ": " x)))
  ^{:line 1136 :file "cli/agents-cli.bclj"} (System/exit 1)))
  ^{:line 1137 :file "cli/agents-cli.bclj"} (recur ^{:line 1137 :file "cli/agents-cli.bclj"} (nnext xs) positionals ^{:line 1138 :file "cli/agents-cli.bclj"} (if ^{:line 1138 :file "cli/agents-cli.bclj"} (= :domain field) ^{:line 1139 :file "cli/agents-cli.bclj"} (update opts :domains into ^{:line 1139 :file "cli/agents-cli.bclj"} (remove str/blank? ^{:line 1139 :file "cli/agents-cli.bclj"} (map str/trim ^{:line 1139 :file "cli/agents-cli.bclj"} (str/split ^{:line 1139 :file "cli/agents-cli.bclj"} (str v) #",")))) ^{:line 1140 :file "cli/agents-cli.bclj"} (-> opts ^{:line 1140 :file "cli/agents-cli.bclj"} (assoc field v) ^{:line 1140 :file "cli/agents-cli.bclj"} (update :seen conj field)))))
  ^{:line 1141 :file "cli/agents-cli.bclj"} (str/starts-with? x "--") ^{:line 1141 :file "cli/agents-cli.bclj"} (do
  ^{:line 1141 :file "cli/agents-cli.bclj"} (println ^{:line 1141 :file "cli/agents-cli.bclj"} (red ^{:line 1141 :file "cli/agents-cli.bclj"} (str "unknown spawn option: " x)))
  ^{:line 1141 :file "cli/agents-cli.bclj"} (System/exit 1))
  :else ^{:line 1142 :file "cli/agents-cli.bclj"} (recur ^{:line 1142 :file "cli/agents-cli.bclj"} (rest xs) ^{:line 1142 :file "cli/agents-cli.bclj"} (conj positionals x) opts)) ^{:line 1143 :file "cli/agents-cli.bclj"} (assoc ^{:line 1143 :file "cli/agents-cli.bclj"} (dissoc opts :seen) :positionals positionals)))))

^{:line 1145 :file "cli/agents-cli.bclj"} (defn- parse-json-input [label input]
  ^{:line 1148 :file "cli/agents-cli.bclj"} (if input ^{:line 1148 :file "cli/agents-cli.bclj"} (do
  ^{:line 1149 :file "cli/agents-cli.bclj"} (try
  ^{:line 1150 :file "cli/agents-cli.bclj"} (let [source ^{:line 1150 :file "cli/agents-cli.bclj"} (if ^{:line 1150 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 1150 :file "cli/agents-cli.bclj"} (str input) "@") ^{:line 1151 :file "cli/agents-cli.bclj"} (slurp ^{:line 1151 :file "cli/agents-cli.bclj"} (subs ^{:line 1151 :file "cli/agents-cli.bclj"} (str input) 1)) input)]
  ^{:line 1153 :file "cli/agents-cli.bclj"} (json/parse-string source true))
  (catch Exception e
    ^{:line 1155 :file "cli/agents-cli.bclj"} (println ^{:line 1155 :file "cli/agents-cli.bclj"} (red ^{:line 1155 :file "cli/agents-cli.bclj"} (str label " must be valid JSON or @file: " ^{:line 1155 :file "cli/agents-cli.bclj"} (.getMessage e))))
    ^{:line 1156 :file "cli/agents-cli.bclj"} (System/exit 1))))))

^{:line 1181 :file "cli/agents-cli.bclj"} (def routing-economics-preflight-timeout-ms 120000)

^{:line 1183 :file "cli/agents-cli.bclj"} (defn- preflight-failure-message
  "Never let a preflight die anonymously. `run` reports three distinguishable\n   non-ok shapes and only one of them carries subprocess output; name the other\n   two explicitly rather than falling back to a bare adjective." [result timeout-ms]
  ^{:line 1189 :file "cli/agents-cli.bclj"} (let [out ^{:line 1189 :file "cli/agents-cli.bclj"} (str ^{:line 1189 :file "cli/agents-cli.bclj"} (:out result))
   err ^{:line 1190 :file "cli/agents-cli.bclj"} (str ^{:line 1190 :file "cli/agents-cli.bclj"} (:err result))
   joined ^{:line 1191 :file "cli/agents-cli.bclj"} (str/trim ^{:line 1191 :file "cli/agents-cli.bclj"} (str err ^{:line 1191 :file "cli/agents-cli.bclj"} (if ^{:line 1191 :file "cli/agents-cli.bclj"} (and ^{:line 1191 :file "cli/agents-cli.bclj"} (seq err) ^{:line 1191 :file "cli/agents-cli.bclj"} (seq out)) ^{:line 1191 :file "cli/agents-cli.bclj"} (do
  "\n")) out))]
  ^{:line 1192 :file "cli/agents-cli.bclj"} (cond
  ^{:line 1193 :file "cli/agents-cli.bclj"} (seq joined) joined
  ^{:line 1194 :file "cli/agents-cli.bclj"} (:timeout result) ^{:line 1195 :file "cli/agents-cli.bclj"} (str "routing economics preflight exceeded its " timeout-ms "ms budget and was killed before it could report a reason" " (a cold or contended coordinator on port " ^{:line 1197 :file "cli/agents-cli.bclj"} (or ^{:line 1197 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_PORT") "7977") " is the usual cause; retry, and if it persists check that the" " coordinator is live and @catalog:current is imported)")
  ^{:line 1200 :file "cli/agents-cli.bclj"} (:error result) ^{:line 1201 :file "cli/agents-cli.bclj"} (str "routing economics preflight could not be started: " ^{:line 1201 :file "cli/agents-cli.bclj"} (:error result))
  :else ^{:line 1203 :file "cli/agents-cli.bclj"} (str "routing economics preflight exited " ^{:line 1203 :file "cli/agents-cli.bclj"} (:exit result) " without writing a reason to stdout or stderr"))))

^{:line 1206 :file "cli/agents-cli.bclj"} (defn- preflight-routing-economics! [routing-metadata routing-assessment pin-evidence provider target model dry?]
  ^{:line 1214 :file "cli/agents-cli.bclj"} (let [payload ^{:line 1214 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1214 :file "cli/agents-cli.bclj"} {:routingMetadata routing-metadata} routing-assessment ^{:line 1215 :file "cli/agents-cli.bclj"} (assoc :routingAssessment routing-assessment) pin-evidence ^{:line 1216 :file "cli/agents-cli.bclj"} (assoc :pinEvidence pin-evidence) provider ^{:line 1217 :file "cli/agents-cli.bclj"} (assoc :provider provider) target ^{:line 1218 :file "cli/agents-cli.bclj"} (assoc :target target) model ^{:line 1219 :file "cli/agents-cli.bclj"} (assoc :model model))
   result ^{:line 1220 :file "cli/agents-cli.bclj"} (run ^{:line 1220 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" ROUTING-ECONOMICS-PREFLIGHT-CLI] :timeout routing-economics-preflight-timeout-ms :env ^{:line 1225 :file "cli/agents-cli.bclj"} (assoc ^{:line 1225 :file "cli/agents-cli.bclj"} (into ^{:line 1225 :file "cli/agents-cli.bclj"} {} ^{:line 1225 :file "cli/agents-cli.bclj"} (System/getenv)) "NORTH_STAFFING_SOURCE" "file") :in ^{:line 1226 :file "cli/agents-cli.bclj"} (json/generate-string payload))]
  ^{:line 1227 :file "cli/agents-cli.bclj"} (if ^{:line 1227 :file "cli/agents-cli.bclj"} (not ^{:line 1227 :file "cli/agents-cli.bclj"} (:ok result)) ^{:line 1227 :file "cli/agents-cli.bclj"} (do
  ^{:line 1228 :file "cli/agents-cli.bclj"} (println ^{:line 1228 :file "cli/agents-cli.bclj"} (red ^{:line 1228 :file "cli/agents-cli.bclj"} (preflight-failure-message result routing-economics-preflight-timeout-ms)))
  ^{:line 1230 :file "cli/agents-cli.bclj"} (System/exit 1)))
  ^{:line 1231 :file "cli/agents-cli.bclj"} (try
  ^{:line 1232 :file "cli/agents-cli.bclj"} (let [receipt ^{:line 1232 :file "cli/agents-cli.bclj"} (json/parse-string ^{:line 1232 :file "cli/agents-cli.bclj"} (str/trim ^{:line 1232 :file "cli/agents-cli.bclj"} (:out result)) true)]
  ^{:line 1233 :file "cli/agents-cli.bclj"} (if ^{:line 1233 :file "cli/agents-cli.bclj"} (not ^{:line 1233 :file "cli/agents-cli.bclj"} (= 1 ^{:line 1233 :file "cli/agents-cli.bclj"} (:version receipt))) ^{:line 1233 :file "cli/agents-cli.bclj"} (do
  ^{:line 1234 :file "cli/agents-cli.bclj"} (throw ^{:line 1234 :file "cli/agents-cli.bclj"} (ex-info "missing immutable admission receipt" ^{:line 1234 :file "cli/agents-cli.bclj"} {}))))
  receipt)
  (catch Exception _
    ^{:line 1237 :file "cli/agents-cli.bclj"} (println ^{:line 1237 :file "cli/agents-cli.bclj"} (red "routing economics preflight returned an invalid admission receipt"))
    ^{:line 1238 :file "cli/agents-cli.bclj"} (System/exit 1)))))

^{:line 1240 :file "cli/agents-cli.bclj"} (defn- resolved-spawn-topology
  "Resolve the exact topology cmd-spawn will apply, including bespoke nearest\n   preset hydration. Delegate classification must inspect this value before it\n   may label a handoff atomic." [{:keys [topology nearest composition positionals]}]
  ^{:line 1244 :file "cli/agents-cli.bclj"} (let [role ^{:line 1244 :file "cli/agents-cli.bclj"} (first positionals)
   templates ^{:line 1245 :file "cli/agents-cli.bclj"} (or ^{:line 1245 :file "cli/agents-cli.bclj"} (orchestration-routing) ^{:line 1245 :file "cli/agents-cli.bclj"} {})
   supplied-composition ^{:line 1246 :file "cli/agents-cli.bclj"} (parse-json-input "--composition" composition)
   nearest-role ^{:line 1247 :file "cli/agents-cli.bclj"} (or nearest ^{:line 1247 :file "cli/agents-cli.bclj"} (:nearestTemplate supplied-composition))
   base ^{:line 1248 :file "cli/agents-cli.bclj"} (or ^{:line 1248 :file "cli/agents-cli.bclj"} (get templates role) ^{:line 1248 :file "cli/agents-cli.bclj"} (get templates nearest-role))]
  ^{:line 1249 :file "cli/agents-cli.bclj"} (or topology ^{:line 1249 :file "cli/agents-cli.bclj"} (:topology base))))

^{:line 1251 :file "cli/agents-cli.bclj"} (def canonical-orchestration-capabilities ^{:line 1255 :file "cli/agents-cli.bclj"} ["filesystem.read" "filesystem.search" "filesystem.write" "shell" "shell.readonly" "web" "coordination"])

^{:line 1257 :file "cli/agents-cli.bclj"} (def bespoke-fingerprint-version "v1")

^{:line 1258 :file "cli/agents-cli.bclj"} (def bespoke-fingerprint-domain "north:bespoke-contract:v1")

^{:line 1259 :file "cli/agents-cli.bclj"} (def edge-ascii-whitespace #"^[\u0009-\u000d\u0020]+|[\u0009-\u000d\u0020]+$")

^{:line 1261 :file "cli/agents-cli.bclj"} (defn- canonical-contract-text [value]
  ^{:line 1262 :file "cli/agents-cli.bclj"} (-> value ^{:line 1263 :file "cli/agents-cli.bclj"} (str/replace #"\r\n?" "\n") ^{:line 1264 :file "cli/agents-cli.bclj"} (java.text.Normalizer/normalize java.text.Normalizer$Form/NFC) ^{:line 1265 :file "cli/agents-cli.bclj"} (str/replace edge-ascii-whitespace "")))

^{:line 1267 :file "cli/agents-cli.bclj"} (defn- canonical-contract-list [values]
  ^{:line 1268 :file "cli/agents-cli.bclj"} (->> values ^{:line 1268 :file "cli/agents-cli.bclj"} (map canonical-contract-text) distinct sort vec))

^{:line 1270 :file "cli/agents-cli.bclj"} (defn- canonical-bespoke-contract [contract]
  ^{:line 1273 :file "cli/agents-cli.bclj"} (let [requested-capabilities ^{:line 1273 :file "cli/agents-cli.bclj"} (set ^{:line 1273 :file "cli/agents-cli.bclj"} (map canonical-contract-text ^{:line 1273 :file "cli/agents-cli.bclj"} (:capabilities contract)))]
  ^{:line 1274 :file "cli/agents-cli.bclj"} (array-map :responsibility ^{:line 1275 :file "cli/agents-cli.bclj"} (canonical-contract-text ^{:line 1275 :file "cli/agents-cli.bclj"} (:responsibility contract)) :deliverable ^{:line 1276 :file "cli/agents-cli.bclj"} (canonical-contract-text ^{:line 1276 :file "cli/agents-cli.bclj"} (:deliverable contract)) :capabilities ^{:line 1277 :file "cli/agents-cli.bclj"} (vec ^{:line 1277 :file "cli/agents-cli.bclj"} (filter ^{:line 1277 :file "cli/agents-cli.bclj"} (fn [capability] ^{:line 1278 :file "cli/agents-cli.bclj"} (contains? requested-capabilities capability)) canonical-orchestration-capabilities)) :mayDecide ^{:line 1280 :file "cli/agents-cli.bclj"} (canonical-contract-list ^{:line 1280 :file "cli/agents-cli.bclj"} (:mayDecide contract)) :mustEscalate ^{:line 1281 :file "cli/agents-cli.bclj"} (canonical-contract-list ^{:line 1281 :file "cli/agents-cli.bclj"} (:mustEscalate contract)) :doneWhen ^{:line 1282 :file "cli/agents-cli.bclj"} (canonical-contract-list ^{:line 1282 :file "cli/agents-cli.bclj"} (:doneWhen contract)) :report ^{:line 1283 :file "cli/agents-cli.bclj"} (canonical-contract-text ^{:line 1283 :file "cli/agents-cli.bclj"} (:report contract)))))

^{:line 1285 :file "cli/agents-cli.bclj"} (defn- utf8-segment [value]
  ^{:line 1286 :file "cli/agents-cli.bclj"} (str ^{:line 1286 :file "cli/agents-cli.bclj"} (alength ^{:line 1286 :file "cli/agents-cli.bclj"} (.getBytes value java.nio.charset.StandardCharsets/UTF_8)) ":" value))

^{:line 1288 :file "cli/agents-cli.bclj"} (defn- utf8-list-segment [values]
  ^{:line 1289 :file "cli/agents-cli.bclj"} (str ^{:line 1289 :file "cli/agents-cli.bclj"} (count values) ":" ^{:line 1289 :file "cli/agents-cli.bclj"} (apply str ^{:line 1289 :file "cli/agents-cli.bclj"} (map utf8-segment values))))

^{:line 1291 :file "cli/agents-cli.bclj"} (defn- canonical-bespoke-contract-payload [canonical]
  ^{:line 1292 :file "cli/agents-cli.bclj"} (str bespoke-fingerprint-domain "\n" "responsibility=" ^{:line 1293 :file "cli/agents-cli.bclj"} (utf8-segment ^{:line 1293 :file "cli/agents-cli.bclj"} (:responsibility canonical)) "\n" "deliverable=" ^{:line 1294 :file "cli/agents-cli.bclj"} (utf8-segment ^{:line 1294 :file "cli/agents-cli.bclj"} (:deliverable canonical)) "\n" "capabilities=" ^{:line 1295 :file "cli/agents-cli.bclj"} (utf8-list-segment ^{:line 1295 :file "cli/agents-cli.bclj"} (:capabilities canonical)) "\n" "mayDecide=" ^{:line 1296 :file "cli/agents-cli.bclj"} (utf8-list-segment ^{:line 1296 :file "cli/agents-cli.bclj"} (:mayDecide canonical)) "\n" "mustEscalate=" ^{:line 1297 :file "cli/agents-cli.bclj"} (utf8-list-segment ^{:line 1297 :file "cli/agents-cli.bclj"} (:mustEscalate canonical)) "\n" "doneWhen=" ^{:line 1298 :file "cli/agents-cli.bclj"} (utf8-list-segment ^{:line 1298 :file "cli/agents-cli.bclj"} (:doneWhen canonical)) "\n" "report=" ^{:line 1299 :file "cli/agents-cli.bclj"} (utf8-segment ^{:line 1299 :file "cli/agents-cli.bclj"} (:report canonical))))

^{:line 1301 :file "cli/agents-cli.bclj"} (defn- bespoke-contract-sha256 [contract]
  ^{:line 1302 :file "cli/agents-cli.bclj"} (let [canonical ^{:line 1302 :file "cli/agents-cli.bclj"} (canonical-bespoke-contract contract)
   bytes ^{:line 1303 :file "cli/agents-cli.bclj"} (.digest ^{:line 1303 :file "cli/agents-cli.bclj"} (doto ^{:line 1303 :file "cli/agents-cli.bclj"} (java.security.MessageDigest/getInstance "SHA-256")
  ^{:line 1304 :file "cli/agents-cli.bclj"} (.update ^{:line 1304 :file "cli/agents-cli.bclj"} (.getBytes ^{:line 1304 :file "cli/agents-cli.bclj"} (canonical-bespoke-contract-payload canonical) java.nio.charset.StandardCharsets/UTF_8))))]
  ^{:line 1306 :file "cli/agents-cli.bclj"} (apply str ^{:line 1306 :file "cli/agents-cli.bclj"} (map ^{:line 1306 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1306 :file "cli/agents-cli.bclj"} (format "%02x" ^{:line 1306 :file "cli/agents-cli.bclj"} (bit-and ^{:line 1306 :file "cli/agents-cli.bclj"} (int %1) 0xff))) bytes))))

^{:line 1308 :file "cli/agents-cli.bclj"} (def routing-override-fields ^{:line 1309 :file "cli/agents-cli.bclj"} #{"taskGrade" "domainRequirements" "tier" "reasoning" "posture"})

^{:line 1310 :file "cli/agents-cli.bclj"} (def routing-request-fields ^{:line 1311 :file "cli/agents-cli.bclj"} #{:role :taskGrade :domainRequirements :topology :tier :reasoning :posture :composition})

^{:line 1312 :file "cli/agents-cli.bclj"} (def bespoke-contract-fields ^{:line 1313 :file "cli/agents-cli.bclj"} #{:responsibility :deliverable :capabilities :mayDecide :mustEscalate :doneWhen :report})

^{:line 1314 :file "cli/agents-cli.bclj"} (def role-id-pattern #"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")

^{:line 1316 :file "cli/agents-cli.bclj"} (defn- valid-string-list? [value require-items?]
  ^{:line 1319 :file "cli/agents-cli.bclj"} (and ^{:line 1319 :file "cli/agents-cli.bclj"} (sequential? value) ^{:line 1320 :file "cli/agents-cli.bclj"} (or ^{:line 1320 :file "cli/agents-cli.bclj"} (not require-items?) ^{:line 1320 :file "cli/agents-cli.bclj"} (seq value)) ^{:line 1321 :file "cli/agents-cli.bclj"} (every? string? value) ^{:line 1322 :file "cli/agents-cli.bclj"} (let [normalized ^{:line 1322 :file "cli/agents-cli.bclj"} (mapv canonical-contract-text value)]
  ^{:line 1323 :file "cli/agents-cli.bclj"} (and ^{:line 1323 :file "cli/agents-cli.bclj"} (every? seq normalized) ^{:line 1324 :file "cli/agents-cli.bclj"} (= ^{:line 1324 :file "cli/agents-cli.bclj"} (count normalized) ^{:line 1324 :file "cli/agents-cli.bclj"} (count ^{:line 1324 :file "cli/agents-cli.bclj"} (set normalized)))))))

^{:line 1326 :file "cli/agents-cli.bclj"} (defn- valid-contract-string-list? [value]
  ^{:line 1327 :file "cli/agents-cli.bclj"} (and ^{:line 1327 :file "cli/agents-cli.bclj"} (sequential? value) ^{:line 1327 :file "cli/agents-cli.bclj"} (seq value) ^{:line 1327 :file "cli/agents-cli.bclj"} (every? string? value) ^{:line 1328 :file "cli/agents-cli.bclj"} (let [normalized ^{:line 1328 :file "cli/agents-cli.bclj"} (mapv canonical-contract-text value)]
  ^{:line 1329 :file "cli/agents-cli.bclj"} (and ^{:line 1329 :file "cli/agents-cli.bclj"} (every? seq normalized) ^{:line 1330 :file "cli/agents-cli.bclj"} (= ^{:line 1330 :file "cli/agents-cli.bclj"} (count normalized) ^{:line 1330 :file "cli/agents-cli.bclj"} (count ^{:line 1330 :file "cli/agents-cli.bclj"} (set normalized)))))))

^{:line 1332 :file "cli/agents-cli.bclj"} (defn- valid-contract-text? [value]
  ^{:line 1333 :file "cli/agents-cli.bclj"} (and ^{:line 1333 :file "cli/agents-cli.bclj"} (string? value) ^{:line 1333 :file "cli/agents-cli.bclj"} (seq ^{:line 1333 :file "cli/agents-cli.bclj"} (canonical-contract-text value))))

^{:line 1335 :file "cli/agents-cli.bclj"} (defn- non-empty-string? [value]
  ^{:line 1336 :file "cli/agents-cli.bclj"} (and ^{:line 1336 :file "cli/agents-cli.bclj"} (string? value) ^{:line 1336 :file "cli/agents-cli.bclj"} (not ^{:line 1336 :file "cli/agents-cli.bclj"} (str/blank? value))))

^{:line 1338 :file "cli/agents-cli.bclj"} (defn- topology-capability-problem [topology capabilities]
  ^{:line 1341 :file "cli/agents-cli.bclj"} (let [caps ^{:line 1341 :file "cli/agents-cli.bclj"} (set capabilities)
   missing-closure ^{:line 1342 :file "cli/agents-cli.bclj"} (fn [surface required] ^{:line 1345 :file "cli/agents-cli.bclj"} (let [missing ^{:line 1345 :file "cli/agents-cli.bclj"} (remove ^{:line 1345 :file "cli/agents-cli.bclj"} (fn [capability] ^{:line 1346 :file "cli/agents-cli.bclj"} (contains? caps capability)) required)]
  ^{:line 1348 :file "cli/agents-cli.bclj"} (if ^{:line 1348 :file "cli/agents-cli.bclj"} (and ^{:line 1348 :file "cli/agents-cli.bclj"} (caps surface) ^{:line 1348 :file "cli/agents-cli.bclj"} (seq missing)) ^{:line 1348 :file "cli/agents-cli.bclj"} (do
  ^{:line 1349 :file "cli/agents-cli.bclj"} (str "composition.contract.capabilities: capability list is not closed; missing implied " ^{:line 1350 :file "cli/agents-cli.bclj"} (str/join ", " missing))))))]
  ^{:line 1351 :file "cli/agents-cli.bclj"} (cond
  ^{:line 1352 :file "cli/agents-cli.bclj"} (and ^{:line 1352 :file "cli/agents-cli.bclj"} (caps "shell") ^{:line 1352 :file "cli/agents-cli.bclj"} (caps "shell.readonly")) "shell and shell.readonly are mutually exclusive"
  ^{:line 1354 :file "cli/agents-cli.bclj"} (and ^{:line 1354 :file "cli/agents-cli.bclj"} (= topology "orchestrator") ^{:line 1354 :file "cli/agents-cli.bclj"} (not ^{:line 1354 :file "cli/agents-cli.bclj"} (contains? caps "coordination"))) "orchestrator topology requires coordination capability"
  ^{:line 1356 :file "cli/agents-cli.bclj"} (and ^{:line 1356 :file "cli/agents-cli.bclj"} (= topology "orchestrator") ^{:line 1356 :file "cli/agents-cli.bclj"} (caps "filesystem.write")) "orchestrator topology forbids filesystem.write capability"
  ^{:line 1358 :file "cli/agents-cli.bclj"} (and ^{:line 1358 :file "cli/agents-cli.bclj"} (= topology "orchestrator") ^{:line 1358 :file "cli/agents-cli.bclj"} (caps "shell")) "orchestrator topology forbids unrestricted shell capability"
  ^{:line 1360 :file "cli/agents-cli.bclj"} (and ^{:line 1360 :file "cli/agents-cli.bclj"} (= topology "worker") ^{:line 1360 :file "cli/agents-cli.bclj"} (caps "coordination")) "worker topology forbids coordination capability"
  :else ^{:line 1363 :file "cli/agents-cli.bclj"} (or ^{:line 1363 :file "cli/agents-cli.bclj"} (missing-closure "shell" ^{:line 1364 :file "cli/agents-cli.bclj"} ["filesystem.read" "filesystem.search" "filesystem.write"]) ^{:line 1365 :file "cli/agents-cli.bclj"} (missing-closure "shell.readonly" ^{:line 1366 :file "cli/agents-cli.bclj"} ["filesystem.read" "filesystem.search"])))))

^{:line 1368 :file "cli/agents-cli.bclj"} (def ^:dynamic *delegate-request* nil)

^{:line 1369 :file "cli/agents-cli.bclj"} (def ^:dynamic *selected-routing-request* nil)

^{:line 1370 :file "cli/agents-cli.bclj"} (def ^:dynamic *selected-routing-assessment* nil)

^{:line 1371 :file "cli/agents-cli.bclj"} (declare resolve-delegate-referent! resolve-recursive-child-referent! delegate-brief)

^{:line 1373 :file "cli/agents-cli.bclj"} (defn referent-title-verdict
  "A spawn attribution target exists only when the coordinator's exact-subject\n  projection contains exactly one entity_kind=referent and one nonblank title.\n  Read through the same :show projection as the daemon-first CLI; the independent\n  :resolved index can lag that projection and must not turn a visible Referent\n  into a false absence. A failed read is :unreadable — a degraded coordinator is\n  not an absent Referent." [id]
  ^{:line 1380 :file "cli/agents-cli.bclj"} (try
  ^{:line 1381 :file "cli/agents-cli.bclj"} (let [subject ^{:line 1381 :file "cli/agents-cli.bclj"} (str "@" ^{:line 1381 :file "cli/agents-cli.bclj"} (str/replace-first ^{:line 1381 :file "cli/agents-cli.bclj"} (str id) #"^@" ""))
   rows ^{:line 1382 :file "cli/agents-cli.bclj"} (north.coord/show-rows! ^{:line 1382 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) subject)
   kinds ^{:line 1383 :file "cli/agents-cli.bclj"} (mapv second ^{:line 1383 :file "cli/agents-cli.bclj"} (filter ^{:line 1383 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1383 :file "cli/agents-cli.bclj"} (= "entity_kind" ^{:line 1383 :file "cli/agents-cli.bclj"} (first %1))) rows))
   titles ^{:line 1384 :file "cli/agents-cli.bclj"} (mapv second ^{:line 1385 :file "cli/agents-cli.bclj"} (filter ^{:line 1385 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1385 :file "cli/agents-cli.bclj"} (= "title" ^{:line 1385 :file "cli/agents-cli.bclj"} (first %1))) rows))]
  ^{:line 1387 :file "cli/agents-cli.bclj"} (if ^{:line 1387 :file "cli/agents-cli.bclj"} (and ^{:line 1387 :file "cli/agents-cli.bclj"} (= ^{:line 1387 :file "cli/agents-cli.bclj"} ["referent"] kinds) ^{:line 1388 :file "cli/agents-cli.bclj"} (= 1 ^{:line 1388 :file "cli/agents-cli.bclj"} (count titles)) ^{:line 1389 :file "cli/agents-cli.bclj"} (not ^{:line 1389 :file "cli/agents-cli.bclj"} (str/blank? ^{:line 1389 :file "cli/agents-cli.bclj"} (first titles)))) :titled :untitled))
  (catch Exception _
    :unreadable)))

^{:line 1394 :file "cli/agents-cli.bclj"} (defn title-bearing-referent? [id]
  ^{:line 1395 :file "cli/agents-cli.bclj"} (= :titled ^{:line 1395 :file "cli/agents-cli.bclj"} (referent-title-verdict id)))

^{:line 1397 :file "cli/agents-cli.bclj"} (defn warn-unarmed-notify! [notify]
  ^{:line 1398 :file "cli/agents-cli.bclj"} (if notify ^{:line 1398 :file "cli/agents-cli.bclj"} (do
  ^{:line 1399 :file "cli/agents-cli.bclj"} (let [route ^{:line 1399 :file "cli/agents-cli.bclj"} (north.message-routing/require-live-address ^{:line 1400 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) notify)]
  ^{:line 1401 :file "cli/agents-cli.bclj"} (if ^{:line 1401 :file "cli/agents-cli.bclj"} (false? ^{:line 1401 :file "cli/agents-cli.bclj"} (:live route)) ^{:line 1401 :file "cli/agents-cli.bclj"} (do
  ^{:line 1402 :file "cli/agents-cli.bclj"} (println ^{:line 1403 :file "cli/agents-cli.bclj"} (ylw ^{:line 1404 :file "cli/agents-cli.bclj"} (str "NOTIFY TARGET " notify " HAS NO ARMED LISTENER — completions will not wake it; arm: north-arm " notify)))))))))

^{:line 1408 :file "cli/agents-cli.bclj"} (defn cmd-spawn! [args]
  ^{:line 1409 :file "cli/agents-cli.bclj"} (north.topology-authority/require-coordination! "spawn")
  ^{:line 1410 :file "cli/agents-cli.bclj"} (let [{:keys [dry? notify provider target model taskGrade domains topology tier reasoning posture composition assessment pinEvidence rationale nearest contract overrideReason promotion-specified? promotionCandidate positionals referent ad-hoc?]} ^{:line 1410 :file "cli/agents-cli.bclj"} (parse-spawn-args args)
   selected-request *selected-routing-request*
   _ ^{:line 1412 :file "cli/agents-cli.bclj"} (if ^{:line 1412 :file "cli/agents-cli.bclj"} (and selected-request ^{:line 1413 :file "cli/agents-cli.bclj"} (or ^{:line 1413 :file "cli/agents-cli.bclj"} (not ^{:line 1413 :file "cli/agents-cli.bclj"} (map? selected-request)) ^{:line 1414 :file "cli/agents-cli.bclj"} (not= routing-request-fields ^{:line 1414 :file "cli/agents-cli.bclj"} (set ^{:line 1414 :file "cli/agents-cli.bclj"} (keys selected-request))))) ^{:line 1412 :file "cli/agents-cli.bclj"} (do
  ^{:line 1415 :file "cli/agents-cli.bclj"} (println ^{:line 1415 :file "cli/agents-cli.bclj"} (red "selected delegation run design must contain exactly the eight routing fields"))
  ^{:line 1416 :file "cli/agents-cli.bclj"} (System/exit 1)))
   [first-positional second-positional & remaining-positionals] positionals
   invoked-role ^{:line 1422 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1422 :file "cli/agents-cli.bclj"} (:role selected-request) first-positional)
   prompt ^{:line 1423 :file "cli/agents-cli.bclj"} (if selected-request first-positional second-positional)
   extra ^{:line 1424 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1424 :file "cli/agents-cli.bclj"} (rest positionals) remaining-positionals)
   _ ^{:line 1425 :file "cli/agents-cli.bclj"} (if ^{:line 1425 :file "cli/agents-cli.bclj"} (and referent ad-hoc?) ^{:line 1425 :file "cli/agents-cli.bclj"} (do
  ^{:line 1426 :file "cli/agents-cli.bclj"} (println ^{:line 1426 :file "cli/agents-cli.bclj"} (red "choose one: --referent <id> binds this run, --ad-hoc runs it unattributed"))
  ^{:line 1427 :file "cli/agents-cli.bclj"} (System/exit 2)))
   _ ^{:line 1432 :file "cli/agents-cli.bclj"} (if ^{:line 1432 :file "cli/agents-cli.bclj"} (not ^{:line 1432 :file "cli/agents-cli.bclj"} (or referent ad-hoc? *delegate-request* ^{:line 1433 :file "cli/agents-cli.bclj"} (str/blank? ^{:line 1433 :file "cli/agents-cli.bclj"} (str invoked-role)) ^{:line 1433 :file "cli/agents-cli.bclj"} (str/blank? ^{:line 1433 :file "cli/agents-cli.bclj"} (str prompt)))) ^{:line 1432 :file "cli/agents-cli.bclj"} (do
  ^{:line 1434 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 1435 :file "cli/agents-cli.bclj"} (println ^{:line 1435 :file "cli/agents-cli.bclj"} (red "North spawn requires --referent <id> so its effort is attributable"))
  ^{:line 1436 :file "cli/agents-cli.bclj"} (println ^{:line 1436 :file "cli/agents-cli.bclj"} (dim "  pass --referent <id> to bind this run to a workstream,"))
  ^{:line 1437 :file "cli/agents-cli.bclj"} (println ^{:line 1437 :file "cli/agents-cli.bclj"} (dim "  or --ad-hoc to deliberately run it unattributed.")))
  ^{:line 1438 :file "cli/agents-cli.bclj"} (System/exit 2)))
   _ ^{:line 1447 :file "cli/agents-cli.bclj"} (if ^{:line 1447 :file "cli/agents-cli.bclj"} (and referent ^{:line 1448 :file "cli/agents-cli.bclj"} (nil? ^{:line 1448 :file "cli/agents-cli.bclj"} (re-matches #"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|\d{4}-\d{2}-\d{2}-\d{6}" ^{:line 1450 :file "cli/agents-cli.bclj"} (str/replace-first ^{:line 1450 :file "cli/agents-cli.bclj"} (str referent) #"^@" "")))) ^{:line 1447 :file "cli/agents-cli.bclj"} (do
  ^{:line 1451 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 1452 :file "cli/agents-cli.bclj"} (println ^{:line 1452 :file "cli/agents-cli.bclj"} (red ^{:line 1452 :file "cli/agents-cli.bclj"} (str "--referent " referent " is not a canonical referent id")))
  ^{:line 1453 :file "cli/agents-cli.bclj"} (println ^{:line 1453 :file "cli/agents-cli.bclj"} (dim "  a prefix is recorded verbatim on the run and never joins back to its referent,"))
  ^{:line 1454 :file "cli/agents-cli.bclj"} (println ^{:line 1454 :file "cli/agents-cli.bclj"} (dim ^{:line 1454 :file "cli/agents-cli.bclj"} (str "  so the run would look bound and be orphaned. Resolve it first:")))
  ^{:line 1455 :file "cli/agents-cli.bclj"} (println ^{:line 1455 :file "cli/agents-cli.bclj"} (dim ^{:line 1455 :file "cli/agents-cli.bclj"} (str "    north fact show " referent "   # prints the full id, or names the ambiguity"))))
  ^{:line 1456 :file "cli/agents-cli.bclj"} (System/exit 2)))
   _ ^{:line 1464 :file "cli/agents-cli.bclj"} (if referent ^{:line 1464 :file "cli/agents-cli.bclj"} (do
  ^{:line 1465 :file "cli/agents-cli.bclj"} (let [bare ^{:line 1465 :file "cli/agents-cli.bclj"} (str/replace-first ^{:line 1465 :file "cli/agents-cli.bclj"} (str referent) #"^@" "")
   verdict ^{:line 1466 :file "cli/agents-cli.bclj"} (referent-title-verdict bare)]
  ^{:line 1467 :file "cli/agents-cli.bclj"} (if ^{:line 1467 :file "cli/agents-cli.bclj"} (= :unreadable verdict) ^{:line 1467 :file "cli/agents-cli.bclj"} (do
  ^{:line 1468 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 1469 :file "cli/agents-cli.bclj"} (println ^{:line 1469 :file "cli/agents-cli.bclj"} (red ^{:line 1469 :file "cli/agents-cli.bclj"} (str "--referent " bare " could not be read through the coordinator")))
  ^{:line 1470 :file "cli/agents-cli.bclj"} (println ^{:line 1470 :file "cli/agents-cli.bclj"} (dim "  the exact-subject projection failed; this is a degraded coordinator, not a missing referent."))
  ^{:line 1471 :file "cli/agents-cli.bclj"} (println ^{:line 1471 :file "cli/agents-cli.bclj"} (dim "  check `north system doctor`, then retry.")))
  ^{:line 1472 :file "cli/agents-cli.bclj"} (System/exit 75)))
  ^{:line 1473 :file "cli/agents-cli.bclj"} (if ^{:line 1473 :file "cli/agents-cli.bclj"} (= :untitled verdict) ^{:line 1473 :file "cli/agents-cli.bclj"} (do
  ^{:line 1474 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 1475 :file "cli/agents-cli.bclj"} (println ^{:line 1475 :file "cli/agents-cli.bclj"} (red ^{:line 1475 :file "cli/agents-cli.bclj"} (str "--referent " bare " names no exact Referent")))
  ^{:line 1476 :file "cli/agents-cli.bclj"} (println ^{:line 1476 :file "cli/agents-cli.bclj"} (dim "  it must carry exactly entity_kind=referent and one nonblank title."))
  ^{:line 1477 :file "cli/agents-cli.bclj"} (println ^{:line 1477 :file "cli/agents-cli.bclj"} (dim "  capture it first, or correct the id:"))
  ^{:line 1478 :file "cli/agents-cli.bclj"} (println ^{:line 1478 :file "cli/agents-cli.bclj"} (dim ^{:line 1478 :file "cli/agents-cli.bclj"} (str "    north fact show " ^{:line 1478 :file "cli/agents-cli.bclj"} (subs bare 0 ^{:line 1478 :file "cli/agents-cli.bclj"} (min 8 ^{:line 1478 :file "cli/agents-cli.bclj"} (count bare))) "   # find the real id by prefix"))))
  ^{:line 1480 :file "cli/agents-cli.bclj"} (System/exit 2))))))
   catalog ^{:line 1481 :file "cli/agents-cli.bclj"} (orchestration-catalog)
   dt ^{:line 1482 :file "cli/agents-cli.bclj"} (or ^{:line 1482 :file "cli/agents-cli.bclj"} (orchestration-routing) ^{:line 1482 :file "cli/agents-cli.bclj"} {})
   raw-supplied-composition ^{:line 1483 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1484 :file "cli/agents-cli.bclj"} (:composition selected-request) ^{:line 1485 :file "cli/agents-cli.bclj"} (parse-json-input "--composition" composition))
   routing-assessment ^{:line 1486 :file "cli/agents-cli.bclj"} (if selected-request *selected-routing-assessment* ^{:line 1488 :file "cli/agents-cli.bclj"} (parse-json-input "--assessment" assessment))
   pin-evidence ^{:line 1489 :file "cli/agents-cli.bclj"} (parse-json-input "--pin-evidence" pinEvidence)
   override-reason-conflict ^{:line 1490 :file "cli/agents-cli.bclj"} (and ^{:line 1490 :file "cli/agents-cli.bclj"} (= "template" ^{:line 1490 :file "cli/agents-cli.bclj"} (:kind raw-supplied-composition)) overrideReason ^{:line 1492 :file "cli/agents-cli.bclj"} (contains? raw-supplied-composition :overrideReason) ^{:line 1493 :file "cli/agents-cli.bclj"} (not= overrideReason ^{:line 1493 :file "cli/agents-cli.bclj"} (:overrideReason raw-supplied-composition)))
   supplied-composition ^{:line 1494 :file "cli/agents-cli.bclj"} (cond-> raw-supplied-composition ^{:line 1495 :file "cli/agents-cli.bclj"} (and ^{:line 1495 :file "cli/agents-cli.bclj"} (= "template" ^{:line 1495 :file "cli/agents-cli.bclj"} (:kind raw-supplied-composition)) overrideReason ^{:line 1497 :file "cli/agents-cli.bclj"} (not ^{:line 1497 :file "cli/agents-cli.bclj"} (contains? raw-supplied-composition :overrideReason))) ^{:line 1498 :file "cli/agents-cli.bclj"} (assoc :overrideReason overrideReason))
   supplied-template ^{:line 1499 :file "cli/agents-cli.bclj"} (if ^{:line 1499 :file "cli/agents-cli.bclj"} (= "template" ^{:line 1499 :file "cli/agents-cli.bclj"} (:kind supplied-composition)) ^{:line 1499 :file "cli/agents-cli.bclj"} (do
  ^{:line 1500 :file "cli/agents-cli.bclj"} (get dt ^{:line 1500 :file "cli/agents-cli.bclj"} (:id supplied-composition))))
   supplied-contract ^{:line 1501 :file "cli/agents-cli.bclj"} (parse-json-input "--contract" contract)
   canonical ^{:line 1502 :file "cli/agents-cli.bclj"} (get dt invoked-role)
   default-bespoke? ^{:line 1503 :file "cli/agents-cli.bclj"} (and invoked-role ^{:line 1503 :file "cli/agents-cli.bclj"} (nil? canonical))
   composition-kind ^{:line 1504 :file "cli/agents-cli.bclj"} (or ^{:line 1504 :file "cli/agents-cli.bclj"} (:kind supplied-composition) ^{:line 1505 :file "cli/agents-cli.bclj"} (if default-bespoke? "bespoke" "template"))
   bespoke? ^{:line 1506 :file "cli/agents-cli.bclj"} (= "bespoke" composition-kind)
   template? ^{:line 1507 :file "cli/agents-cli.bclj"} (= "template" composition-kind)
   bespoke-reason ^{:line 1508 :file "cli/agents-cli.bclj"} (or rationale ^{:line 1508 :file "cli/agents-cli.bclj"} (:bespokeReason supplied-composition))
   nearest-role ^{:line 1509 :file "cli/agents-cli.bclj"} (or nearest ^{:line 1509 :file "cli/agents-cli.bclj"} (:nearestTemplate supplied-composition))
   nearest-template ^{:line 1510 :file "cli/agents-cli.bclj"} (get dt nearest-role)
   contract-value ^{:line 1511 :file "cli/agents-cli.bclj"} (or supplied-contract ^{:line 1511 :file "cli/agents-cli.bclj"} (:contract supplied-composition))
   catalog-capability-order ^{:line 1512 :file "cli/agents-cli.bclj"} (vec ^{:line 1512 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1512 :file "cli/agents-cli.bclj"} [:vocabulary :capabilities]))
   capability-values ^{:line 1513 :file "cli/agents-cli.bclj"} (set canonical-orchestration-capabilities)
   promotion-value ^{:line 1514 :file "cli/agents-cli.bclj"} (if promotion-specified? promotionCandidate ^{:line 1515 :file "cli/agents-cli.bclj"} (if ^{:line 1515 :file "cli/agents-cli.bclj"} (and ^{:line 1515 :file "cli/agents-cli.bclj"} (map? supplied-composition) ^{:line 1516 :file "cli/agents-cli.bclj"} (contains? supplied-composition :promotionCandidate)) ^{:line 1517 :file "cli/agents-cli.bclj"} (:promotionCandidate supplied-composition) false))
   template-base ^{:line 1519 :file "cli/agents-cli.bclj"} (if template? ^{:line 1519 :file "cli/agents-cli.bclj"} (do
  ^{:line 1519 :file "cli/agents-cli.bclj"} (or supplied-template canonical)))
   base ^{:line 1520 :file "cli/agents-cli.bclj"} (or template-base nearest-template)
   preset-grade ^{:line 1521 :file "cli/agents-cli.bclj"} (:taskGrade base)
   preset-tier ^{:line 1522 :file "cli/agents-cli.bclj"} (:tier base)
   preset-posture ^{:line 1523 :file "cli/agents-cli.bclj"} (:posture base)
   preset-topology ^{:line 1524 :file "cli/agents-cli.bclj"} (:topology base)
   preset-deliberation ^{:line 1525 :file "cli/agents-cli.bclj"} (:deliberation base)
   selected-grade ^{:line 1526 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1526 :file "cli/agents-cli.bclj"} (:taskGrade selected-request) ^{:line 1527 :file "cli/agents-cli.bclj"} (or taskGrade preset-grade))
   selected-tier ^{:line 1528 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1528 :file "cli/agents-cli.bclj"} (:tier selected-request) ^{:line 1529 :file "cli/agents-cli.bclj"} (or tier preset-tier))
   selected-topology ^{:line 1530 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1530 :file "cli/agents-cli.bclj"} (:topology selected-request) ^{:line 1531 :file "cli/agents-cli.bclj"} (or topology preset-topology))
   selected-role ^{:line 1532 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1532 :file "cli/agents-cli.bclj"} (:role selected-request) invoked-role)
   selected-posture ^{:line 1533 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1533 :file "cli/agents-cli.bclj"} (:posture selected-request) ^{:line 1534 :file "cli/agents-cli.bclj"} (or posture preset-posture ^{:line 1534 :file "cli/agents-cli.bclj"} (:posture ^{:line 1534 :file "cli/agents-cli.bclj"} (:defaults catalog))))
   selected-reasoning ^{:line 1535 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1535 :file "cli/agents-cli.bclj"} (:reasoning selected-request) ^{:line 1536 :file "cli/agents-cli.bclj"} (or reasoning preset-deliberation))
   selected-domains ^{:line 1537 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1538 :file "cli/agents-cli.bclj"} (vec ^{:line 1538 :file "cli/agents-cli.bclj"} (:domainRequirements selected-request)) ^{:line 1539 :file "cli/agents-cli.bclj"} (vec ^{:line 1539 :file "cli/agents-cli.bclj"} (distinct domains)))
   missing-bespoke-axes ^{:line 1540 :file "cli/agents-cli.bclj"} (if ^{:line 1540 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1540 :file "cli/agents-cli.bclj"} (nil? nearest-template) ^{:line 1540 :file "cli/agents-cli.bclj"} (not selected-request)) ^{:line 1540 :file "cli/agents-cli.bclj"} (do
  ^{:line 1541 :file "cli/agents-cli.bclj"} (seq ^{:line 1541 :file "cli/agents-cli.bclj"} (keep ^{:line 1541 :file "cli/agents-cli.bclj"} (fn [[label value]] ^{:line 1541 :file "cli/agents-cli.bclj"} (if ^{:line 1541 :file "cli/agents-cli.bclj"} (nil? value) ^{:line 1541 :file "cli/agents-cli.bclj"} (do
  label))) ^{:line 1542 :file "cli/agents-cli.bclj"} [^{:line 1542 :file "cli/agents-cli.bclj"} ["--task-grade" taskGrade] ^{:line 1543 :file "cli/agents-cli.bclj"} ["--topology" topology] ^{:line 1544 :file "cli/agents-cli.bclj"} ["--tier" tier] ^{:line 1545 :file "cli/agents-cli.bclj"} ["--reasoning" reasoning] ^{:line 1546 :file "cli/agents-cli.bclj"} ["--posture" posture]]))))
   route-problem ^{:line 1547 :file "cli/agents-cli.bclj"} (north.orchestration-staffing/unsupported-route-problem selected-tier selected-reasoning)
   actual-overrides ^{:line 1549 :file "cli/agents-cli.bclj"} (if template? ^{:line 1549 :file "cli/agents-cli.bclj"} (do
  ^{:line 1550 :file "cli/agents-cli.bclj"} (vec ^{:line 1550 :file "cli/agents-cli.bclj"} (keep ^{:line 1550 :file "cli/agents-cli.bclj"} (fn [[field selected preset]] ^{:line 1550 :file "cli/agents-cli.bclj"} (if ^{:line 1550 :file "cli/agents-cli.bclj"} (not= selected preset) ^{:line 1550 :file "cli/agents-cli.bclj"} (do
  field))) ^{:line 1551 :file "cli/agents-cli.bclj"} [^{:line 1551 :file "cli/agents-cli.bclj"} ["taskGrade" selected-grade ^{:line 1551 :file "cli/agents-cli.bclj"} (:taskGrade template-base)] ^{:line 1552 :file "cli/agents-cli.bclj"} ["domainRequirements" selected-domains ^{:line 1552 :file "cli/agents-cli.bclj"} []] ^{:line 1553 :file "cli/agents-cli.bclj"} ["tier" selected-tier ^{:line 1553 :file "cli/agents-cli.bclj"} (:tier template-base)] ^{:line 1554 :file "cli/agents-cli.bclj"} ["reasoning" selected-reasoning ^{:line 1554 :file "cli/agents-cli.bclj"} (:deliberation template-base)] ^{:line 1555 :file "cli/agents-cli.bclj"} ["posture" selected-posture ^{:line 1555 :file "cli/agents-cli.bclj"} (:posture template-base)]]))))
   generated-composition ^{:line 1556 :file "cli/agents-cli.bclj"} (if default-bespoke? ^{:line 1557 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1557 :file "cli/agents-cli.bclj"} {:kind "bespoke" :id invoked-role :bespokeReason bespoke-reason :promotionCandidate promotion-value :contract contract-value} nearest-role ^{:line 1560 :file "cli/agents-cli.bclj"} (assoc :nearestTemplate nearest-role)) ^{:line 1561 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1561 :file "cli/agents-cli.bclj"} {:kind "template" :id selected-role :overrides actual-overrides} ^{:line 1563 :file "cli/agents-cli.bclj"} (seq actual-overrides) ^{:line 1563 :file "cli/agents-cli.bclj"} (assoc :overrideReason overrideReason)))
   selected-composition ^{:line 1564 :file "cli/agents-cli.bclj"} (if selected-request ^{:line 1565 :file "cli/agents-cli.bclj"} (:composition selected-request) ^{:line 1566 :file "cli/agents-cli.bclj"} (or supplied-composition generated-composition))
   selected-capabilities ^{:line 1567 :file "cli/agents-cli.bclj"} (if template? ^{:line 1567 :file "cli/agents-cli.bclj"} (:capabilities template-base) ^{:line 1568 :file "cli/agents-cli.bclj"} (:capabilities contract-value))
   normalized-selected-capabilities ^{:line 1569 :file "cli/agents-cli.bclj"} (if ^{:line 1569 :file "cli/agents-cli.bclj"} (and ^{:line 1569 :file "cli/agents-cli.bclj"} (sequential? selected-capabilities) ^{:line 1569 :file "cli/agents-cli.bclj"} (every? string? selected-capabilities)) ^{:line 1569 :file "cli/agents-cli.bclj"} (do
  ^{:line 1570 :file "cli/agents-cli.bclj"} (mapv canonical-contract-text selected-capabilities)))
   capability-problem ^{:line 1571 :file "cli/agents-cli.bclj"} (if normalized-selected-capabilities ^{:line 1571 :file "cli/agents-cli.bclj"} (do
  ^{:line 1572 :file "cli/agents-cli.bclj"} (or ^{:line 1572 :file "cli/agents-cli.bclj"} (topology-capability-problem selected-topology normalized-selected-capabilities) ^{:line 1573 :file "cli/agents-cli.bclj"} (north.orchestration-staffing/posture-capability-problem selected-posture normalized-selected-capabilities))))
   allowed-composition-fields ^{:line 1575 :file "cli/agents-cli.bclj"} (case composition-kind
    "template" ^{:line 1576 :file "cli/agents-cli.bclj"} #{:kind :id :overrides :overrideReason}
    "bespoke" ^{:line 1577 :file "cli/agents-cli.bclj"} #{:kind :id :nearestTemplate :bespokeReason :promotionCandidate :contract}
    ^{:line 1578 :file "cli/agents-cli.bclj"} #{})
   unknown-composition-fields ^{:line 1579 :file "cli/agents-cli.bclj"} (if ^{:line 1579 :file "cli/agents-cli.bclj"} (map? selected-composition) ^{:line 1579 :file "cli/agents-cli.bclj"} (do
  ^{:line 1580 :file "cli/agents-cli.bclj"} (seq ^{:line 1580 :file "cli/agents-cli.bclj"} (remove ^{:line 1580 :file "cli/agents-cli.bclj"} (fn [field] ^{:line 1581 :file "cli/agents-cli.bclj"} (contains? allowed-composition-fields field)) ^{:line 1582 :file "cli/agents-cli.bclj"} (keys selected-composition)))))
   declared-overrides ^{:line 1583 :file "cli/agents-cli.bclj"} (if ^{:line 1583 :file "cli/agents-cli.bclj"} (map? selected-composition) ^{:line 1583 :file "cli/agents-cli.bclj"} (do
  ^{:line 1583 :file "cli/agents-cli.bclj"} (:overrides selected-composition)))
   contract-fields ^{:line 1584 :file "cli/agents-cli.bclj"} (if ^{:line 1584 :file "cli/agents-cli.bclj"} (map? ^{:line 1584 :file "cli/agents-cli.bclj"} (:contract selected-composition)) ^{:line 1584 :file "cli/agents-cli.bclj"} (do
  ^{:line 1584 :file "cli/agents-cli.bclj"} (set ^{:line 1584 :file "cli/agents-cli.bclj"} (keys ^{:line 1584 :file "cli/agents-cli.bclj"} (:contract selected-composition)))))]
  ^{:line 1585 :file "cli/agents-cli.bclj"} (cond
  ^{:line 1586 :file "cli/agents-cli.bclj"} (or ^{:line 1586 :file "cli/agents-cli.bclj"} (nil? invoked-role) ^{:line 1586 :file "cli/agents-cli.bclj"} (nil? prompt) ^{:line 1586 :file "cli/agents-cli.bclj"} (seq extra)) ^{:line 1587 :file "cli/agents-cli.bclj"} (do
  ^{:line 1587 :file "cli/agents-cli.bclj"} (println ^{:line 1587 :file "cli/agents-cli.bclj"} (red "usage:") "north agent spawn <role> \"<prompt>\" [--task-grade G] [--domain D] [--topology T] [--tier T] [--reasoning R] [--posture P] [--override-reason WHY] [--composition JSON|@file] [--assessment JSON|@file] [--rationale WHY] [--nearest PRESET] [--contract JSON|@file] [--promotion-candidate|--no-promotion-candidate] [--provider P] [--target ACCOUNT] [--model MODEL] [--pin-evidence JSON|@file] [--notify PEER] [--dry-run]")
  ^{:line 1588 :file "cli/agents-cli.bclj"} (println "role is functional identity independent of composition: catalogued and novel roles may use template or bespoke compositions")
  ^{:line 1589 :file "cli/agents-cli.bclj"} (println "roles:" ^{:line 1589 :file "cli/agents-cli.bclj"} (str/join " " ^{:line 1589 :file "cli/agents-cli.bclj"} (sort ^{:line 1589 :file "cli/agents-cli.bclj"} (keys dt)))))
  ^{:line 1590 :file "cli/agents-cli.bclj"} (^{:line 1590 :file "cli/agents-cli.bclj"} #{"orchestrator" "worker"} invoked-role) ^{:line 1591 :file "cli/agents-cli.bclj"} (do
  ^{:line 1591 :file "cli/agents-cli.bclj"} (println ^{:line 1591 :file "cli/agents-cli.bclj"} (red ^{:line 1591 :file "cli/agents-cli.bclj"} (str invoked-role " is a topology, not a role")))
  ^{:line 1592 :file "cli/agents-cli.bclj"} (println ^{:line 1592 :file "cli/agents-cli.bclj"} (if ^{:line 1592 :file "cli/agents-cli.bclj"} (= invoked-role "orchestrator") "use director for decomposition/reconciliation, or choose a worker function for atomic work" "choose the worker function that names the deliverable, such as executor, implementer, integrator, or verifier"))
  ^{:line 1595 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1596 :file "cli/agents-cli.bclj"} (= invoked-role "researcher") ^{:line 1597 :file "cli/agents-cli.bclj"} (do
  ^{:line 1597 :file "cli/agents-cli.bclj"} (println ^{:line 1597 :file "cli/agents-cli.bclj"} (red "researcher is retired because it was ambiguous"))
  ^{:line 1598 :file "cli/agents-cli.bclj"} (println "use scout for source gathering, analyst for deep mechanism research, or scientist for cutting-edge inquiry")
  ^{:line 1599 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1600 :file "cli/agents-cli.bclj"} (and invoked-role ^{:line 1600 :file "cli/agents-cli.bclj"} (nil? ^{:line 1600 :file "cli/agents-cli.bclj"} (re-matches role-id-pattern invoked-role))) ^{:line 1601 :file "cli/agents-cli.bclj"} (do
  ^{:line 1601 :file "cli/agents-cli.bclj"} (println ^{:line 1601 :file "cli/agents-cli.bclj"} (red "role must be a lowercase kebab-case Orchestration role id"))
  ^{:line 1601 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1602 :file "cli/agents-cli.bclj"} (nil? catalog) ^{:line 1602 :file "cli/agents-cli.bclj"} (do
  ^{:line 1602 :file "cli/agents-cli.bclj"} (println ^{:line 1602 :file "cli/agents-cli.bclj"} (red ^{:line 1602 :file "cli/agents-cli.bclj"} (str "Delegation run-composition catalog unavailable: " ORCHESTRATION-STAFFING)))
  ^{:line 1602 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1603 :file "cli/agents-cli.bclj"} (not= canonical-orchestration-capabilities catalog-capability-order) ^{:line 1604 :file "cli/agents-cli.bclj"} (do
  ^{:line 1604 :file "cli/agents-cli.bclj"} (println ^{:line 1604 :file "cli/agents-cli.bclj"} (red "Orchestration capability vocabulary order disagrees with North's canonical fingerprint vocabulary"))
  ^{:line 1605 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1606 :file "cli/agents-cli.bclj"} (and template? ^{:line 1606 :file "cli/agents-cli.bclj"} (or rationale nearest contract promotion-specified?)) ^{:line 1607 :file "cli/agents-cli.bclj"} (do
  ^{:line 1607 :file "cli/agents-cli.bclj"} (println ^{:line 1607 :file "cli/agents-cli.bclj"} (red "--nearest, --rationale, --contract, and promotion decisions apply only to bespoke compositions"))
  ^{:line 1607 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1608 :file "cli/agents-cli.bclj"} (and template? ^{:line 1608 :file "cli/agents-cli.bclj"} (some? topology)) ^{:line 1609 :file "cli/agents-cli.bclj"} (do
  ^{:line 1609 :file "cli/agents-cli.bclj"} (println ^{:line 1609 :file "cli/agents-cli.bclj"} (red "--topology applies only to bespoke compositions; stock-template topology is fixed"))
  ^{:line 1610 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1611 :file "cli/agents-cli.bclj"} (and bespoke? overrideReason) ^{:line 1612 :file "cli/agents-cli.bclj"} (do
  ^{:line 1612 :file "cli/agents-cli.bclj"} (println ^{:line 1612 :file "cli/agents-cli.bclj"} (red "--override-reason applies only to template axis overrides"))
  ^{:line 1612 :file "cli/agents-cli.bclj"} (System/exit 1))
  override-reason-conflict ^{:line 1614 :file "cli/agents-cli.bclj"} (do
  ^{:line 1614 :file "cli/agents-cli.bclj"} (println ^{:line 1614 :file "cli/agents-cli.bclj"} (red "--override-reason conflicts with composition.overrideReason"))
  ^{:line 1615 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1616 :file "cli/agents-cli.bclj"} (and bespoke? nearest-role ^{:line 1616 :file "cli/agents-cli.bclj"} (nil? nearest-template)) ^{:line 1617 :file "cli/agents-cli.bclj"} (do
  ^{:line 1617 :file "cli/agents-cli.bclj"} (println ^{:line 1617 :file "cli/agents-cli.bclj"} (red ^{:line 1617 :file "cli/agents-cli.bclj"} (str "unknown nearest template: " nearest-role)))
  ^{:line 1617 :file "cli/agents-cli.bclj"} (System/exit 1))
  missing-bespoke-axes ^{:line 1619 :file "cli/agents-cli.bclj"} (do
  ^{:line 1619 :file "cli/agents-cli.bclj"} (println ^{:line 1619 :file "cli/agents-cli.bclj"} (red ^{:line 1619 :file "cli/agents-cli.bclj"} (str "bespoke composition without --nearest must explicitly set: " ^{:line 1620 :file "cli/agents-cli.bclj"} (str/join ", " missing-bespoke-axes))))
  ^{:line 1621 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1622 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1622 :file "cli/agents-cli.bclj"} (not ^{:line 1622 :file "cli/agents-cli.bclj"} (non-empty-string? bespoke-reason))) ^{:line 1623 :file "cli/agents-cli.bclj"} (do
  ^{:line 1623 :file "cli/agents-cli.bclj"} (println ^{:line 1623 :file "cli/agents-cli.bclj"} (red ^{:line 1623 :file "cli/agents-cli.bclj"} (str "bespoke composition " ^{:line 1623 :file "cli/agents-cli.bclj"} (:id selected-composition) " requires --rationale or composition.bespokeReason")))
  ^{:line 1623 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1624 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1624 :file "cli/agents-cli.bclj"} (nil? contract-value)) ^{:line 1625 :file "cli/agents-cli.bclj"} (do
  ^{:line 1625 :file "cli/agents-cli.bclj"} (println ^{:line 1625 :file "cli/agents-cli.bclj"} (red ^{:line 1625 :file "cli/agents-cli.bclj"} (str "bespoke composition " ^{:line 1625 :file "cli/agents-cli.bclj"} (:id selected-composition) " requires --contract JSON|@file or composition.contract")))
  ^{:line 1625 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1626 :file "cli/agents-cli.bclj"} (and supplied-composition rationale ^{:line 1626 :file "cli/agents-cli.bclj"} (not= rationale ^{:line 1626 :file "cli/agents-cli.bclj"} (:bespokeReason supplied-composition))) ^{:line 1627 :file "cli/agents-cli.bclj"} (do
  ^{:line 1627 :file "cli/agents-cli.bclj"} (println ^{:line 1627 :file "cli/agents-cli.bclj"} (red "--rationale conflicts with composition.bespokeReason"))
  ^{:line 1627 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1628 :file "cli/agents-cli.bclj"} (and supplied-composition nearest ^{:line 1628 :file "cli/agents-cli.bclj"} (not= nearest ^{:line 1628 :file "cli/agents-cli.bclj"} (:nearestTemplate supplied-composition))) ^{:line 1629 :file "cli/agents-cli.bclj"} (do
  ^{:line 1629 :file "cli/agents-cli.bclj"} (println ^{:line 1629 :file "cli/agents-cli.bclj"} (red "--nearest conflicts with composition.nearestTemplate"))
  ^{:line 1629 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1630 :file "cli/agents-cli.bclj"} (and supplied-composition supplied-contract ^{:line 1630 :file "cli/agents-cli.bclj"} (not= supplied-contract ^{:line 1630 :file "cli/agents-cli.bclj"} (:contract supplied-composition))) ^{:line 1631 :file "cli/agents-cli.bclj"} (do
  ^{:line 1631 :file "cli/agents-cli.bclj"} (println ^{:line 1631 :file "cli/agents-cli.bclj"} (red "--contract conflicts with composition.contract"))
  ^{:line 1631 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1632 :file "cli/agents-cli.bclj"} (and supplied-composition promotion-specified? ^{:line 1632 :file "cli/agents-cli.bclj"} (not= promotionCandidate ^{:line 1632 :file "cli/agents-cli.bclj"} (:promotionCandidate supplied-composition))) ^{:line 1633 :file "cli/agents-cli.bclj"} (do
  ^{:line 1633 :file "cli/agents-cli.bclj"} (println ^{:line 1633 :file "cli/agents-cli.bclj"} (red "promotion flag conflicts with composition.promotionCandidate"))
  ^{:line 1633 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1634 :file "cli/agents-cli.bclj"} (and target ^{:line 1634 :file "cli/agents-cli.bclj"} (str/blank? target)) ^{:line 1635 :file "cli/agents-cli.bclj"} (do
  ^{:line 1635 :file "cli/agents-cli.bclj"} (println ^{:line 1635 :file "cli/agents-cli.bclj"} (red "--target requires a non-empty account target"))
  ^{:line 1635 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1636 :file "cli/agents-cli.bclj"} (not ^{:line 1636 :file "cli/agents-cli.bclj"} (contains? ^{:line 1636 :file "cli/agents-cli.bclj"} (set ^{:line 1636 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1636 :file "cli/agents-cli.bclj"} [:vocabulary :taskGrades])) selected-grade)) ^{:line 1637 :file "cli/agents-cli.bclj"} (do
  ^{:line 1637 :file "cli/agents-cli.bclj"} (println ^{:line 1637 :file "cli/agents-cli.bclj"} (red ^{:line 1637 :file "cli/agents-cli.bclj"} (str "invalid taskGrade: " selected-grade)))
  ^{:line 1637 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1638 :file "cli/agents-cli.bclj"} (not ^{:line 1638 :file "cli/agents-cli.bclj"} (contains? ^{:line 1638 :file "cli/agents-cli.bclj"} (set ^{:line 1638 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1638 :file "cli/agents-cli.bclj"} [:vocabulary :topologies])) selected-topology)) ^{:line 1639 :file "cli/agents-cli.bclj"} (do
  ^{:line 1639 :file "cli/agents-cli.bclj"} (println ^{:line 1639 :file "cli/agents-cli.bclj"} (red ^{:line 1639 :file "cli/agents-cli.bclj"} (str "invalid topology: " selected-topology)))
  ^{:line 1639 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1640 :file "cli/agents-cli.bclj"} (not ^{:line 1640 :file "cli/agents-cli.bclj"} (contains? ^{:line 1640 :file "cli/agents-cli.bclj"} (set ^{:line 1640 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1640 :file "cli/agents-cli.bclj"} [:vocabulary :semanticTiers])) selected-tier)) ^{:line 1641 :file "cli/agents-cli.bclj"} (do
  ^{:line 1641 :file "cli/agents-cli.bclj"} (println ^{:line 1641 :file "cli/agents-cli.bclj"} (red ^{:line 1641 :file "cli/agents-cli.bclj"} (str "invalid tier: " selected-tier)))
  ^{:line 1641 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1642 :file "cli/agents-cli.bclj"} (not ^{:line 1642 :file "cli/agents-cli.bclj"} (contains? ^{:line 1642 :file "cli/agents-cli.bclj"} (set ^{:line 1642 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1642 :file "cli/agents-cli.bclj"} [:vocabulary :deliberations])) selected-reasoning)) ^{:line 1643 :file "cli/agents-cli.bclj"} (do
  ^{:line 1643 :file "cli/agents-cli.bclj"} (println ^{:line 1643 :file "cli/agents-cli.bclj"} (red ^{:line 1643 :file "cli/agents-cli.bclj"} (str "invalid reasoning: " selected-reasoning)))
  ^{:line 1643 :file "cli/agents-cli.bclj"} (System/exit 1))
  route-problem ^{:line 1645 :file "cli/agents-cli.bclj"} (do
  ^{:line 1645 :file "cli/agents-cli.bclj"} (println ^{:line 1645 :file "cli/agents-cli.bclj"} (red route-problem))
  ^{:line 1645 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1646 :file "cli/agents-cli.bclj"} (not ^{:line 1646 :file "cli/agents-cli.bclj"} (contains? ^{:line 1646 :file "cli/agents-cli.bclj"} (set ^{:line 1646 :file "cli/agents-cli.bclj"} (get-in catalog ^{:line 1646 :file "cli/agents-cli.bclj"} [:vocabulary :postures])) selected-posture)) ^{:line 1647 :file "cli/agents-cli.bclj"} (do
  ^{:line 1647 :file "cli/agents-cli.bclj"} (println ^{:line 1647 :file "cli/agents-cli.bclj"} (red ^{:line 1647 :file "cli/agents-cli.bclj"} (str "invalid posture: " selected-posture)))
  ^{:line 1647 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1648 :file "cli/agents-cli.bclj"} (not ^{:line 1648 :file "cli/agents-cli.bclj"} (map? selected-composition)) ^{:line 1649 :file "cli/agents-cli.bclj"} (do
  ^{:line 1649 :file "cli/agents-cli.bclj"} (println ^{:line 1649 :file "cli/agents-cli.bclj"} (red "composition must be a JSON object"))
  ^{:line 1649 :file "cli/agents-cli.bclj"} (System/exit 1))
  unknown-composition-fields ^{:line 1651 :file "cli/agents-cli.bclj"} (do
  ^{:line 1651 :file "cli/agents-cli.bclj"} (println ^{:line 1651 :file "cli/agents-cli.bclj"} (red ^{:line 1651 :file "cli/agents-cli.bclj"} (str "composition contains unknown fields: " ^{:line 1651 :file "cli/agents-cli.bclj"} (str/join ", " ^{:line 1651 :file "cli/agents-cli.bclj"} (map name unknown-composition-fields)))))
  ^{:line 1651 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1652 :file "cli/agents-cli.bclj"} (and ^{:line 1652 :file "cli/agents-cli.bclj"} (= "template" composition-kind) ^{:line 1652 :file "cli/agents-cli.bclj"} (nil? ^{:line 1652 :file "cli/agents-cli.bclj"} (get dt ^{:line 1652 :file "cli/agents-cli.bclj"} (:id selected-composition)))) ^{:line 1653 :file "cli/agents-cli.bclj"} (do
  ^{:line 1653 :file "cli/agents-cli.bclj"} (println ^{:line 1653 :file "cli/agents-cli.bclj"} (red ^{:line 1653 :file "cli/agents-cli.bclj"} (str "unknown template composition.id " ^{:line 1653 :file "cli/agents-cli.bclj"} (:id selected-composition))))
  ^{:line 1653 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1654 :file "cli/agents-cli.bclj"} (and template? ^{:line 1654 :file "cli/agents-cli.bclj"} (not ^{:line 1654 :file "cli/agents-cli.bclj"} (valid-string-list? declared-overrides false))) ^{:line 1655 :file "cli/agents-cli.bclj"} (do
  ^{:line 1655 :file "cli/agents-cli.bclj"} (println ^{:line 1655 :file "cli/agents-cli.bclj"} (red "template composition.overrides must be an array of unique routing-axis names"))
  ^{:line 1655 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1656 :file "cli/agents-cli.bclj"} (and template? ^{:line 1656 :file "cli/agents-cli.bclj"} (some ^{:line 1656 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1656 :file "cli/agents-cli.bclj"} (not ^{:line 1656 :file "cli/agents-cli.bclj"} (contains? routing-override-fields %1))) declared-overrides)) ^{:line 1657 :file "cli/agents-cli.bclj"} (do
  ^{:line 1657 :file "cli/agents-cli.bclj"} (println ^{:line 1657 :file "cli/agents-cli.bclj"} (red ^{:line 1657 :file "cli/agents-cli.bclj"} (str "composition.overrides may contain only: " ^{:line 1658 :file "cli/agents-cli.bclj"} (str/join ", " ^{:line 1658 :file "cli/agents-cli.bclj"} (sort routing-override-fields)))))
  ^{:line 1659 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1660 :file "cli/agents-cli.bclj"} (and template? ^{:line 1660 :file "cli/agents-cli.bclj"} (not= ^{:line 1660 :file "cli/agents-cli.bclj"} (set actual-overrides) ^{:line 1660 :file "cli/agents-cli.bclj"} (set declared-overrides))) ^{:line 1661 :file "cli/agents-cli.bclj"} (do
  ^{:line 1661 :file "cli/agents-cli.bclj"} (println ^{:line 1661 :file "cli/agents-cli.bclj"} (red ^{:line 1661 :file "cli/agents-cli.bclj"} (str "composition.overrides must exactly record changed template axes: " ^{:line 1662 :file "cli/agents-cli.bclj"} (if ^{:line 1662 :file "cli/agents-cli.bclj"} (seq actual-overrides) ^{:line 1662 :file "cli/agents-cli.bclj"} (str/join ", " actual-overrides) "none"))))
  ^{:line 1663 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1664 :file "cli/agents-cli.bclj"} (and template? ^{:line 1664 :file "cli/agents-cli.bclj"} (seq actual-overrides) ^{:line 1664 :file "cli/agents-cli.bclj"} (not ^{:line 1664 :file "cli/agents-cli.bclj"} (non-empty-string? ^{:line 1664 :file "cli/agents-cli.bclj"} (:overrideReason selected-composition)))) ^{:line 1665 :file "cli/agents-cli.bclj"} (do
  ^{:line 1665 :file "cli/agents-cli.bclj"} (println ^{:line 1665 :file "cli/agents-cli.bclj"} (red ^{:line 1665 :file "cli/agents-cli.bclj"} (str "template axis override requires --override-reason (changed: " ^{:line 1666 :file "cli/agents-cli.bclj"} (str/join ", " actual-overrides) ")")))
  ^{:line 1666 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1667 :file "cli/agents-cli.bclj"} (and template? ^{:line 1667 :file "cli/agents-cli.bclj"} (empty? actual-overrides) ^{:line 1667 :file "cli/agents-cli.bclj"} (contains? selected-composition :overrideReason)) ^{:line 1668 :file "cli/agents-cli.bclj"} (do
  ^{:line 1668 :file "cli/agents-cli.bclj"} (println ^{:line 1668 :file "cli/agents-cli.bclj"} (red "unchanged preset must not carry --override-reason"))
  ^{:line 1668 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1669 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1669 :file "cli/agents-cli.bclj"} (not ^{:line 1669 :file "cli/agents-cli.bclj"} (boolean? ^{:line 1669 :file "cli/agents-cli.bclj"} (:promotionCandidate selected-composition)))) ^{:line 1670 :file "cli/agents-cli.bclj"} (do
  ^{:line 1670 :file "cli/agents-cli.bclj"} (println ^{:line 1670 :file "cli/agents-cli.bclj"} (red "bespoke composition.promotionCandidate must be explicit boolean"))
  ^{:line 1670 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1671 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1671 :file "cli/agents-cli.bclj"} (not= bespoke-contract-fields contract-fields)) ^{:line 1672 :file "cli/agents-cli.bclj"} (do
  ^{:line 1672 :file "cli/agents-cli.bclj"} (println ^{:line 1672 :file "cli/agents-cli.bclj"} (red "bespoke composition.contract must contain exactly responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, and report"))
  ^{:line 1672 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1673 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1673 :file "cli/agents-cli.bclj"} (some ^{:line 1673 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1673 :file "cli/agents-cli.bclj"} (not ^{:line 1673 :file "cli/agents-cli.bclj"} (valid-contract-text? ^{:line 1673 :file "cli/agents-cli.bclj"} (get-in selected-composition ^{:line 1673 :file "cli/agents-cli.bclj"} [:contract %1])))) ^{:line 1674 :file "cli/agents-cli.bclj"} [:responsibility :deliverable :report])) ^{:line 1675 :file "cli/agents-cli.bclj"} (do
  ^{:line 1675 :file "cli/agents-cli.bclj"} (println ^{:line 1675 :file "cli/agents-cli.bclj"} (red "bespoke composition.contract requires non-empty responsibility, deliverable, and report"))
  ^{:line 1675 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1676 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1676 :file "cli/agents-cli.bclj"} (some ^{:line 1676 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1676 :file "cli/agents-cli.bclj"} (not ^{:line 1676 :file "cli/agents-cli.bclj"} (valid-contract-string-list? ^{:line 1676 :file "cli/agents-cli.bclj"} (get-in selected-composition ^{:line 1676 :file "cli/agents-cli.bclj"} [:contract %1])))) ^{:line 1677 :file "cli/agents-cli.bclj"} [:mayDecide :mustEscalate :doneWhen])) ^{:line 1678 :file "cli/agents-cli.bclj"} (do
  ^{:line 1678 :file "cli/agents-cli.bclj"} (println ^{:line 1678 :file "cli/agents-cli.bclj"} (red "bespoke composition.contract requires non-empty mayDecide, mustEscalate, and doneWhen lists"))
  ^{:line 1678 :file "cli/agents-cli.bclj"} (System/exit 1))
  ^{:line 1679 :file "cli/agents-cli.bclj"} (and bespoke? ^{:line 1680 :file "cli/agents-cli.bclj"} (or ^{:line 1680 :file "cli/agents-cli.bclj"} (not ^{:line 1680 :file "cli/agents-cli.bclj"} (valid-contract-string-list? selected-capabilities)) ^{:line 1681 :file "cli/agents-cli.bclj"} (some ^{:line 1681 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 1681 :file "cli/agents-cli.bclj"} (not ^{:line 1681 :file "cli/agents-cli.bclj"} (contains? capability-values %1))) normalized-selected-capabilities))) ^{:line 1682 :file "cli/agents-cli.bclj"} (do
  ^{:line 1682 :file "cli/agents-cli.bclj"} (println ^{:line 1682 :file "cli/agents-cli.bclj"} (red "bespoke composition.contract capabilities must be non-empty and canonical"))
  ^{:line 1682 :file "cli/agents-cli.bclj"} (System/exit 1))
  capability-problem ^{:line 1684 :file "cli/agents-cli.bclj"} (do
  ^{:line 1684 :file "cli/agents-cli.bclj"} (println ^{:line 1684 :file "cli/agents-cli.bclj"} (red capability-problem))
  ^{:line 1684 :file "cli/agents-cli.bclj"} (System/exit 1))
  :else ^{:line 1686 :file "cli/agents-cli.bclj"} (let [canonical-contract ^{:line 1686 :file "cli/agents-cli.bclj"} (if bespoke? ^{:line 1686 :file "cli/agents-cli.bclj"} (do
  ^{:line 1687 :file "cli/agents-cli.bclj"} (canonical-bespoke-contract ^{:line 1687 :file "cli/agents-cli.bclj"} (:contract selected-composition))))
   _notify-warning ^{:line 1688 :file "cli/agents-cli.bclj"} (warn-unarmed-notify! notify)
   contract-sha256 ^{:line 1689 :file "cli/agents-cli.bclj"} (if canonical-contract ^{:line 1689 :file "cli/agents-cli.bclj"} (do
  ^{:line 1690 :file "cli/agents-cli.bclj"} (bespoke-contract-sha256 ^{:line 1690 :file "cli/agents-cli.bclj"} (:contract selected-composition))))
   spawn-composition ^{:line 1691 :file "cli/agents-cli.bclj"} (if selected-request selected-composition ^{:line 1693 :file "cli/agents-cli.bclj"} (if bespoke? ^{:line 1694 :file "cli/agents-cli.bclj"} (assoc selected-composition :contract canonical-contract) selected-composition))
   routing-metadata ^{:line 1696 :file "cli/agents-cli.bclj"} {:role selected-role :taskGrade selected-grade :domainRequirements selected-domains :topology selected-topology :tier selected-tier :reasoning selected-reasoning :posture selected-posture :composition spawn-composition}
   _receipt ^{:line 1700 :file "cli/agents-cli.bclj"} (preflight-routing-economics! routing-metadata routing-assessment pin-evidence provider target model dry?)
   _capabilities ^{:line 1702 :file "cli/agents-cli.bclj"} (require-pinned-provider-capabilities! provider target normalized-selected-capabilities)
   struggle-policy ^{:line 1704 :file "cli/agents-cli.bclj"} (resolve-struggle-policy! selected-topology)
   catalog-model ^{:line 1705 :file "cli/agents-cli.bclj"} (:model base)
   effective-model ^{:line 1706 :file "cli/agents-cli.bclj"} (or model ^{:line 1707 :file "cli/agents-cli.bclj"} (if ^{:line 1707 :file "cli/agents-cli.bclj"} (and ^{:line 1707 :file "cli/agents-cli.bclj"} (not ^{:line 1707 :file "cli/agents-cli.bclj"} (:semantic base)) ^{:line 1708 :file "cli/agents-cli.bclj"} (not ^{:line 1708 :file "cli/agents-cli.bclj"} (:orchestration-preset base))) ^{:line 1707 :file "cli/agents-cli.bclj"} (do
  catalog-model)))
   synthetic-effort ^{:line 1710 :file "cli/agents-cli.bclj"} (:effort base)
   synthetic-reasoning ^{:line 1711 :file "cli/agents-cli.bclj"} (:reasoning base)
   orchestration-preset ^{:line 1712 :file "cli/agents-cli.bclj"} (:orchestration-preset base)
   semantic ^{:line 1713 :file "cli/agents-cli.bclj"} (:semantic base)
   delegate-binding ^{:line 1714 :file "cli/agents-cli.bclj"} (cond
  *delegate-request* ^{:line 1716 :file "cli/agents-cli.bclj"} (resolve-delegate-referent! *delegate-request* dry?)
  ^{:line 1718 :file "cli/agents-cli.bclj"} (= "orchestrator" ^{:line 1719 :file "cli/agents-cli.bclj"} (north.topology-authority/current-topology)) ^{:line 1720 :file "cli/agents-cli.bclj"} (resolve-recursive-child-referent! prompt dry?))
   effective-prompt ^{:line 1721 :file "cli/agents-cli.bclj"} (if delegate-binding ^{:line 1722 :file "cli/agents-cli.bclj"} (delegate-brief *delegate-request* delegate-binding) prompt)
   aid ^{:line 1724 :file "cli/agents-cli.bclj"} (north.spawn-process/create-agent-id "lane")
   env ^{:line 1725 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1725 :file "cli/agents-cli.bclj"} {"AGENT_ID" aid "NORTH_STAFFING_SOURCE" "file" "NORTH_STRUGGLE_POLICY_EXPECTED" ^{:line 1732 :file "cli/agents-cli.bclj"} (:canonical struggle-policy)} selected-role ^{:line 1733 :file "cli/agents-cli.bclj"} (assoc "AGENT_IDENTITY_ROLE" selected-role) selected-grade ^{:line 1734 :file "cli/agents-cli.bclj"} (assoc "AGENT_TASK_GRADE" selected-grade) selected-role ^{:line 1735 :file "cli/agents-cli.bclj"} (assoc "AGENT_DOMAIN_REQUIREMENTS" ^{:line 1735 :file "cli/agents-cli.bclj"} (json/generate-string selected-domains)) selected-topology ^{:line 1736 :file "cli/agents-cli.bclj"} (assoc "AGENT_TOPOLOGY" selected-topology) selected-tier ^{:line 1737 :file "cli/agents-cli.bclj"} (assoc "AGENT_TIER" selected-tier) selected-role ^{:line 1738 :file "cli/agents-cli.bclj"} (assoc "AGENT_ROLE" selected-role) selected-posture ^{:line 1739 :file "cli/agents-cli.bclj"} (assoc "AGENT_POSTURE" selected-posture) spawn-composition ^{:line 1740 :file "cli/agents-cli.bclj"} (assoc "AGENT_COMPOSITION" ^{:line 1740 :file "cli/agents-cli.bclj"} (json/generate-string spawn-composition)) effective-model ^{:line 1741 :file "cli/agents-cli.bclj"} (assoc "AGENT_MODEL" effective-model) selected-reasoning ^{:line 1742 :file "cli/agents-cli.bclj"} (assoc "AGENT_REASONING" selected-reasoning) provider ^{:line 1743 :file "cli/agents-cli.bclj"} (assoc "AGENT_PROVIDER" provider) target ^{:line 1744 :file "cli/agents-cli.bclj"} (assoc "AGENT_TARGET" target) routing-assessment ^{:line 1745 :file "cli/agents-cli.bclj"} (assoc "AGENT_ROUTING_ASSESSMENT" ^{:line 1745 :file "cli/agents-cli.bclj"} (json/generate-string routing-assessment)) pin-evidence ^{:line 1746 :file "cli/agents-cli.bclj"} (assoc "NORTH_ROUTING_PIN_EVIDENCE" ^{:line 1746 :file "cli/agents-cli.bclj"} (json/generate-string pin-evidence)) notify ^{:line 1747 :file "cli/agents-cli.bclj"} (assoc "AGENT_COORDINATOR" notify) referent ^{:line 1752 :file "cli/agents-cli.bclj"} (assoc "AGENT_REFERENT" referent "AGENT_REFERENT_PROVENANCE" "exact") ad-hoc? ^{:line 1754 :file "cli/agents-cli.bclj"} (assoc "AGENT_REFERENT_PROVENANCE" "ad-hoc") delegate-binding ^{:line 1755 :file "cli/agents-cli.bclj"} (assoc "NORTH_DELEGATE_REFERENT_ID" ^{:line 1755 :file "cli/agents-cli.bclj"} (:id delegate-binding)))
   immediate-coordinator ^{:line 1756 :file "cli/agents-cli.bclj"} (or notify ^{:line 1756 :file "cli/agents-cli.bclj"} (System/getenv "AGENT_ID") ^{:line 1757 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_AGENT_ID"))
   child-env ^{:line 1758 :file "cli/agents-cli.bclj"} (north.managed-child-env/child ^{:line 1759 :file "cli/agents-cli.bclj"} (into ^{:line 1759 :file "cli/agents-cli.bclj"} {} ^{:line 1759 :file "cli/agents-cli.bclj"} (System/getenv)) immediate-coordinator env)
   spawn-ts ^{:line 1760 :file "cli/agents-cli.bclj"} (str NORTH "/sdk/src/spawn.ts")
   display-env ^{:line 1761 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1761 :file "cli/agents-cli.bclj"} (dissoc env "NORTH_STRUGGLE_POLICY_EXPECTED") bespoke? ^{:line 1762 :file "cli/agents-cli.bclj"} (assoc "AGENT_COMPOSITION" "REDACTED_BESPOKE_CONTRACT") routing-assessment ^{:line 1763 :file "cli/agents-cli.bclj"} (assoc "AGENT_ROUTING_ASSESSMENT" "RECORDED") pin-evidence ^{:line 1764 :file "cli/agents-cli.bclj"} (assoc "NORTH_ROUTING_PIN_EVIDENCE" "RECORDED"))
   envs ^{:line 1765 :file "cli/agents-cli.bclj"} (str/join " " ^{:line 1765 :file "cli/agents-cli.bclj"} (map ^{:line 1765 :file "cli/agents-cli.bclj"} (fn [[k v]] ^{:line 1765 :file "cli/agents-cli.bclj"} (str k "=" v)) ^{:line 1765 :file "cli/agents-cli.bclj"} (sort display-env)))
   dry-route ^{:line 1766 :file "cli/agents-cli.bclj"} (dry-resolved-route provider selected-tier effective-model selected-reasoning)
   fallback-base ^{:line 1769 :file "cli/agents-cli.bclj"} (into ^{:line 1769 :file "cli/agents-cli.bclj"} {} ^{:line 1769 :file "cli/agents-cli.bclj"} (remove ^{:line 1769 :file "cli/agents-cli.bclj"} (comp nil? val) ^{:line 1770 :file "cli/agents-cli.bclj"} {"kind" "lane" "role" selected-role "provider" ^{:line 1771 :file "cli/agents-cli.bclj"} (or ^{:line 1771 :file "cli/agents-cli.bclj"} (:provider dry-route) provider "auto") "provider_target" ^{:line 1772 :file "cli/agents-cli.bclj"} (or target ^{:line 1772 :file "cli/agents-cli.bclj"} (:provider dry-route) provider "auto") "live_input" ^{:line 1773 :file "cli/agents-cli.bclj"} (if ^{:line 1773 :file "cli/agents-cli.bclj"} (= "anthropic" ^{:line 1773 :file "cli/agents-cli.bclj"} (or ^{:line 1773 :file "cli/agents-cli.bclj"} (:provider dry-route) provider)) "streaming" "unsupported") "live_input_state" ^{:line 1776 :file "cli/agents-cli.bclj"} (if ^{:line 1776 :file "cli/agents-cli.bclj"} (= "anthropic" ^{:line 1776 :file "cli/agents-cli.bclj"} (or ^{:line 1776 :file "cli/agents-cli.bclj"} (:provider dry-route) provider)) "pending" "frozen") "live_input_epoch" ^{:line 1779 :file "cli/agents-cli.bclj"} (str ^{:line 1779 :file "cli/agents-cli.bclj"} (java.util.UUID/randomUUID)) "model" ^{:line 1780 :file "cli/agents-cli.bclj"} (or ^{:line 1780 :file "cli/agents-cli.bclj"} (:model dry-route) ^{:line 1780 :file "cli/agents-cli.bclj"} (if selected-tier ^{:line 1780 :file "cli/agents-cli.bclj"} (do
  ^{:line 1780 :file "cli/agents-cli.bclj"} (str "tier:" selected-tier))) "unresolved") "effort" ^{:line 1781 :file "cli/agents-cli.bclj"} (or ^{:line 1781 :file "cli/agents-cli.bclj"} (:effort dry-route) selected-reasoning) "composition_kind" ^{:line 1782 :file "cli/agents-cli.bclj"} (:kind spawn-composition) "composition_id" ^{:line 1783 :file "cli/agents-cli.bclj"} (:id spawn-composition) "composition_overrides" ^{:line 1784 :file "cli/agents-cli.bclj"} (if ^{:line 1784 :file "cli/agents-cli.bclj"} (= "template" ^{:line 1784 :file "cli/agents-cli.bclj"} (:kind spawn-composition)) ^{:line 1784 :file "cli/agents-cli.bclj"} (do
  ^{:line 1785 :file "cli/agents-cli.bclj"} (json/generate-string ^{:line 1785 :file "cli/agents-cli.bclj"} (:overrides spawn-composition)))) "composition_override_reason" ^{:line 1786 :file "cli/agents-cli.bclj"} (if ^{:line 1786 :file "cli/agents-cli.bclj"} (= "template" ^{:line 1786 :file "cli/agents-cli.bclj"} (:kind spawn-composition)) ^{:line 1786 :file "cli/agents-cli.bclj"} (do
  ^{:line 1787 :file "cli/agents-cli.bclj"} (:overrideReason spawn-composition))) "bespoke_reason" ^{:line 1788 :file "cli/agents-cli.bclj"} (if ^{:line 1788 :file "cli/agents-cli.bclj"} (= "bespoke" ^{:line 1788 :file "cli/agents-cli.bclj"} (:kind spawn-composition)) ^{:line 1788 :file "cli/agents-cli.bclj"} (do
  ^{:line 1789 :file "cli/agents-cli.bclj"} (:bespokeReason spawn-composition))) "nearest_template" ^{:line 1790 :file "cli/agents-cli.bclj"} (if ^{:line 1790 :file "cli/agents-cli.bclj"} (= "bespoke" ^{:line 1790 :file "cli/agents-cli.bclj"} (:kind spawn-composition)) ^{:line 1790 :file "cli/agents-cli.bclj"} (do
  ^{:line 1791 :file "cli/agents-cli.bclj"} (:nearestTemplate spawn-composition))) "promotion_candidate" ^{:line 1792 :file "cli/agents-cli.bclj"} (if ^{:line 1792 :file "cli/agents-cli.bclj"} (= "bespoke" ^{:line 1792 :file "cli/agents-cli.bclj"} (:kind spawn-composition)) ^{:line 1792 :file "cli/agents-cli.bclj"} (do
  ^{:line 1793 :file "cli/agents-cli.bclj"} (str ^{:line 1793 :file "cli/agents-cli.bclj"} (:promotionCandidate spawn-composition)))) "composition_contract_sha256" contract-sha256 "composition_contract_fingerprint_version" ^{:line 1795 :file "cli/agents-cli.bclj"} (if contract-sha256 ^{:line 1795 :file "cli/agents-cli.bclj"} (do
  bespoke-fingerprint-version)) "composition_contract_fingerprint_domain" ^{:line 1796 :file "cli/agents-cli.bclj"} (if contract-sha256 ^{:line 1796 :file "cli/agents-cli.bclj"} (do
  bespoke-fingerprint-domain)) "repo" ^{:line 1800 :file "cli/agents-cli.bclj"} (current-repo) "goal" effective-prompt "spawned_at" ^{:line 1801 :file "cli/agents-cli.bclj"} (str ^{:line 1801 :file "cli/agents-cli.bclj"} (java.time.Instant/now)) "display_handle" "dry-run" "display_name" "dry-run"}))
   fallback-facts ^{:line 1803 :file "cli/agents-cli.bclj"} (assoc fallback-base "identity_manifest_sha256" ^{:line 1804 :file "cli/agents-cli.bclj"} (north.agent-provenance/manifest-sha256 fallback-base))]
  ^{:line 1805 :file "cli/agents-cli.bclj"} (println ^{:line 1805 :file "cli/agents-cli.bclj"} (dim "# orchestration dials for role") ^{:line 1805 :file "cli/agents-cli.bclj"} (bold invoked-role) ^{:line 1805 :file "cli/agents-cli.bclj"} (dim "->") ^{:line 1806 :file "cli/agents-cli.bclj"} (str "grade=" selected-grade " tier=" selected-tier " reasoning=" selected-reasoning ^{:line 1807 :file "cli/agents-cli.bclj"} (if ^{:line 1807 :file "cli/agents-cli.bclj"} (and ^{:line 1807 :file "cli/agents-cli.bclj"} (not semantic) ^{:line 1807 :file "cli/agents-cli.bclj"} (not orchestration-preset) model) ^{:line 1807 :file "cli/agents-cli.bclj"} (do
  ^{:line 1807 :file "cli/agents-cli.bclj"} (str " model=" model))) ^{:line 1808 :file "cli/agents-cli.bclj"} (if selected-role ^{:line 1808 :file "cli/agents-cli.bclj"} (do
  ^{:line 1808 :file "cli/agents-cli.bclj"} (str " role=" selected-role))) ^{:line 1809 :file "cli/agents-cli.bclj"} (if selected-composition ^{:line 1809 :file "cli/agents-cli.bclj"} (do
  ^{:line 1810 :file "cli/agents-cli.bclj"} (str " selection=" ^{:line 1810 :file "cli/agents-cli.bclj"} (orchestration-provenance fallback-facts)))) ^{:line 1811 :file "cli/agents-cli.bclj"} (if target ^{:line 1811 :file "cli/agents-cli.bclj"} (do
  ^{:line 1811 :file "cli/agents-cli.bclj"} (str " target=" target))) ^{:line 1812 :file "cli/agents-cli.bclj"} (if selected-posture ^{:line 1812 :file "cli/agents-cli.bclj"} (do
  ^{:line 1812 :file "cli/agents-cli.bclj"} (str " posture=" selected-posture))) ^{:line 1813 :file "cli/agents-cli.bclj"} (if selected-topology ^{:line 1813 :file "cli/agents-cli.bclj"} (do
  ^{:line 1813 :file "cli/agents-cli.bclj"} (str " topology=" selected-topology))) ^{:line 1814 :file "cli/agents-cli.bclj"} (if ^{:line 1814 :file "cli/agents-cli.bclj"} (seq selected-domains) ^{:line 1814 :file "cli/agents-cli.bclj"} (do
  ^{:line 1814 :file "cli/agents-cli.bclj"} (str " domains=" ^{:line 1814 :file "cli/agents-cli.bclj"} (str/join "," selected-domains))))))
  ^{:line 1815 :file "cli/agents-cli.bclj"} (println ^{:line 1815 :file "cli/agents-cli.bclj"} (dim "# struggle observer ->") ^{:line 1816 :file "cli/agents-cli.bclj"} (str "policy=" ^{:line 1816 :file "cli/agents-cli.bclj"} (:version struggle-policy) " topology=" ^{:line 1817 :file "cli/agents-cli.bclj"} (:topology struggle-policy) " error-streak=" ^{:line 1818 :file "cli/agents-cli.bclj"} (:errorStreak struggle-policy) " loop-repeat=" ^{:line 1819 :file "cli/agents-cli.bclj"} (:loopRepeat struggle-policy) " loop-window=" ^{:line 1820 :file "cli/agents-cli.bclj"} (:loopWindow struggle-policy) " no-progress-turns=" ^{:line 1821 :file "cli/agents-cli.bclj"} (:noProgressTurns struggle-policy)))
  ^{:line 1822 :file "cli/agents-cli.bclj"} (if bespoke? ^{:line 1822 :file "cli/agents-cli.bclj"} (do
  ^{:line 1823 :file "cli/agents-cli.bclj"} (println ^{:line 1823 :file "cli/agents-cli.bclj"} (dim "# bespoke evidence ->") ^{:line 1824 :file "cli/agents-cli.bclj"} (str "version=" bespoke-fingerprint-version " domain=" bespoke-fingerprint-domain " sha256=" contract-sha256 " capabilities=" ^{:line 1827 :file "cli/agents-cli.bclj"} (str/join "," ^{:line 1827 :file "cli/agents-cli.bclj"} (:capabilities canonical-contract)) " reason=recorded"))))
  ^{:line 1829 :file "cli/agents-cli.bclj"} (echo-cmd envs POLICY-BUN "run" spawn-ts ^{:line 1829 :file "cli/agents-cli.bclj"} (str "\"" effective-prompt "\""))
  ^{:line 1830 :file "cli/agents-cli.bclj"} (if dry? ^{:line 1831 :file "cli/agents-cli.bclj"} (do
  ^{:line 1832 :file "cli/agents-cli.bclj"} (println ^{:line 1832 :file "cli/agents-cli.bclj"} (ylw "[dry-run]") "not executed. semantic handle would be" ^{:line 1833 :file "cli/agents-cli.bclj"} (bold ^{:line 1833 :file "cli/agents-cli.bclj"} (semantic-handle aid fallback-facts)))
  ^{:line 1834 :file "cli/agents-cli.bclj"} (println "control:" ^{:line 1834 :file "cli/agents-cli.bclj"} (dim aid))
  ^{:line 1835 :file "cli/agents-cli.bclj"} (if ^{:line 1835 :file "cli/agents-cli.bclj"} (and selected-tier ^{:line 1835 :file "cli/agents-cli.bclj"} (nil? dry-route)) ^{:line 1835 :file "cli/agents-cli.bclj"} (do
  ^{:line 1836 :file "cli/agents-cli.bclj"} (println "selected semantic tier:" ^{:line 1836 :file "cli/agents-cli.bclj"} (bold selected-tier) ^{:line 1836 :file "cli/agents-cli.bclj"} (dim "(provider:auto resolves at spawn)"))))) ^{:line 1837 :file "cli/agents-cli.bclj"} (let [log ^{:line 1837 :file "cli/agents-cli.bclj"} (io/file AGENT-LOGDIR ^{:line 1837 :file "cli/agents-cli.bclj"} (str aid ".log"))]
  ^{:line 1838 :file "cli/agents-cli.bclj"} (.mkdirs ^{:line 1838 :file "cli/agents-cli.bclj"} (.getParentFile log))
  ^{:line 1839 :file "cli/agents-cli.bclj"} (let [process ^{:line 1839 :file "cli/agents-cli.bclj"} (north.spawn-process/launch-detached! ^{:line 1840 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" spawn-ts effective-prompt] child-env log)
   startup ^{:line 1841 :file "cli/agents-cli.bclj"} (north.spawn-process/await-startup process aid log agent-facts-one agent-online? :timeout-ms ^{:line 1844 :file "cli/agents-cli.bclj"} (north.spawn-process/startup-timeout-for-capabilities normalized-selected-capabilities))]
  ^{:line 1846 :file "cli/agents-cli.bclj"} (case ^{:line 1846 :file "cli/agents-cli.bclj"} (:status startup)
    :ready ^{:line 1848 :file "cli/agents-cli.bclj"} (do
  ^{:line 1849 :file "cli/agents-cli.bclj"} (println ^{:line 1849 :file "cli/agents-cli.bclj"} (grn "spawned") ^{:line 1849 :file "cli/agents-cli.bclj"} (bold ^{:line 1849 :file "cli/agents-cli.bclj"} (:handle startup)))
  ^{:line 1850 :file "cli/agents-cli.bclj"} (println "control:" ^{:line 1850 :file "cli/agents-cli.bclj"} (dim aid))
  ^{:line 1851 :file "cli/agents-cli.bclj"} (println "watch:" ^{:line 1851 :file "cli/agents-cli.bclj"} (cyn ^{:line 1851 :file "cli/agents-cli.bclj"} (str "north agent watch " aid))))
    :completed ^{:line 1854 :file "cli/agents-cli.bclj"} (do
  ^{:line 1855 :file "cli/agents-cli.bclj"} (println ^{:line 1855 :file "cli/agents-cli.bclj"} (grn "completed") ^{:line 1855 :file "cli/agents-cli.bclj"} (bold ^{:line 1855 :file "cli/agents-cli.bclj"} (:handle startup)) ^{:line 1856 :file "cli/agents-cli.bclj"} (dim ^{:line 1856 :file "cli/agents-cli.bclj"} (str "outcome=" ^{:line 1856 :file "cli/agents-cli.bclj"} (:outcome startup))))
  ^{:line 1857 :file "cli/agents-cli.bclj"} (println "control:" ^{:line 1857 :file "cli/agents-cli.bclj"} (dim aid))
  ^{:line 1858 :file "cli/agents-cli.bclj"} (println "log:" ^{:line 1858 :file "cli/agents-cli.bclj"} (dim ^{:line 1858 :file "cli/agents-cli.bclj"} (str log))))
    ^{:line 1860 :file "cli/agents-cli.bclj"} (do
  ^{:line 1861 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 1862 :file "cli/agents-cli.bclj"} (println ^{:line 1862 :file "cli/agents-cli.bclj"} (red ^{:line 1862 :file "cli/agents-cli.bclj"} (north.spawn-process/failure-message startup))))
  ^{:line 1863 :file "cli/agents-cli.bclj"} (System/exit 1))))))))))

^{:line 1865 :file "cli/agents-cli.bclj"} (def cmd-spawn cmd-spawn!)

^{:line 1867 :file "cli/agents-cli.bclj"} (defn- cmd-spawn-selected!
  "Admit one already-selected Agent Machinery routing request without encoding\n   any of its eight fields or its validated assessment as CLI flags. Only\n   North-owned runtime controls remain in CONTROLS." [routing-request routing-assessment task controls]
  ^{:line 1875 :file "cli/agents-cli.bclj"} (binding [*selected-routing-request* routing-request
   *selected-routing-assessment* routing-assessment]
  ^{:line 1877 :file "cli/agents-cli.bclj"} (cmd-spawn ^{:line 1877 :file "cli/agents-cli.bclj"} (into ^{:line 1877 :file "cli/agents-cli.bclj"} [task] controls))))

^{:line 1882 :file "cli/agents-cli.bclj"} (def delegate-usage ^{:line 1883 :file "cli/agents-cli.bclj"} (str "north agent delegate \"<intent>\" [--role <worker-role> | --composite] " "[--referent <id>] [--context <file>] [spawn options]\n" "       north agent delegate --handoff <session-hard-cap.json> " "[--role <worker-role> | --composite] [spawn options]"))

^{:line 1888 :file "cli/agents-cli.bclj"} (defn- delegate-die [message]
  ^{:line 1889 :file "cli/agents-cli.bclj"} (println ^{:line 1889 :file "cli/agents-cli.bclj"} (red message))
  ^{:line 1890 :file "cli/agents-cli.bclj"} (println ^{:line 1890 :file "cli/agents-cli.bclj"} (red "usage:") delegate-usage)
  ^{:line 1891 :file "cli/agents-cli.bclj"} (System/exit 1))

^{:line 1893 :file "cli/agents-cli.bclj"} (def delegation-run-design-result-version "north:delegation-run-design-result:v1")

^{:line 1895 :file "cli/agents-cli.bclj"} (def delegation-run-design-result-fields ^{:line 1896 :file "cli/agents-cli.bclj"} #{:version :routingRequest :routingAssessment})

^{:line 1897 :file "cli/agents-cli.bclj"} (def delegation-run-design-timeout-ms ^{:line 1897 :file "cli/agents-cli.bclj"} (* 10 60 1000))

^{:line 1899 :file "cli/agents-cli.bclj"} (defn- bounded-delegation-diagnostic [value]
  ^{:line 1900 :file "cli/agents-cli.bclj"} (let [text ^{:line 1900 :file "cli/agents-cli.bclj"} (str/trim ^{:line 1900 :file "cli/agents-cli.bclj"} (str ^{:line 1900 :file "cli/agents-cli.bclj"} (or value "")))]
  ^{:line 1901 :file "cli/agents-cli.bclj"} (if ^{:line 1901 :file "cli/agents-cli.bclj"} (seq text) ^{:line 1901 :file "cli/agents-cli.bclj"} (do
  ^{:line 1902 :file "cli/agents-cli.bclj"} (subs text 0 ^{:line 1902 :file "cli/agents-cli.bclj"} (min 2000 ^{:line 1902 :file "cli/agents-cli.bclj"} (count text)))))))

^{:line 1904 :file "cli/agents-cli.bclj"} (defn- select-delegation-run-design!
  "Ask Agent Machinery to select and validate one portable route. North owns\n   only the single deliberation transport and carries the validated assessment\n   beside the exact eight-field request for concrete admission." [intent context]
  ^{:line 1910 :file "cli/agents-cli.bclj"} (let [payload ^{:line 1910 :file "cli/agents-cli.bclj"} (cond-> ^{:line 1910 :file "cli/agents-cli.bclj"} {:intent intent} ^{:line 1911 :file "cli/agents-cli.bclj"} (some? context) ^{:line 1911 :file "cli/agents-cli.bclj"} (assoc :context context))
   result ^{:line 1912 :file "cli/agents-cli.bclj"} (run ^{:line 1912 :file "cli/agents-cli.bclj"} [POLICY-BUN "run" DELEGATION-RUN-DESIGN-TRANSPORT] :timeout delegation-run-design-timeout-ms :in ^{:line 1914 :file "cli/agents-cli.bclj"} (json/generate-string payload))
   output ^{:line 1915 :file "cli/agents-cli.bclj"} (str/trim ^{:line 1915 :file "cli/agents-cli.bclj"} (str ^{:line 1915 :file "cli/agents-cli.bclj"} (or ^{:line 1915 :file "cli/agents-cli.bclj"} (:out result) "")))]
  ^{:line 1916 :file "cli/agents-cli.bclj"} (if ^{:line 1916 :file "cli/agents-cli.bclj"} (not ^{:line 1916 :file "cli/agents-cli.bclj"} (:ok result)) ^{:line 1916 :file "cli/agents-cli.bclj"} (do
  ^{:line 1917 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 1918 :file "cli/agents-cli.bclj"} (or ^{:line 1918 :file "cli/agents-cli.bclj"} (bounded-delegation-diagnostic ^{:line 1918 :file "cli/agents-cli.bclj"} (:err result)) ^{:line 1919 :file "cli/agents-cli.bclj"} (if ^{:line 1919 :file "cli/agents-cli.bclj"} (:timeout result) ^{:line 1919 :file "cli/agents-cli.bclj"} (do
  ^{:line 1920 :file "cli/agents-cli.bclj"} (str "delegation run-design selection exceeded " delegation-run-design-timeout-ms "ms"))) ^{:line 1922 :file "cli/agents-cli.bclj"} (if ^{:line 1922 :file "cli/agents-cli.bclj"} (:error result) ^{:line 1922 :file "cli/agents-cli.bclj"} (do
  ^{:line 1923 :file "cli/agents-cli.bclj"} (str "delegation run-design selection could not start: " ^{:line 1923 :file "cli/agents-cli.bclj"} (:error result)))) ^{:line 1924 :file "cli/agents-cli.bclj"} (str "delegation run-design selection exited " ^{:line 1924 :file "cli/agents-cli.bclj"} (:exit result))))))
  ^{:line 1925 :file "cli/agents-cli.bclj"} (let [selected ^{:line 1925 :file "cli/agents-cli.bclj"} (try
  ^{:line 1926 :file "cli/agents-cli.bclj"} (json/parse-string output true)
  (catch Exception _
    nil))
   routing-request ^{:line 1928 :file "cli/agents-cli.bclj"} (:routingRequest selected)
   routing-assessment ^{:line 1929 :file "cli/agents-cli.bclj"} (:routingAssessment selected)]
  ^{:line 1930 :file "cli/agents-cli.bclj"} (if ^{:line 1930 :file "cli/agents-cli.bclj"} (not ^{:line 1930 :file "cli/agents-cli.bclj"} (and ^{:line 1930 :file "cli/agents-cli.bclj"} (map? selected) ^{:line 1931 :file "cli/agents-cli.bclj"} (= delegation-run-design-result-fields ^{:line 1931 :file "cli/agents-cli.bclj"} (set ^{:line 1931 :file "cli/agents-cli.bclj"} (keys selected))) ^{:line 1932 :file "cli/agents-cli.bclj"} (= delegation-run-design-result-version ^{:line 1932 :file "cli/agents-cli.bclj"} (:version selected)) ^{:line 1933 :file "cli/agents-cli.bclj"} (map? routing-request) ^{:line 1934 :file "cli/agents-cli.bclj"} (= routing-request-fields ^{:line 1934 :file "cli/agents-cli.bclj"} (set ^{:line 1934 :file "cli/agents-cli.bclj"} (keys routing-request))) ^{:line 1935 :file "cli/agents-cli.bclj"} (map? routing-assessment) ^{:line 1936 :file "cli/agents-cli.bclj"} (= "minimum-sufficient-v1" ^{:line 1936 :file "cli/agents-cli.bclj"} (:version routing-assessment)))) ^{:line 1930 :file "cli/agents-cli.bclj"} (do
  ^{:line 1937 :file "cli/agents-cli.bclj"} (delegate-die "delegation run-design transport returned a malformed result envelope")))
  ^{:line 1939 :file "cli/agents-cli.bclj"} {:routing-request routing-request :routing-assessment routing-assessment})))

^{:line 1942 :file "cli/agents-cli.bclj"} (def delegate-referent-id-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

^{:line 1943 :file "cli/agents-cli.bclj"} (def delegate-referent-title-max-utf8-bytes 160)

^{:line 1944 :file "cli/agents-cli.bclj"} (def delegate-handoff-max-bytes ^{:line 1944 :file "cli/agents-cli.bclj"} (* 64 1024))

^{:line 1945 :file "cli/agents-cli.bclj"} (def delegate-handoff-hard-cap-ms ^{:line 1945 :file "cli/agents-cli.bclj"} (* 60 60 1000))

^{:line 1946 :file "cli/agents-cli.bclj"} (def delegate-handoff-next-action "Resume only this deliverable; inspect the named referent, worktree, branch, and session transcript before editing.")

^{:line 1948 :file "cli/agents-cli.bclj"} (def delegate-handoff-required-keys ^{:line 1949 :file "cli/agents-cli.bclj"} #{:version :reason :writtenAt :hardCapMs :agentId :referentId :goal :repo :nextAction :completionClaimed})

^{:line 1951 :file "cli/agents-cli.bclj"} (def delegate-handoff-optional-keys ^{:line 1951 :file "cli/agents-cli.bclj"} #{:worktree :branch})

^{:line 1952 :file "cli/agents-cli.bclj"} (def delegate-handoff-private-permissions ^{:line 1953 :file "cli/agents-cli.bclj"} #{java.nio.file.attribute.PosixFilePermission/OWNER_READ java.nio.file.attribute.PosixFilePermission/OWNER_WRITE})

^{:line 1955 :file "cli/agents-cli.bclj"} (def delegate-routing-override-flags ^{:line 1956 :file "cli/agents-cli.bclj"} #{"--taskGrade" "--task-grade" "--domain" "--topology" "--tier" "--reasoning" "--deliberation" "--posture" "--composition" "--rationale" "--nearest" "--contract" "--override-reason" "--assessment" "--routing-assessment" "--promotion-candidate" "--nominate" "--no-promotion-candidate"})

^{:line 1961 :file "cli/agents-cli.bclj"} (def capture-receipt-keys ^{:line 1962 :file "cli/agents-cli.bclj"} #{:id :referent :title :path :expected :committed :complete :reason})

^{:line 1964 :file "cli/agents-cli.bclj"} (defn- canonical-delegate-referent [raw]
  ^{:line 1965 :file "cli/agents-cli.bclj"} (let [value ^{:line 1965 :file "cli/agents-cli.bclj"} (some-> raw str)
   bare ^{:line 1966 :file "cli/agents-cli.bclj"} (if value ^{:line 1966 :file "cli/agents-cli.bclj"} (do
  ^{:line 1966 :file "cli/agents-cli.bclj"} (str/replace-first value #"^@" "")))]
  ^{:line 1967 :file "cli/agents-cli.bclj"} (if ^{:line 1967 :file "cli/agents-cli.bclj"} (and value ^{:line 1968 :file "cli/agents-cli.bclj"} (= value ^{:line 1968 :file "cli/agents-cli.bclj"} (str/trim value)) ^{:line 1969 :file "cli/agents-cli.bclj"} (not ^{:line 1969 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 1969 :file "cli/agents-cli.bclj"} (str bare) "@")) ^{:line 1970 :file "cli/agents-cli.bclj"} (<= ^{:line 1970 :file "cli/agents-cli.bclj"} (count ^{:line 1970 :file "cli/agents-cli.bclj"} (str bare)) 512) ^{:line 1971 :file "cli/agents-cli.bclj"} (re-matches delegate-referent-id-pattern ^{:line 1971 :file "cli/agents-cli.bclj"} (str bare))) ^{:line 1967 :file "cli/agents-cli.bclj"} (do
  bare))))

^{:line 1974 :file "cli/agents-cli.bclj"} (defn- normalize-delegate-referent [raw]
  ^{:line 1975 :file "cli/agents-cli.bclj"} (or ^{:line 1975 :file "cli/agents-cli.bclj"} (canonical-delegate-referent raw) ^{:line 1976 :file "cli/agents-cli.bclj"} (delegate-die "--referent must be a bare or single-@ ASCII North referent id")))

^{:line 1978 :file "cli/agents-cli.bclj"} (defn- delegate-handoff-text? [value]
  ^{:line 1979 :file "cli/agents-cli.bclj"} (and ^{:line 1979 :file "cli/agents-cli.bclj"} (string? value) ^{:line 1980 :file "cli/agents-cli.bclj"} (not ^{:line 1980 :file "cli/agents-cli.bclj"} (str/blank? value)) ^{:line 1981 :file "cli/agents-cli.bclj"} (not ^{:line 1981 :file "cli/agents-cli.bclj"} (str/includes? value "\u0000"))))

^{:line 1983 :file "cli/agents-cli.bclj"} (defn- delegate-handoff-absolute-path? [value]
  ^{:line 1984 :file "cli/agents-cli.bclj"} (and ^{:line 1984 :file "cli/agents-cli.bclj"} (delegate-handoff-text? value) ^{:line 1985 :file "cli/agents-cli.bclj"} (= value ^{:line 1985 :file "cli/agents-cli.bclj"} (str/trim value)) ^{:line 1986 :file "cli/agents-cli.bclj"} (try
  ^{:line 1987 :file "cli/agents-cli.bclj"} (.isAbsolute ^{:line 1988 :file "cli/agents-cli.bclj"} (java.nio.file.Paths/get value ^{:line 1988 :file "cli/agents-cli.bclj"} (make-array String 0)))
  (catch Exception _
    false))))

^{:line 1991 :file "cli/agents-cli.bclj"} (defn- delegate-handoff-written-at? [value]
  ^{:line 1992 :file "cli/agents-cli.bclj"} (and ^{:line 1992 :file "cli/agents-cli.bclj"} (string? value) ^{:line 1993 :file "cli/agents-cli.bclj"} (boolean ^{:line 1994 :file "cli/agents-cli.bclj"} (re-matches #"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$" value)) ^{:line 1995 :file "cli/agents-cli.bclj"} (try
  ^{:line 1996 :file "cli/agents-cli.bclj"} (java.time.Instant/parse value)
  true
  (catch java.time.format.DateTimeParseException _
    false))))

^{:line 2000 :file "cli/agents-cli.bclj"} (defn- decode-delegate-handoff-utf8! [bytes path]
  ^{:line 2003 :file "cli/agents-cli.bclj"} (try
  ^{:line 2004 :file "cli/agents-cli.bclj"} (let [decoder ^{:line 2004 :file "cli/agents-cli.bclj"} (doto ^{:line 2004 :file "cli/agents-cli.bclj"} (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
  ^{:line 2005 :file "cli/agents-cli.bclj"} (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
  ^{:line 2006 :file "cli/agents-cli.bclj"} (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
  ^{:line 2007 :file "cli/agents-cli.bclj"} (str ^{:line 2007 :file "cli/agents-cli.bclj"} (.decode decoder ^{:line 2007 :file "cli/agents-cli.bclj"} (java.nio.ByteBuffer/wrap bytes))))
  (catch java.nio.charset.CharacterCodingException _
    ^{:line 2009 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2009 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact is not valid UTF-8: " path)))))

^{:line 2011 :file "cli/agents-cli.bclj"} (defn- read-delegate-handoff! [raw-path]
  ^{:line 2012 :file "cli/agents-cli.bclj"} (let [file ^{:line 2012 :file "cli/agents-cli.bclj"} (io/file raw-path)
   path ^{:line 2013 :file "cli/agents-cli.bclj"} (.toPath file)
   display-path ^{:line 2014 :file "cli/agents-cli.bclj"} (.getPath file)
   no-follow ^{:line 2015 :file "cli/agents-cli.bclj"} (into-array java.nio.file.LinkOption ^{:line 2016 :file "cli/agents-cli.bclj"} [java.nio.file.LinkOption/NOFOLLOW_LINKS])]
  ^{:line 2017 :file "cli/agents-cli.bclj"} (if ^{:line 2017 :file "cli/agents-cli.bclj"} (not ^{:line 2017 :file "cli/agents-cli.bclj"} (java.nio.file.Files/exists path no-follow)) ^{:line 2017 :file "cli/agents-cli.bclj"} (do
  ^{:line 2018 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2018 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact not found: " display-path))))
  ^{:line 2019 :file "cli/agents-cli.bclj"} (let [attributes ^{:line 2019 :file "cli/agents-cli.bclj"} (try
  ^{:line 2020 :file "cli/agents-cli.bclj"} (java.nio.file.Files/readAttributes path java.nio.file.attribute.BasicFileAttributes no-follow)
  (catch java.io.IOException _
    ^{:line 2023 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2023 :file "cli/agents-cli.bclj"} (str "cannot inspect session hard-cap artifact: " display-path))))]
  ^{:line 2025 :file "cli/agents-cli.bclj"} (if ^{:line 2025 :file "cli/agents-cli.bclj"} (or ^{:line 2025 :file "cli/agents-cli.bclj"} (java.nio.file.Files/isSymbolicLink path) ^{:line 2026 :file "cli/agents-cli.bclj"} (not ^{:line 2026 :file "cli/agents-cli.bclj"} (.isRegularFile attributes))) ^{:line 2025 :file "cli/agents-cli.bclj"} (do
  ^{:line 2027 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2027 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact must be a regular non-symlink file: " display-path))))
  ^{:line 2029 :file "cli/agents-cli.bclj"} (if ^{:line 2029 :file "cli/agents-cli.bclj"} (not ^{:line 2029 :file "cli/agents-cli.bclj"} (<= 1 ^{:line 2029 :file "cli/agents-cli.bclj"} (.size attributes) delegate-handoff-max-bytes)) ^{:line 2029 :file "cli/agents-cli.bclj"} (do
  ^{:line 2030 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2030 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact must be between 1 and " delegate-handoff-max-bytes " bytes")))))
  ^{:line 2032 :file "cli/agents-cli.bclj"} (let [permissions ^{:line 2032 :file "cli/agents-cli.bclj"} (try
  ^{:line 2033 :file "cli/agents-cli.bclj"} (set ^{:line 2033 :file "cli/agents-cli.bclj"} (java.nio.file.Files/getPosixFilePermissions path no-follow))
  (catch UnsupportedOperationException _
    ^{:line 2035 :file "cli/agents-cli.bclj"} (delegate-die "session hard-cap adoption requires a POSIX private-file boundary")))]
  ^{:line 2037 :file "cli/agents-cli.bclj"} (if ^{:line 2037 :file "cli/agents-cli.bclj"} (not ^{:line 2037 :file "cli/agents-cli.bclj"} (= delegate-handoff-private-permissions permissions)) ^{:line 2037 :file "cli/agents-cli.bclj"} (do
  ^{:line 2038 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2038 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact must have mode 0600: " display-path)))))
  ^{:line 2040 :file "cli/agents-cli.bclj"} (let [bytes ^{:line 2040 :file "cli/agents-cli.bclj"} (try
  ^{:line 2041 :file "cli/agents-cli.bclj"} (java.nio.file.Files/readAllBytes path)
  (catch java.io.IOException _
    ^{:line 2043 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2043 :file "cli/agents-cli.bclj"} (str "cannot read session hard-cap artifact: " display-path))))]
  ^{:line 2047 :file "cli/agents-cli.bclj"} (if ^{:line 2047 :file "cli/agents-cli.bclj"} (not ^{:line 2047 :file "cli/agents-cli.bclj"} (<= 1 ^{:line 2047 :file "cli/agents-cli.bclj"} (alength bytes) delegate-handoff-max-bytes)) ^{:line 2047 :file "cli/agents-cli.bclj"} (do
  ^{:line 2048 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2048 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact must be between 1 and " delegate-handoff-max-bytes " bytes"))))
  ^{:line 2050 :file "cli/agents-cli.bclj"} (let [raw ^{:line 2050 :file "cli/agents-cli.bclj"} (decode-delegate-handoff-utf8! bytes display-path)
   document ^{:line 2051 :file "cli/agents-cli.bclj"} (try
  ^{:line 2052 :file "cli/agents-cli.bclj"} (json/parse-string raw true)
  (catch Exception _
    ^{:line 2054 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2054 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact is not valid JSON: " display-path))))
   keys-present ^{:line 2056 :file "cli/agents-cli.bclj"} (if ^{:line 2056 :file "cli/agents-cli.bclj"} (map? document) ^{:line 2056 :file "cli/agents-cli.bclj"} (do
  ^{:line 2056 :file "cli/agents-cli.bclj"} (set ^{:line 2056 :file "cli/agents-cli.bclj"} (keys document))))
   allowed-keys ^{:line 2057 :file "cli/agents-cli.bclj"} (set/union delegate-handoff-required-keys delegate-handoff-optional-keys)
   canonical-referent ^{:line 2059 :file "cli/agents-cli.bclj"} (if ^{:line 2059 :file "cli/agents-cli.bclj"} (map? document) ^{:line 2059 :file "cli/agents-cli.bclj"} (do
  ^{:line 2060 :file "cli/agents-cli.bclj"} (canonical-delegate-referent ^{:line 2060 :file "cli/agents-cli.bclj"} (:referentId document))))]
  ^{:line 2061 :file "cli/agents-cli.bclj"} (if ^{:line 2061 :file "cli/agents-cli.bclj"} (not ^{:line 2061 :file "cli/agents-cli.bclj"} (and ^{:line 2061 :file "cli/agents-cli.bclj"} (map? document) ^{:line 2062 :file "cli/agents-cli.bclj"} (set/subset? delegate-handoff-required-keys keys-present) ^{:line 2063 :file "cli/agents-cli.bclj"} (set/subset? keys-present allowed-keys) ^{:line 2064 :file "cli/agents-cli.bclj"} (= 1 ^{:line 2064 :file "cli/agents-cli.bclj"} (:version document)) ^{:line 2065 :file "cli/agents-cli.bclj"} (= "session_hard_cap" ^{:line 2065 :file "cli/agents-cli.bclj"} (:reason document)) ^{:line 2066 :file "cli/agents-cli.bclj"} (delegate-handoff-written-at? ^{:line 2066 :file "cli/agents-cli.bclj"} (:writtenAt document)) ^{:line 2067 :file "cli/agents-cli.bclj"} (= delegate-handoff-hard-cap-ms ^{:line 2067 :file "cli/agents-cli.bclj"} (:hardCapMs document)) ^{:line 2068 :file "cli/agents-cli.bclj"} (valid-control-id? ^{:line 2068 :file "cli/agents-cli.bclj"} (:agentId document)) ^{:line 2069 :file "cli/agents-cli.bclj"} (= ^{:line 2069 :file "cli/agents-cli.bclj"} (:referentId document) canonical-referent) ^{:line 2070 :file "cli/agents-cli.bclj"} (delegate-handoff-text? ^{:line 2070 :file "cli/agents-cli.bclj"} (:goal document)) ^{:line 2071 :file "cli/agents-cli.bclj"} (delegate-handoff-absolute-path? ^{:line 2071 :file "cli/agents-cli.bclj"} (:repo document)) ^{:line 2072 :file "cli/agents-cli.bclj"} (or ^{:line 2072 :file "cli/agents-cli.bclj"} (not ^{:line 2072 :file "cli/agents-cli.bclj"} (contains? document :worktree)) ^{:line 2073 :file "cli/agents-cli.bclj"} (delegate-handoff-absolute-path? ^{:line 2073 :file "cli/agents-cli.bclj"} (:worktree document))) ^{:line 2074 :file "cli/agents-cli.bclj"} (or ^{:line 2074 :file "cli/agents-cli.bclj"} (not ^{:line 2074 :file "cli/agents-cli.bclj"} (contains? document :branch)) ^{:line 2075 :file "cli/agents-cli.bclj"} (delegate-handoff-text? ^{:line 2075 :file "cli/agents-cli.bclj"} (:branch document))) ^{:line 2076 :file "cli/agents-cli.bclj"} (= delegate-handoff-next-action ^{:line 2076 :file "cli/agents-cli.bclj"} (:nextAction document)) ^{:line 2077 :file "cli/agents-cli.bclj"} (false? ^{:line 2077 :file "cli/agents-cli.bclj"} (:completionClaimed document)))) ^{:line 2061 :file "cli/agents-cli.bclj"} (do
  ^{:line 2078 :file "cli/agents-cli.bclj"} (delegate-die "session hard-cap artifact does not match North's incomplete v1 handoff contract")))
  ^{:line 2080 :file "cli/agents-cli.bclj"} {:task ^{:line 2080 :file "cli/agents-cli.bclj"} (:goal document) :referent canonical-referent :context ^{:line 2082 :file "cli/agents-cli.bclj"} (str/trim raw)}))))

^{:line 2084 :file "cli/agents-cli.bclj"} (defn- structured-facts? [facts]
  ^{:line 2085 :file "cli/agents-cli.bclj"} (and ^{:line 2085 :file "cli/agents-cli.bclj"} (sequential? facts) ^{:line 2086 :file "cli/agents-cli.bclj"} (every? ^{:line 2086 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2086 :file "cli/agents-cli.bclj"} (and ^{:line 2086 :file "cli/agents-cli.bclj"} (map? %1) ^{:line 2086 :file "cli/agents-cli.bclj"} (= ^{:line 2086 :file "cli/agents-cli.bclj"} #{:predicate :value} ^{:line 2086 :file "cli/agents-cli.bclj"} (set ^{:line 2086 :file "cli/agents-cli.bclj"} (keys %1))) ^{:line 2086 :file "cli/agents-cli.bclj"} (string? ^{:line 2086 :file "cli/agents-cli.bclj"} (:predicate %1)) ^{:line 2086 :file "cli/agents-cli.bclj"} (string? ^{:line 2086 :file "cli/agents-cli.bclj"} (:value %1)))) facts)))

^{:line 2096 :file "cli/agents-cli.bclj"} (defn- structured-subject-facts [subject]
  ^{:line 2097 :file "cli/agents-cli.bclj"} (mapv ^{:line 2097 :file "cli/agents-cli.bclj"} (fn [[predicate value]] ^{:line 2097 :file "cli/agents-cli.bclj"} {:predicate predicate :value value}) ^{:line 2098 :file "cli/agents-cli.bclj"} (north.coord/show-rows! ^{:line 2098 :file "cli/agents-cli.bclj"} (Integer/parseInt PORT) subject)))

^{:line 2100 :file "cli/agents-cli.bclj"} (defn- parse-referent-facts! [id facts]
  ^{:line 2103 :file "cli/agents-cli.bclj"} (if ^{:line 2103 :file "cli/agents-cli.bclj"} (not ^{:line 2103 :file "cli/agents-cli.bclj"} (structured-facts? facts)) ^{:line 2103 :file "cli/agents-cli.bclj"} (do
  ^{:line 2104 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2104 :file "cli/agents-cli.bclj"} (str "referent @" id " returned an invalid structured fact projection"))))
  ^{:line 2105 :file "cli/agents-cli.bclj"} (let [kinds ^{:line 2105 :file "cli/agents-cli.bclj"} (mapv ^{:line 2105 :file "cli/agents-cli.bclj"} (fn [fact] ^{:line 2105 :file "cli/agents-cli.bclj"} (:value fact)) ^{:line 2106 :file "cli/agents-cli.bclj"} (filter ^{:line 2106 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2106 :file "cli/agents-cli.bclj"} (= "entity_kind" ^{:line 2106 :file "cli/agents-cli.bclj"} (:predicate %1))) facts))
   titles ^{:line 2107 :file "cli/agents-cli.bclj"} (mapv ^{:line 2107 :file "cli/agents-cli.bclj"} (fn [fact] ^{:line 2107 :file "cli/agents-cli.bclj"} (:value fact)) ^{:line 2108 :file "cli/agents-cli.bclj"} (filter ^{:line 2108 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2108 :file "cli/agents-cli.bclj"} (= "title" ^{:line 2108 :file "cli/agents-cli.bclj"} (:predicate %1))) facts))]
  ^{:line 2109 :file "cli/agents-cli.bclj"} (if ^{:line 2109 :file "cli/agents-cli.bclj"} (not ^{:line 2109 :file "cli/agents-cli.bclj"} (and ^{:line 2109 :file "cli/agents-cli.bclj"} (= ^{:line 2109 :file "cli/agents-cli.bclj"} ["referent"] kinds) ^{:line 2110 :file "cli/agents-cli.bclj"} (= 1 ^{:line 2110 :file "cli/agents-cli.bclj"} (count titles)) ^{:line 2111 :file "cli/agents-cli.bclj"} (not ^{:line 2111 :file "cli/agents-cli.bclj"} (str/blank? ^{:line 2111 :file "cli/agents-cli.bclj"} (first titles))))) ^{:line 2109 :file "cli/agents-cli.bclj"} (do
  ^{:line 2112 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2112 :file "cli/agents-cli.bclj"} (str "referent @" id " is not an exact title-bearing North Referent"))))
  ^{:line 2113 :file "cli/agents-cli.bclj"} {:id id :title ^{:line 2114 :file "cli/agents-cli.bclj"} (first titles) :facts facts :committed? ^{:line 2116 :file "cli/agents-cli.bclj"} (boolean ^{:line 2116 :file "cli/agents-cli.bclj"} (some ^{:line 2116 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2116 :file "cli/agents-cli.bclj"} (= "committed" ^{:line 2116 :file "cli/agents-cli.bclj"} (:predicate %1))) facts)) :done-when ^{:line 2117 :file "cli/agents-cli.bclj"} (mapv ^{:line 2117 :file "cli/agents-cli.bclj"} (fn [fact] ^{:line 2117 :file "cli/agents-cli.bclj"} (:value fact)) ^{:line 2118 :file "cli/agents-cli.bclj"} (filter ^{:line 2118 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2118 :file "cli/agents-cli.bclj"} (= "done_when" ^{:line 2118 :file "cli/agents-cli.bclj"} (:predicate %1))) facts))}))

^{:line 2120 :file "cli/agents-cli.bclj"} (defn- read-delegate-referent! [raw]
  ^{:line 2121 :file "cli/agents-cli.bclj"} (let [id ^{:line 2121 :file "cli/agents-cli.bclj"} (normalize-delegate-referent raw)
   facts ^{:line 2122 :file "cli/agents-cli.bclj"} (try
  ^{:line 2123 :file "cli/agents-cli.bclj"} (structured-subject-facts ^{:line 2123 :file "cli/agents-cli.bclj"} (str "@" id))
  (catch Exception e
    ^{:line 2125 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2126 :file "cli/agents-cli.bclj"} (str "cannot prove delegate referent @" id " through North's structured read boundary: " ^{:line 2128 :file "cli/agents-cli.bclj"} (or ^{:line 2128 :file "cli/agents-cli.bclj"} (not-empty ^{:line 2128 :file "cli/agents-cli.bclj"} (str ^{:line 2128 :file "cli/agents-cli.bclj"} (.getMessage e))) ^{:line 2129 :file "cli/agents-cli.bclj"} (.getName ^{:line 2129 :file "cli/agents-cli.bclj"} (class e)))))))]
  ^{:line 2130 :file "cli/agents-cli.bclj"} (parse-referent-facts! id facts)))

^{:line 2132 :file "cli/agents-cli.bclj"} (defn- fact-set [facts]
  ^{:line 2133 :file "cli/agents-cli.bclj"} (reduce ^{:line 2133 :file "cli/agents-cli.bclj"} (fn [acc {:keys [predicate value]}] ^{:line 2136 :file "cli/agents-cli.bclj"} (update acc predicate ^{:line 2136 :file "cli/agents-cli.bclj"} (fnil conj ^{:line 2136 :file "cli/agents-cli.bclj"} #{}) value)) ^{:line 2137 :file "cli/agents-cli.bclj"} {} facts))

^{:line 2140 :file "cli/agents-cli.bclj"} (defn- utf8-byte-count [value]
  ^{:line 2141 :file "cli/agents-cli.bclj"} (alength ^{:line 2141 :file "cli/agents-cli.bclj"} (.getBytes ^{:line 2141 :file "cli/agents-cli.bclj"} (str value) java.nio.charset.StandardCharsets/UTF_8)))

^{:line 2143 :file "cli/agents-cli.bclj"} (defn- utf8-prefix [value max-bytes]
  ^{:line 2146 :file "cli/agents-cli.bclj"} (loop [end 0]
  ^{:line 2147 :file "cli/agents-cli.bclj"} (if ^{:line 2147 :file "cli/agents-cli.bclj"} (>= end ^{:line 2147 :file "cli/agents-cli.bclj"} (.length value)) value ^{:line 2149 :file "cli/agents-cli.bclj"} (let [next ^{:line 2149 :file "cli/agents-cli.bclj"} (+ end ^{:line 2149 :file "cli/agents-cli.bclj"} (long ^{:line 2149 :file "cli/agents-cli.bclj"} (Character/charCount ^{:line 2149 :file "cli/agents-cli.bclj"} (.codePointAt value end))))]
  ^{:line 2150 :file "cli/agents-cli.bclj"} (if ^{:line 2150 :file "cli/agents-cli.bclj"} (> ^{:line 2150 :file "cli/agents-cli.bclj"} (utf8-byte-count ^{:line 2150 :file "cli/agents-cli.bclj"} (subs value 0 next)) max-bytes) ^{:line 2151 :file "cli/agents-cli.bclj"} (subs value 0 end) ^{:line 2152 :file "cli/agents-cli.bclj"} (recur next))))))

^{:line 2154 :file "cli/agents-cli.bclj"} (defn delegate-referent-title [task]
  ^{:line 2158 :file "cli/agents-cli.bclj"} (let [lines ^{:line 2158 :file "cli/agents-cli.bclj"} (str/split ^{:line 2158 :file "cli/agents-cli.bclj"} (str task) #"\R" -1)
   line ^{:line 2159 :file "cli/agents-cli.bclj"} (or ^{:line 2159 :file "cli/agents-cli.bclj"} (first ^{:line 2159 :file "cli/agents-cli.bclj"} (remove str/blank? ^{:line 2159 :file "cli/agents-cli.bclj"} (map str/trim lines))) "Delegated task")
   collapsed ^{:line 2160 :file "cli/agents-cli.bclj"} (-> line ^{:line 2161 :file "cli/agents-cli.bclj"} (str/replace #"[\p{javaWhitespace}\p{Z}]+" " ") str/trim)]
  ^{:line 2163 :file "cli/agents-cli.bclj"} (utf8-prefix collapsed delegate-referent-title-max-utf8-bytes)))

^{:line 2165 :file "cli/agents-cli.bclj"} (defn- managed-referent-binding []
  ^{:line 2166 :file "cli/agents-cli.bclj"} (let [ambient-run ^{:line 2166 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_RUN_ID")
   referent ^{:line 2167 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_REFERENT_ID")
   capability ^{:line 2168 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_RUN_CAPABILITY")
   agent ^{:line 2169 :file "cli/agents-cli.bclj"} (System/getenv "AGENT_ID")
   values ^{:line 2170 :file "cli/agents-cli.bclj"} [ambient-run referent capability agent]
   present ^{:line 2171 :file "cli/agents-cli.bclj"} (mapv ^{:line 2171 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2171 :file "cli/agents-cli.bclj"} (boolean ^{:line 2171 :file "cli/agents-cli.bclj"} (known %1))) values)]
  ^{:line 2172 :file "cli/agents-cli.bclj"} (if ^{:line 2172 :file "cli/agents-cli.bclj"} (every? true? present) ^{:line 2174 :file "cli/agents-cli.bclj"} (try
  ^{:line 2175 :file "cli/agents-cli.bclj"} (let [run-id ^{:line 2175 :file "cli/agents-cli.bclj"} (canonical-delegate-referent ambient-run)
   referent-id ^{:line 2176 :file "cli/agents-cli.bclj"} (canonical-delegate-referent referent)
   rows ^{:line 2177 :file "cli/agents-cli.bclj"} (if ^{:line 2177 :file "cli/agents-cli.bclj"} (and run-id referent-id) ^{:line 2177 :file "cli/agents-cli.bclj"} (do
  ^{:line 2178 :file "cli/agents-cli.bclj"} (structured-subject-facts ^{:line 2178 :file "cli/agents-cli.bclj"} (str "@" run-id))))
   facts ^{:line 2179 :file "cli/agents-cli.bclj"} (if ^{:line 2179 :file "cli/agents-cli.bclj"} (structured-facts? rows) ^{:line 2179 :file "cli/agents-cli.bclj"} (do
  ^{:line 2179 :file "cli/agents-cli.bclj"} (fact-set rows)))
   reporter ^{:line 2180 :file "cli/agents-cli.bclj"} (str "@agent:" ^{:line 2180 :file "cli/agents-cli.bclj"} (str/replace-first agent #"^@?agent:" ""))]
  ^{:line 2181 :file "cli/agents-cli.bclj"} (if ^{:line 2181 :file "cli/agents-cli.bclj"} (and run-id referent-id facts ^{:line 2184 :file "cli/agents-cli.bclj"} (north.terminal-projection/run-reservation-valid? facts) ^{:line 2185 :file "cli/agents-cli.bclj"} (= ^{:line 2185 :file "cli/agents-cli.bclj"} #{^{:line 2185 :file "cli/agents-cli.bclj"} (str "@" referent-id)} ^{:line 2186 :file "cli/agents-cli.bclj"} (get facts "run_reservation_referent")) ^{:line 2187 :file "cli/agents-cli.bclj"} (= ^{:line 2187 :file "cli/agents-cli.bclj"} #{reporter} ^{:line 2188 :file "cli/agents-cli.bclj"} (get facts "run_reservation_agent")) ^{:line 2189 :file "cli/agents-cli.bclj"} (= ^{:line 2189 :file "cli/agents-cli.bclj"} #{^{:line 2189 :file "cli/agents-cli.bclj"} (north.terminal-projection/sha256 capability)} ^{:line 2190 :file "cli/agents-cli.bclj"} (get facts "run_capability_sha256"))) ^{:line 2191 :file "cli/agents-cli.bclj"} {:kind :complete :referent referent-id} ^{:line 2192 :file "cli/agents-cli.bclj"} {:kind :none :residue? true}))
  (catch Exception _
    ^{:line 2194 :file "cli/agents-cli.bclj"} {:kind :none :residue? true})) ^{:line 2173 :file "cli/agents-cli.bclj"} {:kind :none :residue? ^{:line 2173 :file "cli/agents-cli.bclj"} (some true? present)})))

^{:line 2199 :file "cli/agents-cli.bclj"} (def delegate-capture-timeout-ms 180000)

^{:line 2201 :file "cli/agents-cli.bclj"} (defn- capture-delegate-referent! [task]
  ^{:line 2202 :file "cli/agents-cli.bclj"} (let [title ^{:line 2202 :file "cli/agents-cli.bclj"} (delegate-referent-title task)
   capture-env ^{:line 2203 :file "cli/agents-cli.bclj"} (assoc ^{:line 2203 :file "cli/agents-cli.bclj"} (into ^{:line 2203 :file "cli/agents-cli.bclj"} {} ^{:line 2203 :file "cli/agents-cli.bclj"} (System/getenv)) "NORTH_CAPTURE_STRUCTURED" "1")
   result ^{:line 2205 :file "cli/agents-cli.bclj"} (run ^{:line 2205 :file "cli/agents-cli.bclj"} [NORTH-CLI "work" "capture" title] :timeout delegate-capture-timeout-ms :env capture-env)]
  ^{:line 2207 :file "cli/agents-cli.bclj"} (if ^{:line 2207 :file "cli/agents-cli.bclj"} (not ^{:line 2207 :file "cli/agents-cli.bclj"} (:ok result)) ^{:line 2207 :file "cli/agents-cli.bclj"} (do
  ^{:line 2208 :file "cli/agents-cli.bclj"} (delegate-die "North could not capture a durable delegate referent")))
  ^{:line 2209 :file "cli/agents-cli.bclj"} (let [receipt ^{:line 2209 :file "cli/agents-cli.bclj"} (try
  ^{:line 2209 :file "cli/agents-cli.bclj"} (json/parse-string ^{:line 2209 :file "cli/agents-cli.bclj"} (str/trim ^{:line 2209 :file "cli/agents-cli.bclj"} (:out result)) true)
  (catch Exception _
    ^{:line 2211 :file "cli/agents-cli.bclj"} (delegate-die "North capture did not return its exact structured receipt")))]
  ^{:line 2212 :file "cli/agents-cli.bclj"} (if ^{:line 2212 :file "cli/agents-cli.bclj"} (not ^{:line 2212 :file "cli/agents-cli.bclj"} (and ^{:line 2212 :file "cli/agents-cli.bclj"} (map? receipt) ^{:line 2213 :file "cli/agents-cli.bclj"} (= capture-receipt-keys ^{:line 2213 :file "cli/agents-cli.bclj"} (set ^{:line 2213 :file "cli/agents-cli.bclj"} (keys receipt))) ^{:line 2214 :file "cli/agents-cli.bclj"} (string? ^{:line 2214 :file "cli/agents-cli.bclj"} (:id receipt)) ^{:line 2215 :file "cli/agents-cli.bclj"} (string? ^{:line 2215 :file "cli/agents-cli.bclj"} (:referent receipt)) ^{:line 2216 :file "cli/agents-cli.bclj"} (string? ^{:line 2216 :file "cli/agents-cli.bclj"} (:title receipt)) ^{:line 2217 :file "cli/agents-cli.bclj"} (string? ^{:line 2217 :file "cli/agents-cli.bclj"} (:path receipt)) ^{:line 2218 :file "cli/agents-cli.bclj"} (integer? ^{:line 2218 :file "cli/agents-cli.bclj"} (:expected receipt)) ^{:line 2219 :file "cli/agents-cli.bclj"} (integer? ^{:line 2219 :file "cli/agents-cli.bclj"} (:committed receipt)) ^{:line 2220 :file "cli/agents-cli.bclj"} (boolean? ^{:line 2220 :file "cli/agents-cli.bclj"} (:complete receipt)) ^{:line 2221 :file "cli/agents-cli.bclj"} (string? ^{:line 2221 :file "cli/agents-cli.bclj"} (:reason receipt)))) ^{:line 2212 :file "cli/agents-cli.bclj"} (do
  ^{:line 2222 :file "cli/agents-cli.bclj"} (delegate-die "North capture returned a malformed structured receipt")))
  ^{:line 2223 :file "cli/agents-cli.bclj"} (let [id ^{:line 2223 :file "cli/agents-cli.bclj"} (normalize-delegate-referent ^{:line 2223 :file "cli/agents-cli.bclj"} (:id receipt))]
  ^{:line 2224 :file "cli/agents-cli.bclj"} (if ^{:line 2224 :file "cli/agents-cli.bclj"} (not ^{:line 2224 :file "cli/agents-cli.bclj"} (and ^{:line 2224 :file "cli/agents-cli.bclj"} (:complete receipt) ^{:line 2225 :file "cli/agents-cli.bclj"} (= "captured" ^{:line 2225 :file "cli/agents-cli.bclj"} (:reason receipt)) ^{:line 2226 :file "cli/agents-cli.bclj"} (= ^{:line 2226 :file "cli/agents-cli.bclj"} (str "@" id) ^{:line 2226 :file "cli/agents-cli.bclj"} (:referent receipt)) ^{:line 2227 :file "cli/agents-cli.bclj"} (= title ^{:line 2227 :file "cli/agents-cli.bclj"} (:title receipt)) ^{:line 2228 :file "cli/agents-cli.bclj"} (pos? ^{:line 2228 :file "cli/agents-cli.bclj"} (:expected receipt)) ^{:line 2229 :file "cli/agents-cli.bclj"} (= ^{:line 2229 :file "cli/agents-cli.bclj"} (:expected receipt) ^{:line 2229 :file "cli/agents-cli.bclj"} (:committed receipt)))) ^{:line 2224 :file "cli/agents-cli.bclj"} (do
  ^{:line 2230 :file "cli/agents-cli.bclj"} (delegate-die "North capture was partial; delegate spawn refused before provider execution")))
  ^{:line 2231 :file "cli/agents-cli.bclj"} (let [referent ^{:line 2231 :file "cli/agents-cli.bclj"} (read-delegate-referent! id)]
  ^{:line 2232 :file "cli/agents-cli.bclj"} (if ^{:line 2232 :file "cli/agents-cli.bclj"} (not ^{:line 2232 :file "cli/agents-cli.bclj"} (and ^{:line 2232 :file "cli/agents-cli.bclj"} (= title ^{:line 2232 :file "cli/agents-cli.bclj"} (:title referent)) ^{:line 2232 :file "cli/agents-cli.bclj"} (:committed? referent))) ^{:line 2232 :file "cli/agents-cli.bclj"} (do
  ^{:line 2233 :file "cli/agents-cli.bclj"} (delegate-die "captured delegate referent failed exact title/commit readback")))
  ^{:line 2234 :file "cli/agents-cli.bclj"} (assoc referent :source :captured))))))

^{:line 2238 :file "cli/agents-cli.bclj"} (def delegate-link-timeout-ms 45000)

^{:line 2240 :file "cli/agents-cli.bclj"} (defn- capture-recursive-child-referent! [task parent]
  ^{:line 2243 :file "cli/agents-cli.bclj"} (let [captured ^{:line 2243 :file "cli/agents-cli.bclj"} (capture-delegate-referent! task)
   child ^{:line 2244 :file "cli/agents-cli.bclj"} (:id captured)
   linked ^{:line 2245 :file "cli/agents-cli.bclj"} (run ^{:line 2245 :file "cli/agents-cli.bclj"} [NORTH-CLI "fact" "tell" child "part_of" parent] :timeout delegate-link-timeout-ms)]
  ^{:line 2247 :file "cli/agents-cli.bclj"} (if ^{:line 2247 :file "cli/agents-cli.bclj"} (not ^{:line 2247 :file "cli/agents-cli.bclj"} (:ok linked)) ^{:line 2247 :file "cli/agents-cli.bclj"} (do
  ^{:line 2250 :file "cli/agents-cli.bclj"} (run ^{:line 2250 :file "cli/agents-cli.bclj"} [NORTH-CLI "fact" "tell" child "abandoned" "recursive child binding failed before provider execution"] :timeout delegate-link-timeout-ms)
  ^{:line 2253 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2253 :file "cli/agents-cli.bclj"} (str "North could not link recursive child @" child " part_of @" parent))))
  ^{:line 2255 :file "cli/agents-cli.bclj"} (let [verified ^{:line 2255 :file "cli/agents-cli.bclj"} (read-delegate-referent! child)
   parents ^{:line 2256 :file "cli/agents-cli.bclj"} (->> ^{:line 2256 :file "cli/agents-cli.bclj"} (:facts verified) ^{:line 2257 :file "cli/agents-cli.bclj"} (filter ^{:line 2257 :file "cli/agents-cli.bclj"} (fn [%1] ^{:line 2257 :file "cli/agents-cli.bclj"} (= "part_of" ^{:line 2257 :file "cli/agents-cli.bclj"} (:predicate %1)))) ^{:line 2258 :file "cli/agents-cli.bclj"} (map ^{:line 2258 :file "cli/agents-cli.bclj"} (fn [fact] ^{:line 2258 :file "cli/agents-cli.bclj"} (:value fact))) set)]
  ^{:line 2260 :file "cli/agents-cli.bclj"} (if ^{:line 2260 :file "cli/agents-cli.bclj"} (not ^{:line 2260 :file "cli/agents-cli.bclj"} (= ^{:line 2260 :file "cli/agents-cli.bclj"} #{^{:line 2260 :file "cli/agents-cli.bclj"} (str "@" parent)} parents)) ^{:line 2260 :file "cli/agents-cli.bclj"} (do
  ^{:line 2261 :file "cli/agents-cli.bclj"} (run ^{:line 2261 :file "cli/agents-cli.bclj"} [NORTH-CLI "fact" "tell" child "abandoned" "recursive child link failed exact readback before provider execution"] :timeout delegate-link-timeout-ms)
  ^{:line 2264 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2264 :file "cli/agents-cli.bclj"} (str "recursive child @" child " did not read back exact parent @" parent))))
  ^{:line 2266 :file "cli/agents-cli.bclj"} (assoc verified :source :recursive-child :parent parent))))

^{:line 2268 :file "cli/agents-cli.bclj"} (defn resolve-recursive-child-referent! [task dry?]
  ^{:line 2271 :file "cli/agents-cli.bclj"} (let [{:keys [kind referent residue?]} ^{:line 2271 :file "cli/agents-cli.bclj"} (managed-referent-binding)]
  ^{:line 2272 :file "cli/agents-cli.bclj"} (if ^{:line 2272 :file "cli/agents-cli.bclj"} (not ^{:line 2272 :file "cli/agents-cli.bclj"} (= kind :complete)) ^{:line 2272 :file "cli/agents-cli.bclj"} (do
  ^{:line 2273 :file "cli/agents-cli.bclj"} (if residue? ^{:line 2273 :file "cli/agents-cli.bclj"} (do
  ^{:line 2274 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2275 :file "cli/agents-cli.bclj"} (println ^{:line 2275 :file "cli/agents-cli.bclj"} (ylw "recursive spawn found unverified parent run residue")))))
  ^{:line 2276 :file "cli/agents-cli.bclj"} (delegate-die "recursive orchestrator spawn requires its exact parent run/referent reservation")))
  ^{:line 2278 :file "cli/agents-cli.bclj"} (if dry? ^{:line 2279 :file "cli/agents-cli.bclj"} {:id "recursive-child-on-execution" :title ^{:line 2280 :file "cli/agents-cli.bclj"} (delegate-referent-title task) :facts ^{:line 2281 :file "cli/agents-cli.bclj"} [^{:line 2281 :file "cli/agents-cli.bclj"} {:predicate "entity_kind" :value "referent"} ^{:line 2282 :file "cli/agents-cli.bclj"} {:predicate "title" :value ^{:line 2282 :file "cli/agents-cli.bclj"} (delegate-referent-title task)} ^{:line 2283 :file "cli/agents-cli.bclj"} {:predicate "committed" :value "dry-run"} ^{:line 2284 :file "cli/agents-cli.bclj"} {:predicate "part_of" :value ^{:line 2284 :file "cli/agents-cli.bclj"} (str "@" referent)}] :committed? true :done-when ^{:line 2286 :file "cli/agents-cli.bclj"} [] :source :dry-recursive-child :parent referent} ^{:line 2289 :file "cli/agents-cli.bclj"} (capture-recursive-child-referent! task referent))))

^{:line 2291 :file "cli/agents-cli.bclj"} (defn resolve-delegate-referent! [{:keys [task explicit-referent handoff?]} dry?]
  ^{:line 2294 :file "cli/agents-cli.bclj"} (cond
  explicit-referent ^{:line 2296 :file "cli/agents-cli.bclj"} (let [referent ^{:line 2296 :file "cli/agents-cli.bclj"} (read-delegate-referent! explicit-referent)
   terminal-predicates ^{:line 2297 :file "cli/agents-cli.bclj"} (->> ^{:line 2297 :file "cli/agents-cli.bclj"} (:facts referent) ^{:line 2298 :file "cli/agents-cli.bclj"} (map ^{:line 2298 :file "cli/agents-cli.bclj"} (fn [fact] ^{:line 2298 :file "cli/agents-cli.bclj"} (:predicate fact))) ^{:line 2299 :file "cli/agents-cli.bclj"} (filter ^{:line 2299 :file "cli/agents-cli.bclj"} (fn [predicate] ^{:line 2300 :file "cli/agents-cli.bclj"} (contains? ^{:line 2300 :file "cli/agents-cli.bclj"} #{"outcome" "abandoned"} predicate))) set)]
  ^{:line 2302 :file "cli/agents-cli.bclj"} (if ^{:line 2302 :file "cli/agents-cli.bclj"} (and handoff? ^{:line 2302 :file "cli/agents-cli.bclj"} (seq terminal-predicates)) ^{:line 2302 :file "cli/agents-cli.bclj"} (do
  ^{:line 2303 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2304 :file "cli/agents-cli.bclj"} (str "session hard-cap artifact referent @" ^{:line 2304 :file "cli/agents-cli.bclj"} (:id referent) " is already terminal (" ^{:line 2306 :file "cli/agents-cli.bclj"} (str/join ", " ^{:line 2306 :file "cli/agents-cli.bclj"} (sort terminal-predicates)) ")"))))
  ^{:line 2307 :file "cli/agents-cli.bclj"} (assoc referent :source :explicit))
  :else ^{:line 2310 :file "cli/agents-cli.bclj"} (let [{:keys [kind referent residue?]} ^{:line 2310 :file "cli/agents-cli.bclj"} (managed-referent-binding)]
  ^{:line 2311 :file "cli/agents-cli.bclj"} (case kind
    :complete ^{:line 2312 :file "cli/agents-cli.bclj"} (if dry? ^{:line 2313 :file "cli/agents-cli.bclj"} (resolve-recursive-child-referent! task true) ^{:line 2314 :file "cli/agents-cli.bclj"} (capture-recursive-child-referent! task referent))
    :none ^{:line 2315 :file "cli/agents-cli.bclj"} (do
  ^{:line 2316 :file "cli/agents-cli.bclj"} (if residue? ^{:line 2316 :file "cli/agents-cli.bclj"} (do
  ^{:line 2317 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2318 :file "cli/agents-cli.bclj"} (println ^{:line 2319 :file "cli/agents-cli.bclj"} (ylw "ignoring unverified ambient North run/referent residue; a fresh delegate referent is required")))))
  ^{:line 2320 :file "cli/agents-cli.bclj"} (if dry? ^{:line 2321 :file "cli/agents-cli.bclj"} {:id "capture-on-execution" :title task :facts ^{:line 2323 :file "cli/agents-cli.bclj"} [^{:line 2323 :file "cli/agents-cli.bclj"} {:predicate "entity_kind" :value "referent"} ^{:line 2324 :file "cli/agents-cli.bclj"} {:predicate "title" :value task} ^{:line 2325 :file "cli/agents-cli.bclj"} {:predicate "committed" :value "dry-run"}] :committed? true :done-when ^{:line 2327 :file "cli/agents-cli.bclj"} [] :source :dry-capture} ^{:line 2329 :file "cli/agents-cli.bclj"} (capture-delegate-referent! task)))
    ^{:line 2330 :file "cli/agents-cli.bclj"} (throw ^{:line 2330 :file "cli/agents-cli.bclj"} (IllegalArgumentException. ^{:line 2331 :file "cli/agents-cli.bclj"} (str "No matching clause: " kind)))))))

^{:line 2333 :file "cli/agents-cli.bclj"} (defn cmd-bind-child-referent! [[task & extra]]
  ^{:line 2334 :file "cli/agents-cli.bclj"} (north.topology-authority/require-coordination! "bind recursive child referent")
  ^{:line 2335 :file "cli/agents-cli.bclj"} (if ^{:line 2335 :file "cli/agents-cli.bclj"} (or ^{:line 2335 :file "cli/agents-cli.bclj"} (nil? task) ^{:line 2335 :file "cli/agents-cli.bclj"} (seq extra)) ^{:line 2335 :file "cli/agents-cli.bclj"} (do
  ^{:line 2336 :file "cli/agents-cli.bclj"} (delegate-die "internal bind-child-referent requires exactly one task argument")))
  ^{:line 2337 :file "cli/agents-cli.bclj"} (let [{:keys [id parent]} ^{:line 2337 :file "cli/agents-cli.bclj"} (resolve-recursive-child-referent! task false)]
  ^{:line 2338 :file "cli/agents-cli.bclj"} (println ^{:line 2338 :file "cli/agents-cli.bclj"} (json/generate-string ^{:line 2338 :file "cli/agents-cli.bclj"} {:referent id :parent parent}))))

^{:line 2340 :file "cli/agents-cli.bclj"} (defn- parse-delegate-args [args]
  ^{:line 2341 :file "cli/agents-cli.bclj"} (let [handoff? ^{:line 2341 :file "cli/agents-cli.bclj"} (= "--handoff" ^{:line 2341 :file "cli/agents-cli.bclj"} (first args))
   handoff ^{:line 2342 :file "cli/agents-cli.bclj"} (if handoff? ^{:line 2342 :file "cli/agents-cli.bclj"} (do
  ^{:line 2342 :file "cli/agents-cli.bclj"} (second args)))
   _ ^{:line 2343 :file "cli/agents-cli.bclj"} (if ^{:line 2343 :file "cli/agents-cli.bclj"} (and handoff? ^{:line 2344 :file "cli/agents-cli.bclj"} (or ^{:line 2344 :file "cli/agents-cli.bclj"} (nil? handoff) ^{:line 2344 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 2344 :file "cli/agents-cli.bclj"} (str handoff) "--"))) ^{:line 2343 :file "cli/agents-cli.bclj"} (do
  ^{:line 2345 :file "cli/agents-cli.bclj"} (delegate-die "--handoff requires a session hard-cap artifact")))
   task ^{:line 2346 :file "cli/agents-cli.bclj"} (if ^{:line 2346 :file "cli/agents-cli.bclj"} (not handoff?) ^{:line 2346 :file "cli/agents-cli.bclj"} (do
  ^{:line 2346 :file "cli/agents-cli.bclj"} (first args)))
   _ ^{:line 2347 :file "cli/agents-cli.bclj"} (if ^{:line 2347 :file "cli/agents-cli.bclj"} (and ^{:line 2347 :file "cli/agents-cli.bclj"} (not handoff?) ^{:line 2348 :file "cli/agents-cli.bclj"} (or ^{:line 2348 :file "cli/agents-cli.bclj"} (nil? task) ^{:line 2348 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 2348 :file "cli/agents-cli.bclj"} (str task) "--"))) ^{:line 2347 :file "cli/agents-cli.bclj"} (do
  ^{:line 2349 :file "cli/agents-cli.bclj"} (delegate-die "delegate requires one quoted intent or --handoff")))
   remaining ^{:line 2351 :file "cli/agents-cli.bclj"} (if handoff? ^{:line 2351 :file "cli/agents-cli.bclj"} (nnext args) ^{:line 2351 :file "cli/agents-cli.bclj"} (rest args))]
  ^{:line 2352 :file "cli/agents-cli.bclj"} (loop [xs remaining
   parsed ^{:line 2353 :file "cli/agents-cli.bclj"} (cond-> ^{:line 2353 :file "cli/agents-cli.bclj"} {:task task :forward ^{:line 2353 :file "cli/agents-cli.bclj"} []} handoff? ^{:line 2354 :file "cli/agents-cli.bclj"} (assoc :handoff handoff))]
  ^{:line 2355 :file "cli/agents-cli.bclj"} (let [x ^{:line 2355 :file "cli/agents-cli.bclj"} (first xs)]
  ^{:line 2355 :file "cli/agents-cli.bclj"} (if x ^{:line 2356 :file "cli/agents-cli.bclj"} (case x
    "--role" ^{:line 2358 :file "cli/agents-cli.bclj"} (let [role ^{:line 2358 :file "cli/agents-cli.bclj"} (second xs)]
  ^{:line 2359 :file "cli/agents-cli.bclj"} (if ^{:line 2359 :file "cli/agents-cli.bclj"} (or ^{:line 2359 :file "cli/agents-cli.bclj"} (nil? role) ^{:line 2359 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 2359 :file "cli/agents-cli.bclj"} (str role) "--")) ^{:line 2359 :file "cli/agents-cli.bclj"} (do
  ^{:line 2360 :file "cli/agents-cli.bclj"} (delegate-die "--role requires a Orchestration worker role")))
  ^{:line 2361 :file "cli/agents-cli.bclj"} (if ^{:line 2361 :file "cli/agents-cli.bclj"} (:mode parsed) ^{:line 2361 :file "cli/agents-cli.bclj"} (do
  ^{:line 2362 :file "cli/agents-cli.bclj"} (delegate-die "choose exactly one delegation mode: --role or --composite")))
  ^{:line 2363 :file "cli/agents-cli.bclj"} (recur ^{:line 2363 :file "cli/agents-cli.bclj"} (nnext xs) ^{:line 2363 :file "cli/agents-cli.bclj"} (assoc parsed :mode :atomic :role role)))
    "--composite" ^{:line 2366 :file "cli/agents-cli.bclj"} (do
  ^{:line 2367 :file "cli/agents-cli.bclj"} (if ^{:line 2367 :file "cli/agents-cli.bclj"} (:mode parsed) ^{:line 2367 :file "cli/agents-cli.bclj"} (do
  ^{:line 2368 :file "cli/agents-cli.bclj"} (delegate-die "choose exactly one delegation mode: --role or --composite")))
  ^{:line 2369 :file "cli/agents-cli.bclj"} (recur ^{:line 2369 :file "cli/agents-cli.bclj"} (rest xs) ^{:line 2369 :file "cli/agents-cli.bclj"} (assoc parsed :mode :composite)))
    "--context" ^{:line 2372 :file "cli/agents-cli.bclj"} (let [path ^{:line 2372 :file "cli/agents-cli.bclj"} (second xs)]
  ^{:line 2373 :file "cli/agents-cli.bclj"} (if ^{:line 2373 :file "cli/agents-cli.bclj"} (or ^{:line 2373 :file "cli/agents-cli.bclj"} (nil? path) ^{:line 2373 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 2373 :file "cli/agents-cli.bclj"} (str path) "--")) ^{:line 2373 :file "cli/agents-cli.bclj"} (do
  ^{:line 2374 :file "cli/agents-cli.bclj"} (delegate-die "--context requires a brief file")))
  ^{:line 2375 :file "cli/agents-cli.bclj"} (if ^{:line 2375 :file "cli/agents-cli.bclj"} (:handoff parsed) ^{:line 2375 :file "cli/agents-cli.bclj"} (do
  ^{:line 2376 :file "cli/agents-cli.bclj"} (delegate-die "--handoff supplies its exact context; omit --context")))
  ^{:line 2377 :file "cli/agents-cli.bclj"} (recur ^{:line 2377 :file "cli/agents-cli.bclj"} (nnext xs) ^{:line 2377 :file "cli/agents-cli.bclj"} (assoc parsed :context path)))
    "--referent" ^{:line 2380 :file "cli/agents-cli.bclj"} (let [referent ^{:line 2380 :file "cli/agents-cli.bclj"} (second xs)]
  ^{:line 2381 :file "cli/agents-cli.bclj"} (if ^{:line 2381 :file "cli/agents-cli.bclj"} (or ^{:line 2381 :file "cli/agents-cli.bclj"} (nil? referent) ^{:line 2381 :file "cli/agents-cli.bclj"} (str/starts-with? ^{:line 2381 :file "cli/agents-cli.bclj"} (str referent) "--")) ^{:line 2381 :file "cli/agents-cli.bclj"} (do
  ^{:line 2382 :file "cli/agents-cli.bclj"} (delegate-die "--referent requires a North referent id")))
  ^{:line 2383 :file "cli/agents-cli.bclj"} (if ^{:line 2383 :file "cli/agents-cli.bclj"} (:handoff parsed) ^{:line 2383 :file "cli/agents-cli.bclj"} (do
  ^{:line 2384 :file "cli/agents-cli.bclj"} (delegate-die "--handoff supplies its exact referent; omit --referent")))
  ^{:line 2385 :file "cli/agents-cli.bclj"} (if ^{:line 2385 :file "cli/agents-cli.bclj"} (:referent parsed) ^{:line 2385 :file "cli/agents-cli.bclj"} (do
  ^{:line 2386 :file "cli/agents-cli.bclj"} (delegate-die "delegate accepts exactly one --referent")))
  ^{:line 2387 :file "cli/agents-cli.bclj"} (recur ^{:line 2387 :file "cli/agents-cli.bclj"} (nnext xs) ^{:line 2387 :file "cli/agents-cli.bclj"} (assoc parsed :referent referent)))
    "--handoff" ^{:line 2390 :file "cli/agents-cli.bclj"} (delegate-die "--handoff must replace the task and may appear exactly once")
    ^{:line 2392 :file "cli/agents-cli.bclj"} (recur ^{:line 2392 :file "cli/agents-cli.bclj"} (rest xs) ^{:line 2392 :file "cli/agents-cli.bclj"} (update parsed :forward conj x))) parsed)))))

^{:line 2395 :file "cli/agents-cli.bclj"} (defn delegate-brief [{:keys [task mode context]} {:keys [id committed? done-when]}]
  ^{:line 2398 :file "cli/agents-cli.bclj"} (let [context-block ^{:line 2398 :file "cli/agents-cli.bclj"} (if context ^{:line 2398 :file "cli/agents-cli.bclj"} (do
  ^{:line 2398 :file "cli/agents-cli.bclj"} (str "CONTEXT BRIEF:\n" context "\n\n")))
   proof-block ^{:line 2399 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2400 :file "cli/agents-cli.bclj"} (seq done-when) ^{:line 2401 :file "cli/agents-cli.bclj"} (str "North has prebound this lane and its immutable starting done_when set to @" id ". Run each exact bar, then record its observation with " "`north-delivery-evidence record \"<exact bar>\" \"<observed result>\"`; " "provider success without those records is not delivery evidence.")
  committed? ^{:line 2407 :file "cli/agents-cli.bclj"} (str "North has prebound this accepted, currently barless referent to @" id ". FIRST ACT: define exact probe + expected-result criteria with " "`north fact tell " id " done_when \"<probe + expected result>\"`. " "After each probe, use `north-delivery-evidence record \"<exact bar>\" " "\"<observed result>\"`; provider success alone is not delivery evidence.")
  :else ^{:line 2414 :file "cli/agents-cli.bclj"} (str "North has prebound this title-bearing referent to @" id ". Record exact done_when criteria before claiming completion, then use " "`north-delivery-evidence record \"<exact bar>\" \"<observed result>\"` after each probe."))]
  ^{:line 2417 :file "cli/agents-cli.bclj"} (str context-block "DELEGATE TASK: " task "\n\n" "NORTH DELIVERY CONTRACT: " proof-block "\n\n" ^{:line 2420 :file "cli/agents-cli.bclj"} (if ^{:line 2420 :file "cli/agents-cli.bclj"} (= mode :composite) ^{:line 2421 :file "cli/agents-cli.bclj"} (str "COMPOSITE INTAKE: @" id " is the aggregate reduction/checkpoint referent. Create a distinct " "title-bearing child referent linked `part_of @" id "` for every terminal piece, and bind each child run to its own referent; " "never make workers prove the aggregate bar set. Keep the North listener/" "continuation live, checkpoint each result as it arrives, and reconcile " "every child before publishing the aggregate outcome.") ^{:line 2428 :file "cli/agents-cli.bclj"} (str "ATOMIC INTAKE: use @" id " as the single durable work/evidence referent and return one evidence-backed result.")))))

^{:line 2431 :file "cli/agents-cli.bclj"} (defn cmd-delegate! [args]
  ^{:line 2432 :file "cli/agents-cli.bclj"} (north.topology-authority/require-coordination! "delegate")
  ^{:line 2433 :file "cli/agents-cli.bclj"} (let [{:keys [task mode role context referent handoff forward]} ^{:line 2433 :file "cli/agents-cli.bclj"} (parse-delegate-args args)
   expert-mode mode
   adopted ^{:line 2435 :file "cli/agents-cli.bclj"} (if handoff ^{:line 2435 :file "cli/agents-cli.bclj"} (do
  ^{:line 2435 :file "cli/agents-cli.bclj"} (read-delegate-handoff! handoff)))
   task ^{:line 2436 :file "cli/agents-cli.bclj"} (or ^{:line 2436 :file "cli/agents-cli.bclj"} (:task adopted) task)
   referent ^{:line 2437 :file "cli/agents-cli.bclj"} (or ^{:line 2437 :file "cli/agents-cli.bclj"} (:referent adopted) referent)
   ctx-file context
   ctx ^{:line 2439 :file "cli/agents-cli.bclj"} (or ^{:line 2439 :file "cli/agents-cli.bclj"} (:context adopted) ^{:line 2440 :file "cli/agents-cli.bclj"} (if ctx-file ^{:line 2440 :file "cli/agents-cli.bclj"} (do
  ^{:line 2441 :file "cli/agents-cli.bclj"} (let [f ^{:line 2441 :file "cli/agents-cli.bclj"} (io/file ctx-file)]
  ^{:line 2442 :file "cli/agents-cli.bclj"} (if ^{:line 2442 :file "cli/agents-cli.bclj"} (not ^{:line 2442 :file "cli/agents-cli.bclj"} (.exists f)) ^{:line 2442 :file "cli/agents-cli.bclj"} (do
  ^{:line 2443 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2443 :file "cli/agents-cli.bclj"} (str "context file not found: " ctx-file))))
  ^{:line 2444 :file "cli/agents-cli.bclj"} (str/trim ^{:line 2444 :file "cli/agents-cli.bclj"} (slurp f))))))
   routing-override ^{:line 2445 :file "cli/agents-cli.bclj"} (if ^{:line 2445 :file "cli/agents-cli.bclj"} (not expert-mode) ^{:line 2445 :file "cli/agents-cli.bclj"} (do
  ^{:line 2446 :file "cli/agents-cli.bclj"} (some delegate-routing-override-flags forward)))
   _ ^{:line 2447 :file "cli/agents-cli.bclj"} (if routing-override ^{:line 2447 :file "cli/agents-cli.bclj"} (do
  ^{:line 2448 :file "cli/agents-cli.bclj"} (delegate-die ^{:line 2449 :file "cli/agents-cli.bclj"} (str routing-override " is a routing override; use --role or --composite to bypass intent selection"))))
   bare-spawn ^{:line 2451 :file "cli/agents-cli.bclj"} (if ^{:line 2451 :file "cli/agents-cli.bclj"} (not expert-mode) ^{:line 2451 :file "cli/agents-cli.bclj"} (do
  ^{:line 2452 :file "cli/agents-cli.bclj"} (parse-spawn-args ^{:line 2452 :file "cli/agents-cli.bclj"} (into ^{:line 2452 :file "cli/agents-cli.bclj"} [task] forward))))
   _ ^{:line 2453 :file "cli/agents-cli.bclj"} (if ^{:line 2453 :file "cli/agents-cli.bclj"} (and bare-spawn ^{:line 2453 :file "cli/agents-cli.bclj"} (not= ^{:line 2453 :file "cli/agents-cli.bclj"} [task] ^{:line 2453 :file "cli/agents-cli.bclj"} (:positionals bare-spawn))) ^{:line 2453 :file "cli/agents-cli.bclj"} (do
  ^{:line 2454 :file "cli/agents-cli.bclj"} (delegate-die "bare delegate accepts exactly one intent and runtime controls")))
   selected ^{:line 2455 :file "cli/agents-cli.bclj"} (if ^{:line 2455 :file "cli/agents-cli.bclj"} (not expert-mode) ^{:line 2455 :file "cli/agents-cli.bclj"} (do
  ^{:line 2456 :file "cli/agents-cli.bclj"} (select-delegation-run-design! task ctx)))
   routing-request ^{:line 2457 :file "cli/agents-cli.bclj"} (:routing-request selected)
   routing-assessment ^{:line 2458 :file "cli/agents-cli.bclj"} (:routing-assessment selected)
   mode ^{:line 2459 :file "cli/agents-cli.bclj"} (or expert-mode ^{:line 2460 :file "cli/agents-cli.bclj"} (case ^{:line 2460 :file "cli/agents-cli.bclj"} (:topology routing-request)
    "worker" :atomic
    "orchestrator" :composite
    ^{:line 2463 :file "cli/agents-cli.bclj"} (delegate-die "Agent Machinery selected an unknown delegation topology")))
   spawn-role ^{:line 2465 :file "cli/agents-cli.bclj"} (if expert-mode ^{:line 2465 :file "cli/agents-cli.bclj"} (do
  ^{:line 2466 :file "cli/agents-cli.bclj"} (if ^{:line 2466 :file "cli/agents-cli.bclj"} (= expert-mode :composite) "director" role)))
   parsed-spawn ^{:line 2467 :file "cli/agents-cli.bclj"} (if expert-mode ^{:line 2467 :file "cli/agents-cli.bclj"} (do
  ^{:line 2468 :file "cli/agents-cli.bclj"} (parse-spawn-args ^{:line 2468 :file "cli/agents-cli.bclj"} (into ^{:line 2468 :file "cli/agents-cli.bclj"} [spawn-role task] forward))))
   effective-topology ^{:line 2469 :file "cli/agents-cli.bclj"} (if parsed-spawn ^{:line 2469 :file "cli/agents-cli.bclj"} (do
  ^{:line 2469 :file "cli/agents-cli.bclj"} (resolved-spawn-topology parsed-spawn)))
   _ ^{:line 2470 :file "cli/agents-cli.bclj"} (if ^{:line 2470 :file "cli/agents-cli.bclj"} (and ^{:line 2470 :file "cli/agents-cli.bclj"} (= expert-mode :atomic) ^{:line 2471 :file "cli/agents-cli.bclj"} (= "orchestrator" effective-topology)) ^{:line 2470 :file "cli/agents-cli.bclj"} (do
  ^{:line 2472 :file "cli/agents-cli.bclj"} (delegate-die "--role is an atomic terminal-worker handoff; use --composite for orchestrator work")))
   inherited-notify ^{:line 2473 :file "cli/agents-cli.bclj"} (and ^{:line 2473 :file "cli/agents-cli.bclj"} (not ^{:line 2473 :file "cli/agents-cli.bclj"} (contains? ^{:line 2473 :file "cli/agents-cli.bclj"} (set forward) "--notify")) ^{:line 2474 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_NOTIFY"))
   controls ^{:line 2475 :file "cli/agents-cli.bclj"} (cond-> forward inherited-notify ^{:line 2476 :file "cli/agents-cli.bclj"} (into ^{:line 2476 :file "cli/agents-cli.bclj"} ["--notify" inherited-notify]))]
  ^{:line 2477 :file "cli/agents-cli.bclj"} (binding [*delegate-request* ^{:line 2477 :file "cli/agents-cli.bclj"} {:task task :mode mode :context ctx :explicit-referent referent :handoff? ^{:line 2479 :file "cli/agents-cli.bclj"} (boolean adopted)}]
  ^{:line 2480 :file "cli/agents-cli.bclj"} (if expert-mode ^{:line 2481 :file "cli/agents-cli.bclj"} (cmd-spawn ^{:line 2481 :file "cli/agents-cli.bclj"} (into ^{:line 2481 :file "cli/agents-cli.bclj"} [spawn-role task] controls)) ^{:line 2482 :file "cli/agents-cli.bclj"} (cmd-spawn-selected! routing-request routing-assessment task controls)))))

^{:line 2484 :file "cli/agents-cli.bclj"} (def watch-usage "north agent watch <agent-id> [--control]")

^{:line 2485 :file "cli/agents-cli.bclj"} (def wire-watch-version "north:wire:v2")

^{:line 2486 :file "cli/agents-cli.bclj"} (def wire-watch-kinds ^{:line 2487 :file "cli/agents-cli.bclj"} #{"run.started" "run.progress" "message.recorded" "model-call.started" "model-call.completed" "tool.admitted" "tool.progress" "tool.terminal" "artifact.published" "resource.pressure" "run.terminated"})

^{:line 2491 :file "cli/agents-cli.bclj"} (def max-watch-json-line-bytes ^{:line 2491 :file "cli/agents-cli.bclj"} (* 2 1024 1024))

^{:line 2494 :file "cli/agents-cli.bclj"} (def max-watch-output-columns 180)

^{:line 2495 :file "cli/agents-cli.bclj"} (def max-watch-field-codepoints 40)

^{:line 2496 :file "cli/agents-cli.bclj"} (def max-watch-output-codepoints ^{:line 2496 :file "cli/agents-cli.bclj"} (quot max-watch-output-columns 2))

^{:line 2498 :file "cli/agents-cli.bclj"} (defn watch-safe-text
  "Collapse terminal controls and whitespace, then bound one display field by\n  Unicode code points. The final line is bounded again after composition."
  ([value]
    ^{:line 2501 :file "cli/agents-cli.bclj"} (watch-safe-text value max-watch-field-codepoints))
  ([value limit]
    ^{:line 2504 :file "cli/agents-cli.bclj"} (let [bound ^{:line 2504 :file "cli/agents-cli.bclj"} (max 1 ^{:line 2504 :file "cli/agents-cli.bclj"} (long limit))
   source ^{:line 2505 :file "cli/agents-cli.bclj"} (str ^{:line 2505 :file "cli/agents-cli.bclj"} (or value ""))
   sampled ^{:line 2506 :file "cli/agents-cli.bclj"} (.toArray ^{:line 2506 :file "cli/agents-cli.bclj"} (.limit ^{:line 2506 :file "cli/agents-cli.bclj"} (.codePoints source) ^{:line 2506 :file "cli/agents-cli.bclj"} (inc bound)))
   truncated? ^{:line 2507 :file "cli/agents-cli.bclj"} (> ^{:line 2507 :file "cli/agents-cli.bclj"} (alength sampled) bound)
   retained ^{:line 2508 :file "cli/agents-cli.bclj"} (if truncated? ^{:line 2508 :file "cli/agents-cli.bclj"} (dec bound) bound)
   bounded ^{:line 2509 :file "cli/agents-cli.bclj"} (if ^{:line 2509 :file "cli/agents-cli.bclj"} (> ^{:line 2509 :file "cli/agents-cli.bclj"} (alength sampled) retained) ^{:line 2510 :file "cli/agents-cli.bclj"} (java.util.Arrays/copyOf sampled retained) sampled)
   cleaned ^{:line 2512 :file "cli/agents-cli.bclj"} (int-array ^{:line 2513 :file "cli/agents-cli.bclj"} (map ^{:line 2513 :file "cli/agents-cli.bclj"} (fn [codepoint] ^{:line 2514 :file "cli/agents-cli.bclj"} (if ^{:line 2514 :file "cli/agents-cli.bclj"} (or ^{:line 2514 :file "cli/agents-cli.bclj"} (Character/isISOControl codepoint) ^{:line 2515 :file "cli/agents-cli.bclj"} (Character/isWhitespace codepoint) ^{:line 2516 :file "cli/agents-cli.bclj"} (= Character/FORMAT ^{:line 2516 :file "cli/agents-cli.bclj"} (Character/getType codepoint))) ^{:line 2517 :file "cli/agents-cli.bclj"} (int \space) codepoint)) bounded))
   text ^{:line 2520 :file "cli/agents-cli.bclj"} (str/trim ^{:line 2520 :file "cli/agents-cli.bclj"} (str/replace ^{:line 2520 :file "cli/agents-cli.bclj"} (String. cleaned 0 ^{:line 2520 :file "cli/agents-cli.bclj"} (alength cleaned)) #" +" " "))]
  ^{:line 2521 :file "cli/agents-cli.bclj"} (str text ^{:line 2521 :file "cli/agents-cli.bclj"} (if truncated? ^{:line 2521 :file "cli/agents-cli.bclj"} (do
  "…"))))))

^{:line 2523 :file "cli/agents-cli.bclj"} (defn- watch-display-path [value]
  ^{:line 2524 :file "cli/agents-cli.bclj"} (let [raw ^{:line 2524 :file "cli/agents-cli.bclj"} (str ^{:line 2524 :file "cli/agents-cli.bclj"} (or value ""))
   home ^{:line 2525 :file "cli/agents-cli.bclj"} (if ^{:line 2525 :file "cli/agents-cli.bclj"} (and HOME ^{:line 2525 :file "cli/agents-cli.bclj"} (not ^{:line 2525 :file "cli/agents-cli.bclj"} (str/blank? HOME))) ^{:line 2525 :file "cli/agents-cli.bclj"} (do
  ^{:line 2526 :file "cli/agents-cli.bclj"} (try
  ^{:line 2526 :file "cli/agents-cli.bclj"} (.getCanonicalPath ^{:line 2526 :file "cli/agents-cli.bclj"} (io/file HOME))
  (catch Exception _
    HOME))))
   home-prefix ^{:line 2527 :file "cli/agents-cli.bclj"} (if home ^{:line 2527 :file "cli/agents-cli.bclj"} (do
  ^{:line 2527 :file "cli/agents-cli.bclj"} (str home java.io.File/separator)))
   shortened ^{:line 2528 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2529 :file "cli/agents-cli.bclj"} (= raw home) "~"
  ^{:line 2530 :file "cli/agents-cli.bclj"} (and home-prefix ^{:line 2530 :file "cli/agents-cli.bclj"} (str/starts-with? raw home-prefix)) ^{:line 2531 :file "cli/agents-cli.bclj"} (str "~/" ^{:line 2531 :file "cli/agents-cli.bclj"} (subs raw ^{:line 2531 :file "cli/agents-cli.bclj"} (count home-prefix)))
  :else raw)]
  ^{:line 2533 :file "cli/agents-cli.bclj"} (watch-safe-text shortened max-watch-field-codepoints)))

^{:line 2535 :file "cli/agents-cli.bclj"} (defn- watch-json-summary [value]
  ^{:line 2536 :file "cli/agents-cli.bclj"} (try
  ^{:line 2537 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2537 :file "cli/agents-cli.bclj"} (json/generate-string value))
  (catch Exception _
    "<unrenderable>")))

^{:line 2540 :file "cli/agents-cli.bclj"} (defn- watch-kv [label value]
  ^{:line 2543 :file "cli/agents-cli.bclj"} (if ^{:line 2543 :file "cli/agents-cli.bclj"} (some? value) ^{:line 2543 :file "cli/agents-cli.bclj"} (do
  ^{:line 2543 :file "cli/agents-cli.bclj"} (str " " label "=" ^{:line 2543 :file "cli/agents-cli.bclj"} (watch-safe-text value)))))

^{:line 2545 :file "cli/agents-cli.bclj"} (defn- watch-event-prefix [event]
  ^{:line 2546 :file "cli/agents-cli.bclj"} (str "[" ^{:line 2546 :file "cli/agents-cli.bclj"} (if ^{:line 2546 :file "cli/agents-cli.bclj"} (and ^{:line 2546 :file "cli/agents-cli.bclj"} (integer? ^{:line 2546 :file "cli/agents-cli.bclj"} (:sequence event)) ^{:line 2546 :file "cli/agents-cli.bclj"} (not ^{:line 2546 :file "cli/agents-cli.bclj"} (neg? ^{:line 2546 :file "cli/agents-cli.bclj"} (:sequence event)))) ^{:line 2547 :file "cli/agents-cli.bclj"} (:sequence event) "?") "] "))

^{:line 2551 :file "cli/agents-cli.bclj"} (defn- watch-progress-detail [progress]
  ^{:line 2552 :file "cli/agents-cli.bclj"} (str ^{:line 2552 :file "cli/agents-cli.bclj"} (watch-kv "action" ^{:line 2552 :file "cli/agents-cli.bclj"} (:currentAction progress)) ^{:line 2553 :file "cli/agents-cli.bclj"} (watch-kv "retry" ^{:line 2553 :file "cli/agents-cli.bclj"} (some-> ^{:line 2553 :file "cli/agents-cli.bclj"} (:retry progress) :attempt)) ^{:line 2554 :file "cli/agents-cli.bclj"} (watch-kv "fallback" ^{:line 2554 :file "cli/agents-cli.bclj"} (some-> ^{:line 2554 :file "cli/agents-cli.bclj"} (:fallback progress) :reason)) ^{:line 2555 :file "cli/agents-cli.bclj"} (watch-kv "compactions" ^{:line 2555 :file "cli/agents-cli.bclj"} (:compactions progress))))

^{:line 2557 :file "cli/agents-cli.bclj"} (defn- render-known-wire-event [event]
  ^{:line 2558 :file "cli/agents-cli.bclj"} (let [kind ^{:line 2558 :file "cli/agents-cli.bclj"} (:kind event)
   prefix ^{:line 2559 :file "cli/agents-cli.bclj"} (watch-event-prefix event)
   rendered ^{:line 2560 :file "cli/agents-cli.bclj"} (case kind
    "run.started" ^{:line 2562 :file "cli/agents-cli.bclj"} (str prefix "▶ run started" ^{:line 2562 :file "cli/agents-cli.bclj"} (watch-kv "owner" ^{:line 2562 :file "cli/agents-cli.bclj"} (:owner event)))
    "run.progress" ^{:line 2565 :file "cli/agents-cli.bclj"} (str prefix "… run " ^{:line 2565 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2565 :file "cli/agents-cli.bclj"} (:lifecycle event)) ^{:line 2566 :file "cli/agents-cli.bclj"} (watch-progress-detail ^{:line 2566 :file "cli/agents-cli.bclj"} (:progress event)))
    "message.recorded" ^{:line 2569 :file "cli/agents-cli.bclj"} (str prefix "message " ^{:line 2569 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2569 :file "cli/agents-cli.bclj"} (:role event)) "/" ^{:line 2570 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2570 :file "cli/agents-cli.bclj"} (:stage event)) ^{:line 2571 :file "cli/agents-cli.bclj"} (watch-kv "content" ^{:line 2571 :file "cli/agents-cli.bclj"} (if ^{:line 2571 :file "cli/agents-cli.bclj"} (contains? event :content) ^{:line 2571 :file "cli/agents-cli.bclj"} (do
  ^{:line 2572 :file "cli/agents-cli.bclj"} (watch-json-summary ^{:line 2572 :file "cli/agents-cli.bclj"} (:content event))))))
    "model-call.started" ^{:line 2575 :file "cli/agents-cli.bclj"} (str prefix "model ▶ " ^{:line 2575 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2575 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2575 :file "cli/agents-cli.bclj"} [:model :provider])) ^{:line 2576 :file "cli/agents-cli.bclj"} (watch-kv "tier" ^{:line 2576 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2576 :file "cli/agents-cli.bclj"} [:model :tier])) ^{:line 2577 :file "cli/agents-cli.bclj"} (watch-kv "effort" ^{:line 2577 :file "cli/agents-cli.bclj"} (:effort event)) ^{:line 2578 :file "cli/agents-cli.bclj"} (watch-kv "attempt" ^{:line 2578 :file "cli/agents-cli.bclj"} (:attempt event)))
    "model-call.completed" ^{:line 2581 :file "cli/agents-cli.bclj"} (str prefix "model " ^{:line 2581 :file "cli/agents-cli.bclj"} (if ^{:line 2581 :file "cli/agents-cli.bclj"} (= "succeeded" ^{:line 2581 :file "cli/agents-cli.bclj"} (:status event)) "✓" "✗") " " ^{:line 2582 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2582 :file "cli/agents-cli.bclj"} (:status event)) ^{:line 2583 :file "cli/agents-cli.bclj"} (watch-kv "origin" ^{:line 2583 :file "cli/agents-cli.bclj"} (:origin event)) ^{:line 2584 :file "cli/agents-cli.bclj"} (watch-kv "input" ^{:line 2584 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2584 :file "cli/agents-cli.bclj"} [:usage :lifetime :inputTokens])) ^{:line 2585 :file "cli/agents-cli.bclj"} (watch-kv "output" ^{:line 2585 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2585 :file "cli/agents-cli.bclj"} [:usage :lifetime :outputTokens])) ^{:line 2586 :file "cli/agents-cli.bclj"} (watch-kv "error" ^{:line 2586 :file "cli/agents-cli.bclj"} (:errorCode event)))
    "tool.admitted" ^{:line 2589 :file "cli/agents-cli.bclj"} (str prefix "tool ▶ " ^{:line 2589 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2589 :file "cli/agents-cli.bclj"} (:name event)) ^{:line 2590 :file "cli/agents-cli.bclj"} (watch-kv "id" ^{:line 2590 :file "cli/agents-cli.bclj"} (:toolCallId event)))
    "tool.progress" ^{:line 2593 :file "cli/agents-cli.bclj"} (str prefix "tool …" ^{:line 2593 :file "cli/agents-cli.bclj"} (watch-kv "id" ^{:line 2593 :file "cli/agents-cli.bclj"} (:toolCallId event)) ^{:line 2594 :file "cli/agents-cli.bclj"} (watch-kv "progress" ^{:line 2594 :file "cli/agents-cli.bclj"} (if ^{:line 2594 :file "cli/agents-cli.bclj"} (contains? event :progress) ^{:line 2594 :file "cli/agents-cli.bclj"} (do
  ^{:line 2595 :file "cli/agents-cli.bclj"} (watch-json-summary ^{:line 2595 :file "cli/agents-cli.bclj"} (:progress event))))))
    "tool.terminal" ^{:line 2598 :file "cli/agents-cli.bclj"} (str prefix "tool " ^{:line 2598 :file "cli/agents-cli.bclj"} (if ^{:line 2598 :file "cli/agents-cli.bclj"} (= "succeeded" ^{:line 2598 :file "cli/agents-cli.bclj"} (:status event)) "✓" "✗") " " ^{:line 2599 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2599 :file "cli/agents-cli.bclj"} (:status event)) ^{:line 2600 :file "cli/agents-cli.bclj"} (watch-kv "id" ^{:line 2600 :file "cli/agents-cli.bclj"} (:toolCallId event)) ^{:line 2601 :file "cli/agents-cli.bclj"} (watch-kv "result" ^{:line 2601 :file "cli/agents-cli.bclj"} (:resultPreview event)) ^{:line 2602 :file "cli/agents-cli.bclj"} (watch-kv "error" ^{:line 2602 :file "cli/agents-cli.bclj"} (:errorCode event)))
    "artifact.published" ^{:line 2605 :file "cli/agents-cli.bclj"} (str prefix "artifact published" ^{:line 2606 :file "cli/agents-cli.bclj"} (watch-kv "label" ^{:line 2606 :file "cli/agents-cli.bclj"} (:label event)) ^{:line 2607 :file "cli/agents-cli.bclj"} (watch-kv "bytes" ^{:line 2607 :file "cli/agents-cli.bclj"} (:bytes event)) ^{:line 2608 :file "cli/agents-cli.bclj"} (watch-kv "media" ^{:line 2608 :file "cli/agents-cli.bclj"} (:mediaType event)))
    "resource.pressure" ^{:line 2611 :file "cli/agents-cli.bclj"} (str prefix "resource " ^{:line 2611 :file "cli/agents-cli.bclj"} (if ^{:line 2611 :file "cli/agents-cli.bclj"} (:advisory event) "advisory" "pressure") ^{:line 2612 :file "cli/agents-cli.bclj"} (watch-kv "name" ^{:line 2612 :file "cli/agents-cli.bclj"} (:resource event)) ^{:line 2613 :file "cli/agents-cli.bclj"} (watch-kv "used" ^{:line 2613 :file "cli/agents-cli.bclj"} (:used event)) ^{:line 2614 :file "cli/agents-cli.bclj"} (watch-kv "limit" ^{:line 2614 :file "cli/agents-cli.bclj"} (:limit event)))
    "run.terminated" ^{:line 2617 :file "cli/agents-cli.bclj"} (str prefix "■ run " ^{:line 2617 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2617 :file "cli/agents-cli.bclj"} (:lifecycle event)) ^{:line 2618 :file "cli/agents-cli.bclj"} (watch-kv "reason" ^{:line 2618 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2618 :file "cli/agents-cli.bclj"} [:reason :code])) ^{:line 2619 :file "cli/agents-cli.bclj"} (watch-kv "detail" ^{:line 2619 :file "cli/agents-cli.bclj"} (get-in event ^{:line 2619 :file "cli/agents-cli.bclj"} [:reason :detail]))))]
  ^{:line 2620 :file "cli/agents-cli.bclj"} (watch-safe-text rendered max-watch-output-codepoints)))

^{:line 2622 :file "cli/agents-cli.bclj"} (defn render-watch-wire-line
  "Project one canonical JSONL envelope for a terminal. TypeScript remains the\n   semantic validator; this display boundary only parses enough shape to render\n   known events and to label malformed or future input visibly." [line]
  ^{:line 2626 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2627 :file "cli/agents-cli.bclj"} (> ^{:line 2627 :file "cli/agents-cli.bclj"} (alength ^{:line 2627 :file "cli/agents-cli.bclj"} (.getBytes ^{:line 2627 :file "cli/agents-cli.bclj"} (str line) java.nio.charset.StandardCharsets/UTF_8)) max-watch-json-line-bytes) "! malformed wire JSONL: line exceeds display bound"
  :else ^{:line 2632 :file "cli/agents-cli.bclj"} (try
  ^{:line 2633 :file "cli/agents-cli.bclj"} (let [event ^{:line 2633 :file "cli/agents-cli.bclj"} (json/parse-string line true)
   version ^{:line 2634 :file "cli/agents-cli.bclj"} (:version event)
   kind ^{:line 2635 :file "cli/agents-cli.bclj"} (:kind event)
   essential ^{:line 2636 :file "cli/agents-cli.bclj"} (:essential event)]
  ^{:line 2637 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2638 :file "cli/agents-cli.bclj"} (not ^{:line 2638 :file "cli/agents-cli.bclj"} (and ^{:line 2638 :file "cli/agents-cli.bclj"} (map? event) ^{:line 2638 :file "cli/agents-cli.bclj"} (string? version) ^{:line 2638 :file "cli/agents-cli.bclj"} (string? kind))) "! malformed wire JSONL: event envelope is not displayable"
  ^{:line 2641 :file "cli/agents-cli.bclj"} (and ^{:line 2641 :file "cli/agents-cli.bclj"} (= wire-watch-version version) ^{:line 2642 :file "cli/agents-cli.bclj"} (contains? wire-watch-kinds kind) ^{:line 2643 :file "cli/agents-cli.bclj"} (= true essential)) ^{:line 2644 :file "cli/agents-cli.bclj"} (render-known-wire-event event)
  ^{:line 2646 :file "cli/agents-cli.bclj"} (and ^{:line 2646 :file "cli/agents-cli.bclj"} (= wire-watch-version version) ^{:line 2646 :file "cli/agents-cli.bclj"} (contains? wire-watch-kinds kind)) "! malformed wire JSONL: known event must be essential"
  ^{:line 2649 :file "cli/agents-cli.bclj"} (= false essential) ^{:line 2650 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2651 :file "cli/agents-cli.bclj"} (str ^{:line 2651 :file "cli/agents-cli.bclj"} (watch-event-prefix event) "○ opaque nonessential" ^{:line 2652 :file "cli/agents-cli.bclj"} (watch-kv "kind" kind) ^{:line 2653 :file "cli/agents-cli.bclj"} (watch-kv "version" version)) max-watch-output-codepoints)
  :else ^{:line 2657 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2658 :file "cli/agents-cli.bclj"} (str "! unsupported essential wire event" ^{:line 2659 :file "cli/agents-cli.bclj"} (watch-kv "kind" kind) ^{:line 2660 :file "cli/agents-cli.bclj"} (watch-kv "version" version)) max-watch-output-codepoints)))
  (catch Exception _
    "! malformed wire JSONL: invalid JSON"))))

^{:line 2665 :file "cli/agents-cli.bclj"} (defn- watch-contained-file [root child-name]
  ^{:line 2668 :file "cli/agents-cli.bclj"} (try
  ^{:line 2669 :file "cli/agents-cli.bclj"} (let [directory ^{:line 2669 :file "cli/agents-cli.bclj"} (.getCanonicalFile ^{:line 2669 :file "cli/agents-cli.bclj"} (io/file root))
   child ^{:line 2670 :file "cli/agents-cli.bclj"} (.getCanonicalFile ^{:line 2670 :file "cli/agents-cli.bclj"} (io/file directory child-name))]
  ^{:line 2671 :file "cli/agents-cli.bclj"} (if ^{:line 2671 :file "cli/agents-cli.bclj"} (= directory ^{:line 2671 :file "cli/agents-cli.bclj"} (.getParentFile child)) ^{:line 2671 :file "cli/agents-cli.bclj"} (do
  child)))
  (catch Exception _
    nil)))

^{:line 2674 :file "cli/agents-cli.bclj"} (defn- watch-file-observation [file]
  ^{:line 2675 :file "cli/agents-cli.bclj"} (let [present? ^{:line 2675 :file "cli/agents-cli.bclj"} (.isFile file)
   bytes ^{:line 2676 :file "cli/agents-cli.bclj"} (if present? ^{:line 2676 :file "cli/agents-cli.bclj"} (do
  ^{:line 2676 :file "cli/agents-cli.bclj"} (.length file)))]
  ^{:line 2677 :file "cli/agents-cli.bclj"} {:path ^{:line 2677 :file "cli/agents-cli.bclj"} (str file) :present? present? :bytes bytes :modified-at ^{:line 2680 :file "cli/agents-cli.bclj"} (if present? ^{:line 2680 :file "cli/agents-cli.bclj"} (do
  ^{:line 2681 :file "cli/agents-cli.bclj"} (str ^{:line 2681 :file "cli/agents-cli.bclj"} (java.time.Instant/ofEpochMilli ^{:line 2681 :file "cli/agents-cli.bclj"} (.lastModified file)))))}))

^{:line 2683 :file "cli/agents-cli.bclj"} (defn watch-plan
  "Resolve the canonical event stream and sparse process/control diagnostic for\n   one exact agent ID. The optional roots arity keeps path/status behavior\n   directly testable without consulting live agent state."
  ([args]
    ^{:line 2687 :file "cli/agents-cli.bclj"} (watch-plan args AGENT-STREAMDIR AGENT-LOGDIR))
  ([args stream-dir control-dir]
    ^{:line 2691 :file "cli/agents-cli.bclj"} (let [[id option & extra] args]
  ^{:line 2692 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2693 :file "cli/agents-cli.bclj"} (or ^{:line 2693 :file "cli/agents-cli.bclj"} (nil? id) ^{:line 2693 :file "cli/agents-cli.bclj"} (seq extra)) ^{:line 2694 :file "cli/agents-cli.bclj"} {:error watch-usage}
  ^{:line 2696 :file "cli/agents-cli.bclj"} (contains? ^{:line 2696 :file "cli/agents-cli.bclj"} #{"--help" "-h" "help"} id) ^{:line 2697 :file "cli/agents-cli.bclj"} {:help watch-usage}
  ^{:line 2699 :file "cli/agents-cli.bclj"} (not ^{:line 2699 :file "cli/agents-cli.bclj"} (valid-control-id? id)) ^{:line 2700 :file "cli/agents-cli.bclj"} {:error ^{:line 2700 :file "cli/agents-cli.bclj"} (str "invalid agent ID; expected " control-id-pattern " and at most " max-control-id-bytes " UTF-8 bytes")}
  ^{:line 2703 :file "cli/agents-cli.bclj"} (and option ^{:line 2703 :file "cli/agents-cli.bclj"} (not= option "--control")) ^{:line 2704 :file "cli/agents-cli.bclj"} {:error ^{:line 2704 :file "cli/agents-cli.bclj"} (str "unknown watch option: " option)}
  :else ^{:line 2707 :file "cli/agents-cli.bclj"} (let [stream-file ^{:line 2707 :file "cli/agents-cli.bclj"} (watch-contained-file stream-dir ^{:line 2707 :file "cli/agents-cli.bclj"} (str "agent-" id ".stream.jsonl"))
   control-file ^{:line 2708 :file "cli/agents-cli.bclj"} (watch-contained-file control-dir ^{:line 2708 :file "cli/agents-cli.bclj"} (str id ".log"))]
  ^{:line 2709 :file "cli/agents-cli.bclj"} (if ^{:line 2709 :file "cli/agents-cli.bclj"} (and stream-file control-file) ^{:line 2711 :file "cli/agents-cli.bclj"} {:id id :mode ^{:line 2712 :file "cli/agents-cli.bclj"} (if ^{:line 2712 :file "cli/agents-cli.bclj"} (= option "--control") :control :stream) :stream ^{:line 2713 :file "cli/agents-cli.bclj"} (watch-file-observation stream-file) :control ^{:line 2714 :file "cli/agents-cli.bclj"} (watch-file-observation control-file)} ^{:line 2710 :file "cli/agents-cli.bclj"} {:error "watch path escapes its configured data root"}))))))

^{:line 2716 :file "cli/agents-cli.bclj"} (defn- watch-status [label data-kind {:keys [path present? bytes modified-at]}]
  ^{:line 2720 :file "cli/agents-cli.bclj"} (watch-safe-text ^{:line 2721 :file "cli/agents-cli.bclj"} (str label ": " ^{:line 2721 :file "cli/agents-cli.bclj"} (watch-display-path path) " — " ^{:line 2722 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2723 :file "cli/agents-cli.bclj"} (not present?) "not present yet"
  ^{:line 2724 :file "cli/agents-cli.bclj"} (zero? bytes) "present but empty"
  :else ^{:line 2725 :file "cli/agents-cli.bclj"} (str data-kind " present (" bytes " bytes, modified " modified-at ")"))) max-watch-output-codepoints))

^{:line 2728 :file "cli/agents-cli.bclj"} (defn watch-status-lines [{:keys [mode stream control]}]
  ^{:line 2729 :file "cli/agents-cli.bclj"} [^{:line 2729 :file "cli/agents-cli.bclj"} (str "watch target: " ^{:line 2729 :file "cli/agents-cli.bclj"} (if ^{:line 2729 :file "cli/agents-cli.bclj"} (= mode :stream) "canonical WireEvent stream" "process/control diagnostics (explicit opt-in)")) ^{:line 2732 :file "cli/agents-cli.bclj"} (watch-status "canonical WireEvent stream" "event data" stream) ^{:line 2733 :file "cli/agents-cli.bclj"} (watch-status "process/control diagnostics" "diagnostic data" control) ^{:line 2734 :file "cli/agents-cli.bclj"} (str "liveness guardrail: process/control logs are sparse diagnostics; " "their silence is not evidence that a worker stalled or died.")])

^{:line 2737 :file "cli/agents-cli.bclj"} (defn- follow-watch-file! [mode path]
  ^{:line 2740 :file "cli/agents-cli.bclj"} (let [tail ^{:line 2740 :file "cli/agents-cli.bclj"} (p/process ^{:line 2740 :file "cli/agents-cli.bclj"} ["tail" "-n" "40" "-F" "--sleep-interval=0.2" "--max-unchanged-stats=1" "--" path] ^{:line 2743 :file "cli/agents-cli.bclj"} {:out :stream :err :inherit})]
  ^{:line 2744 :file "cli/agents-cli.bclj"} (try
  ^{:line 2745 :file "cli/agents-cli.bclj"} (with-open [reader ^{:line 2745 :file "cli/agents-cli.bclj"} (io/reader ^{:line 2745 :file "cli/agents-cli.bclj"} (:out tail))]
  ^{:line 2746 :file "cli/agents-cli.bclj"} (doseq [line ^{:line 2746 :file "cli/agents-cli.bclj"} (line-seq reader)]
  ^{:line 2747 :file "cli/agents-cli.bclj"} (println ^{:line 2747 :file "cli/agents-cli.bclj"} (if ^{:line 2747 :file "cli/agents-cli.bclj"} (= mode :stream) ^{:line 2748 :file "cli/agents-cli.bclj"} (render-watch-wire-line line) ^{:line 2749 :file "cli/agents-cli.bclj"} (watch-safe-text line max-watch-output-codepoints)))
  ^{:line 2750 :file "cli/agents-cli.bclj"} (flush)))
  ^{:line 2751 :file "cli/agents-cli.bclj"} (let [result ^{:line 2751 :file "cli/agents-cli.bclj"} (deref tail)]
  ^{:line 2752 :file "cli/agents-cli.bclj"} (if ^{:line 2752 :file "cli/agents-cli.bclj"} (not ^{:line 2752 :file "cli/agents-cli.bclj"} (zero? ^{:line 2752 :file "cli/agents-cli.bclj"} (:exit result))) ^{:line 2752 :file "cli/agents-cli.bclj"} (do
  ^{:line 2753 :file "cli/agents-cli.bclj"} (System/exit ^{:line 2753 :file "cli/agents-cli.bclj"} (:exit result)))))
  (finally
    ^{:line 2755 :file "cli/agents-cli.bclj"} (if ^{:line 2755 :file "cli/agents-cli.bclj"} (.isAlive ^Process ^{:line 2755 :file "cli/agents-cli.bclj"} (:proc tail)) ^{:line 2755 :file "cli/agents-cli.bclj"} (do
  ^{:line 2756 :file "cli/agents-cli.bclj"} (p/destroy-tree tail)))))))

^{:line 2758 :file "cli/agents-cli.bclj"} (defn cmd-watch! [args]
  ^{:line 2759 :file "cli/agents-cli.bclj"} (let [{:keys [error help mode stream control] :as plan} ^{:line 2759 :file "cli/agents-cli.bclj"} (watch-plan args)]
  ^{:line 2760 :file "cli/agents-cli.bclj"} (cond
  help ^{:line 2761 :file "cli/agents-cli.bclj"} (println "usage:" help)
  error ^{:line 2763 :file "cli/agents-cli.bclj"} (do
  ^{:line 2764 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2765 :file "cli/agents-cli.bclj"} (println ^{:line 2765 :file "cli/agents-cli.bclj"} (red error))
  ^{:line 2766 :file "cli/agents-cli.bclj"} (println ^{:line 2766 :file "cli/agents-cli.bclj"} (red "usage:") watch-usage))
  ^{:line 2767 :file "cli/agents-cli.bclj"} (System/exit 2))
  :else ^{:line 2769 :file "cli/agents-cli.bclj"} (let [target ^{:line 2769 :file "cli/agents-cli.bclj"} (if ^{:line 2769 :file "cli/agents-cli.bclj"} (= mode :control) control stream)]
  ^{:line 2770 :file "cli/agents-cli.bclj"} (doseq [line ^{:line 2770 :file "cli/agents-cli.bclj"} (watch-status-lines plan)]
  ^{:line 2770 :file "cli/agents-cli.bclj"} (println line))
  ^{:line 2771 :file "cli/agents-cli.bclj"} (if ^{:line 2771 :file "cli/agents-cli.bclj"} (not ^{:line 2771 :file "cli/agents-cli.bclj"} (:present? target)) ^{:line 2771 :file "cli/agents-cli.bclj"} (do
  ^{:line 2772 :file "cli/agents-cli.bclj"} (println ^{:line 2772 :file "cli/agents-cli.bclj"} (ylw "waiting for selected file to appear; absence alone is not a terminal verdict."))))
  ^{:line 2773 :file "cli/agents-cli.bclj"} (if ^{:line 2773 :file "cli/agents-cli.bclj"} (= mode :stream) ^{:line 2773 :file "cli/agents-cli.bclj"} (do
  ^{:line 2774 :file "cli/agents-cli.bclj"} (println "explicit diagnostics mode:" ^{:line 2774 :file "cli/agents-cli.bclj"} (cyn ^{:line 2774 :file "cli/agents-cli.bclj"} (str "north agent watch " ^{:line 2774 :file "cli/agents-cli.bclj"} (:id plan) " --control")))))
  ^{:line 2775 :file "cli/agents-cli.bclj"} (echo-cmd "tail -n 40 -F --sleep-interval=0.2 --max-unchanged-stats=1 --" ^{:line 2776 :file "cli/agents-cli.bclj"} (watch-display-path ^{:line 2776 :file "cli/agents-cli.bclj"} (:path target)) ^{:line 2777 :file "cli/agents-cli.bclj"} (if ^{:line 2777 :file "cli/agents-cli.bclj"} (= mode :stream) "| WireEvent projection" "| sanitized diagnostics"))
  ^{:line 2778 :file "cli/agents-cli.bclj"} (follow-watch-file! mode ^{:line 2778 :file "cli/agents-cli.bclj"} (:path target))))))

^{:line 2780 :file "cli/agents-cli.bclj"} (defn cmd-tell-agent [args]
  ^{:line 2781 :file "cli/agents-cli.bclj"} (north.topology-authority/require-coordination! "msg")
  ^{:line 2782 :file "cli/agents-cli.bclj"} (let [rest0 ^{:line 2782 :file "cli/agents-cli.bclj"} (vec ^{:line 2782 :file "cli/agents-cli.bclj"} (remove ^{:line 2782 :file "cli/agents-cli.bclj"} (fn [arg] ^{:line 2783 :file "cli/agents-cli.bclj"} (contains? ^{:line 2783 :file "cli/agents-cli.bclj"} #{"--dry-run"} arg)) args))
   dry? ^{:line 2785 :file "cli/agents-cli.bclj"} (some ^{:line 2785 :file "cli/agents-cli.bclj"} #{"--dry-run"} args)
   from-idx ^{:line 2786 :file "cli/agents-cli.bclj"} (.indexOf rest0 "--from")
   from ^{:line 2787 :file "cli/agents-cli.bclj"} (if ^{:line 2787 :file "cli/agents-cli.bclj"} (>= from-idx 0) ^{:line 2787 :file "cli/agents-cli.bclj"} (nth rest0 ^{:line 2787 :file "cli/agents-cli.bclj"} (inc from-idx) nil) ^{:line 2788 :file "cli/agents-cli.bclj"} (or ^{:line 2788 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_AGENT_ID") "north-cli"))
   pos ^{:line 2789 :file "cli/agents-cli.bclj"} (if ^{:line 2789 :file "cli/agents-cli.bclj"} (>= from-idx 0) ^{:line 2790 :file "cli/agents-cli.bclj"} (keep-indexed ^{:line 2790 :file "cli/agents-cli.bclj"} (fn [%1 %2] ^{:line 2790 :file "cli/agents-cli.bclj"} (if ^{:line 2790 :file "cli/agents-cli.bclj"} (not ^{:line 2790 :file "cli/agents-cli.bclj"} (^{:line 2790 :file "cli/agents-cli.bclj"} #{from-idx ^{:line 2790 :file "cli/agents-cli.bclj"} (inc from-idx)} %1)) ^{:line 2790 :file "cli/agents-cli.bclj"} (do
  %2))) rest0) rest0)
   [id msg] pos]
  ^{:line 2793 :file "cli/agents-cli.bclj"} (if ^{:line 2793 :file "cli/agents-cli.bclj"} (or ^{:line 2793 :file "cli/agents-cli.bclj"} (nil? id) ^{:line 2793 :file "cli/agents-cli.bclj"} (nil? msg)) ^{:line 2794 :file "cli/agents-cli.bclj"} (do
  ^{:line 2795 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2796 :file "cli/agents-cli.bclj"} (println ^{:line 2796 :file "cli/agents-cli.bclj"} (red "usage:") "north agent send <agent-id> \"<msg>\" [--from <me>]"))
  2) ^{:line 2798 :file "cli/agents-cli.bclj"} (let [argv ^{:line 2798 :file "cli/agents-cli.bclj"} ["bb" ^{:line 2798 :file "cli/agents-cli.bclj"} (str NORTH "/cli/msg-cli.clj") PORT "send" from id "msg" msg]]
  ^{:line 2799 :file "cli/agents-cli.bclj"} (echo-cmd ^{:line 2799 :file "cli/agents-cli.bclj"} (str/join " " argv))
  ^{:line 2800 :file "cli/agents-cli.bclj"} (if dry? ^{:line 2801 :file "cli/agents-cli.bclj"} (do
  ^{:line 2802 :file "cli/agents-cli.bclj"} (println ^{:line 2802 :file "cli/agents-cli.bclj"} (ylw "[dry-run]") "not sent; target capability and liveness were not checked.")
  0) ^{:line 2804 :file "cli/agents-cli.bclj"} (let [r ^{:line 2804 :file "cli/agents-cli.bclj"} (run argv :timeout msg-admission-timeout-ms)]
  ^{:line 2805 :file "cli/agents-cli.bclj"} (if ^{:line 2805 :file "cli/agents-cli.bclj"} (:ok r) ^{:line 2806 :file "cli/agents-cli.bclj"} (do
  ^{:line 2807 :file "cli/agents-cli.bclj"} (println ^{:line 2807 :file "cli/agents-cli.bclj"} (grn ^{:line 2807 :file "cli/agents-cli.bclj"} (or ^{:line 2807 :file "cli/agents-cli.bclj"} (known ^{:line 2807 :file "cli/agents-cli.bclj"} (:out r)) "queued for live injection")))
  0) ^{:line 2809 :file "cli/agents-cli.bclj"} (do
  ^{:line 2810 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2811 :file "cli/agents-cli.bclj"} (println ^{:line 2811 :file "cli/agents-cli.bclj"} (red ^{:line 2811 :file "cli/agents-cli.bclj"} (or ^{:line 2811 :file "cli/agents-cli.bclj"} (known ^{:line 2811 :file "cli/agents-cli.bclj"} (:err r)) ^{:line 2812 :file "cli/agents-cli.bclj"} (known ^{:line 2812 :file "cli/agents-cli.bclj"} (:out r)) "msg admission unavailable"))))
  ^{:line 2814 :file "cli/agents-cli.bclj"} (let [status ^{:line 2814 :file "cli/agents-cli.bclj"} (:exit r)]
  ^{:line 2815 :file "cli/agents-cli.bclj"} (if ^{:line 2815 :file "cli/agents-cli.bclj"} (and ^{:line 2815 :file "cli/agents-cli.bclj"} (integer? status) ^{:line 2815 :file "cli/agents-cli.bclj"} (pos? status)) status 2))))))))))

^{:line 2820 :file "cli/agents-cli.bclj"} (def max-safe-fence-epoch 9007199254740991)

^{:line 2822 :file "cli/agents-cli.bclj"} (defn- decode-fence-utf8! [bytes path]
  ^{:line 2825 :file "cli/agents-cli.bclj"} (try
  ^{:line 2826 :file "cli/agents-cli.bclj"} (let [decoder ^{:line 2826 :file "cli/agents-cli.bclj"} (doto ^{:line 2826 :file "cli/agents-cli.bclj"} (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
  ^{:line 2827 :file "cli/agents-cli.bclj"} (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
  ^{:line 2828 :file "cli/agents-cli.bclj"} (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
  ^{:line 2829 :file "cli/agents-cli.bclj"} (str ^{:line 2829 :file "cli/agents-cli.bclj"} (.decode decoder ^{:line 2829 :file "cli/agents-cli.bclj"} (java.nio.ByteBuffer/wrap bytes))))
  (catch java.nio.charset.CharacterCodingException error
    ^{:line 2831 :file "cli/agents-cli.bclj"} (throw ^{:line 2831 :file "cli/agents-cli.bclj"} (ex-info "saved liveness fence is not valid UTF-8" ^{:line 2832 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path path} error)))))

^{:line 2835 :file "cli/agents-cli.bclj"} (defn- saved-presence-fence-json!
  ([bare]
    ^{:line 2837 :file "cli/agents-cli.bclj"} (saved-presence-fence-json! bare ^{:line 2839 :file "cli/agents-cli.bclj"} (or ^{:line 2839 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_AGENT_LOGS_DIR") ^{:line 2840 :file "cli/agents-cli.bclj"} (str HOME "/.local/state/north/agents"))))
  ([bare directory]
    ^{:line 2843 :file "cli/agents-cli.bclj"} (if ^{:line 2843 :file "cli/agents-cli.bclj"} (not ^{:line 2843 :file "cli/agents-cli.bclj"} (valid-control-id? bare)) ^{:line 2843 :file "cli/agents-cli.bclj"} (do
  ^{:line 2844 :file "cli/agents-cli.bclj"} (throw ^{:line 2844 :file "cli/agents-cli.bclj"} (ex-info "north agent goal requires a safe agent id" ^{:line 2845 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :agent bare}))))
    ^{:line 2846 :file "cli/agents-cli.bclj"} (let [directory-file ^{:line 2846 :file "cli/agents-cli.bclj"} (.getCanonicalFile ^{:line 2846 :file "cli/agents-cli.bclj"} (io/file directory))
   file ^{:line 2847 :file "cli/agents-cli.bclj"} (io/file directory-file ^{:line 2847 :file "cli/agents-cli.bclj"} (str bare ".liveness-fence.json"))
   path ^{:line 2848 :file "cli/agents-cli.bclj"} (.toPath file)
   no-follow ^{:line 2849 :file "cli/agents-cli.bclj"} (into-array java.nio.file.LinkOption ^{:line 2850 :file "cli/agents-cli.bclj"} [java.nio.file.LinkOption/NOFOLLOW_LINKS])]
  ^{:line 2851 :file "cli/agents-cli.bclj"} (if ^{:line 2851 :file "cli/agents-cli.bclj"} (or ^{:line 2851 :file "cli/agents-cli.bclj"} (java.nio.file.Files/isSymbolicLink path) ^{:line 2852 :file "cli/agents-cli.bclj"} (not ^{:line 2852 :file "cli/agents-cli.bclj"} (java.nio.file.Files/isRegularFile path no-follow))) ^{:line 2851 :file "cli/agents-cli.bclj"} (do
  ^{:line 2853 :file "cli/agents-cli.bclj"} (throw ^{:line 2853 :file "cli/agents-cli.bclj"} (ex-info "north agent goal requires a regular saved liveness fence" ^{:line 2854 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path ^{:line 2855 :file "cli/agents-cli.bclj"} (.getPath file)}))))
  ^{:line 2856 :file "cli/agents-cli.bclj"} (let [permissions ^{:line 2856 :file "cli/agents-cli.bclj"} (java.nio.file.Files/getPosixFilePermissions path no-follow)
   expected-permissions ^{:line 2857 :file "cli/agents-cli.bclj"} #{java.nio.file.attribute.PosixFilePermission/OWNER_READ java.nio.file.attribute.PosixFilePermission/OWNER_WRITE}]
  ^{:line 2859 :file "cli/agents-cli.bclj"} (if ^{:line 2859 :file "cli/agents-cli.bclj"} (not ^{:line 2859 :file "cli/agents-cli.bclj"} (= expected-permissions ^{:line 2859 :file "cli/agents-cli.bclj"} (set permissions))) ^{:line 2859 :file "cli/agents-cli.bclj"} (do
  ^{:line 2860 :file "cli/agents-cli.bclj"} (throw ^{:line 2860 :file "cli/agents-cli.bclj"} (ex-info "saved liveness fence must have mode 0600" ^{:line 2861 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path ^{:line 2862 :file "cli/agents-cli.bclj"} (.getPath file)})))))
  ^{:line 2863 :file "cli/agents-cli.bclj"} (let [bytes ^{:line 2863 :file "cli/agents-cli.bclj"} (java.nio.file.Files/readAllBytes path)]
  ^{:line 2864 :file "cli/agents-cli.bclj"} (if ^{:line 2864 :file "cli/agents-cli.bclj"} (not ^{:line 2864 :file "cli/agents-cli.bclj"} (<= 1 ^{:line 2864 :file "cli/agents-cli.bclj"} (alength bytes) 512)) ^{:line 2864 :file "cli/agents-cli.bclj"} (do
  ^{:line 2865 :file "cli/agents-cli.bclj"} (throw ^{:line 2865 :file "cli/agents-cli.bclj"} (ex-info "saved liveness fence has an invalid size" ^{:line 2866 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path ^{:line 2867 :file "cli/agents-cli.bclj"} (.getPath file)}))))
  ^{:line 2868 :file "cli/agents-cli.bclj"} (let [raw ^{:line 2868 :file "cli/agents-cli.bclj"} (decode-fence-utf8! bytes ^{:line 2868 :file "cli/agents-cli.bclj"} (.getPath file))
   parsed ^{:line 2869 :file "cli/agents-cli.bclj"} (try
  ^{:line 2870 :file "cli/agents-cli.bclj"} (json/parse-string raw)
  (catch Exception error
    ^{:line 2872 :file "cli/agents-cli.bclj"} (throw ^{:line 2872 :file "cli/agents-cli.bclj"} (ex-info "saved liveness fence is not valid JSON" ^{:line 2873 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path ^{:line 2874 :file "cli/agents-cli.bclj"} (.getPath file)} error))))
   epoch ^{:line 2876 :file "cli/agents-cli.bclj"} (get parsed "epoch")
   expected ^{:line 2877 :file "cli/agents-cli.bclj"} (array-map "resource" ^{:line 2878 :file "cli/agents-cli.bclj"} (str "session:" bare) "holder" bare "epoch" epoch)]
  ^{:line 2881 :file "cli/agents-cli.bclj"} (if ^{:line 2881 :file "cli/agents-cli.bclj"} (not ^{:line 2882 :file "cli/agents-cli.bclj"} (and ^{:line 2882 :file "cli/agents-cli.bclj"} (map? parsed) ^{:line 2883 :file "cli/agents-cli.bclj"} (= ^{:line 2883 :file "cli/agents-cli.bclj"} #{"resource" "holder" "epoch"} ^{:line 2883 :file "cli/agents-cli.bclj"} (set ^{:line 2883 :file "cli/agents-cli.bclj"} (keys parsed))) ^{:line 2884 :file "cli/agents-cli.bclj"} (= ^{:line 2884 :file "cli/agents-cli.bclj"} (get expected "resource") ^{:line 2884 :file "cli/agents-cli.bclj"} (get parsed "resource")) ^{:line 2885 :file "cli/agents-cli.bclj"} (= bare ^{:line 2885 :file "cli/agents-cli.bclj"} (get parsed "holder")) ^{:line 2886 :file "cli/agents-cli.bclj"} (integer? epoch) ^{:line 2887 :file "cli/agents-cli.bclj"} (<= 1 epoch max-safe-fence-epoch) ^{:line 2888 :file "cli/agents-cli.bclj"} (= ^{:line 2888 :file "cli/agents-cli.bclj"} (str ^{:line 2888 :file "cli/agents-cli.bclj"} (json/generate-string expected) "\n") raw))) ^{:line 2881 :file "cli/agents-cli.bclj"} (do
  ^{:line 2889 :file "cli/agents-cli.bclj"} (throw ^{:line 2889 :file "cli/agents-cli.bclj"} (ex-info "saved liveness fence does not exactly match the agent" ^{:line 2890 :file "cli/agents-cli.bclj"} {:type :invalid-saved-liveness-fence :path ^{:line 2891 :file "cli/agents-cli.bclj"} (.getPath file) :agent bare}))))
  ^{:line 2893 :file "cli/agents-cli.bclj"} (json/generate-string expected))))))

^{:line 2895 :file "cli/agents-cli.bclj"} (defn cmd-goal! [[id goal & _]]
  ^{:line 2896 :file "cli/agents-cli.bclj"} (north.topology-authority/require-coordination! "goal")
  ^{:line 2897 :file "cli/agents-cli.bclj"} (if ^{:line 2897 :file "cli/agents-cli.bclj"} (or ^{:line 2897 :file "cli/agents-cli.bclj"} (nil? id) ^{:line 2897 :file "cli/agents-cli.bclj"} (nil? goal)) ^{:line 2898 :file "cli/agents-cli.bclj"} (println ^{:line 2898 :file "cli/agents-cli.bclj"} (red "usage:") "north agent goal <agent-id> \"<new-goal>\"") ^{:line 2899 :file "cli/agents-cli.bclj"} (let [subj ^{:line 2899 :file "cli/agents-cli.bclj"} (str "agent:" ^{:line 2899 :file "cli/agents-cli.bclj"} (str/replace-first id #"^@?(agent:)?" ""))
   bare ^{:line 2900 :file "cli/agents-cli.bclj"} (subs subj ^{:line 2900 :file "cli/agents-cli.bclj"} (count "agent:"))
   facts ^{:line 2901 :file "cli/agents-cli.bclj"} (assoc ^{:line 2901 :file "cli/agents-cli.bclj"} (or ^{:line 2901 :file "cli/agents-cli.bclj"} (agent-facts-one bare) ^{:line 2901 :file "cli/agents-cli.bclj"} {}) "goal" goal)
   dn ^{:line 2902 :file "cli/agents-cli.bclj"} (render-display-name bare facts)
   update ^{:line 2903 :file "cli/agents-cli.bclj"} (json/generate-string ^{:line 2903 :file "cli/agents-cli.bclj"} {"goal" goal "display_name" dn})
   presence-fence ^{:line 2904 :file "cli/agents-cli.bclj"} (saved-presence-fence-json! bare)
   result ^{:line 2905 :file "cli/agents-cli.bclj"} (run ^{:line 2905 :file "cli/agents-cli.bclj"} ["bb" ^{:line 2905 :file "cli/agents-cli.bclj"} (str NORTH "/cli/agent-fact-internal.clj") PORT "goal" subj update "" "" "" "" "" presence-fence] :timeout 10000)]
  ^{:line 2909 :file "cli/agents-cli.bclj"} (if ^{:line 2909 :file "cli/agents-cli.bclj"} (:ok result) ^{:line 2910 :file "cli/agents-cli.bclj"} (do
  ^{:line 2910 :file "cli/agents-cli.bclj"} (println ^{:line 2910 :file "cli/agents-cli.bclj"} (grn "goal set") ^{:line 2910 :file "cli/agents-cli.bclj"} (bold bare))
  ^{:line 2911 :file "cli/agents-cli.bclj"} (println "  " dn)) ^{:line 2912 :file "cli/agents-cli.bclj"} (do
  ^{:line 2912 :file "cli/agents-cli.bclj"} (println ^{:line 2912 :file "cli/agents-cli.bclj"} (red "goal update failed"))
  ^{:line 2913 :file "cli/agents-cli.bclj"} (println ^{:line 2913 :file "cli/agents-cli.bclj"} (str/trim ^{:line 2913 :file "cli/agents-cli.bclj"} (str ^{:line 2913 :file "cli/agents-cli.bclj"} (:out result) ^{:line 2913 :file "cli/agents-cli.bclj"} (:err result)))))))))

^{:line 2915 :file "cli/agents-cli.bclj"} (def cmd-agents cmd-agents!)

^{:line 2916 :file "cli/agents-cli.bclj"} (def cmd-bind-child-referent cmd-bind-child-referent!)

^{:line 2917 :file "cli/agents-cli.bclj"} (def cmd-delegate cmd-delegate!)

^{:line 2918 :file "cli/agents-cli.bclj"} (def cmd-watch cmd-watch!)

^{:line 2919 :file "cli/agents-cli.bclj"} (def cmd-goal cmd-goal!)

^{:line 2922 :file "cli/agents-cli.bclj"} (if ^{:line 2922 :file "cli/agents-cli.bclj"} (not ^{:line 2922 :file "cli/agents-cli.bclj"} (or ^{:line 2922 :file "cli/agents-cli.bclj"} (= ^{:line 2922 :file "cli/agents-cli.bclj"} (System/getenv "NORTH_AGENTS_LIB") "1") ^{:line 2923 :file "cli/agents-cli.bclj"} (= ^{:line 2923 :file "cli/agents-cli.bclj"} (System/getProperty "north.agents.lib") "1"))) ^{:line 2922 :file "cli/agents-cli.bclj"} (do
  ^{:line 2924 :file "cli/agents-cli.bclj"} (let [[cmd & args] *command-line-args*]
  ^{:line 2925 :file "cli/agents-cli.bclj"} (try
  ^{:line 2926 :file "cli/agents-cli.bclj"} (case cmd
    "agents" ^{:line 2927 :file "cli/agents-cli.bclj"} (cmd-agents args)
    "templates" ^{:line 2928 :file "cli/agents-cli.bclj"} (cmd-templates args)
    "spawn" ^{:line 2931 :file "cli/agents-cli.bclj"} (cond
  ^{:line 2932 :file "cli/agents-cli.bclj"} (some ^{:line 2932 :file "cli/agents-cli.bclj"} #{"--doctor"} args) ^{:line 2933 :file "cli/agents-cli.bclj"} (do
  ^{:line 2933 :file "cli/agents-cli.bclj"} (load-file ^{:line 2933 :file "cli/agents-cli.bclj"} (str NORTH "/cli/spawn-doctor.clj"))
  ^{:line 2934 :file "cli/agents-cli.bclj"} (let [status ^{:line 2934 :file "cli/agents-cli.bclj"} (^{:line 2934 :file "cli/agents-cli.bclj"} (resolve 'north.spawn-doctor/run!) args)]
  ^{:line 2935 :file "cli/agents-cli.bclj"} (if ^{:line 2935 :file "cli/agents-cli.bclj"} (pos? status) ^{:line 2935 :file "cli/agents-cli.bclj"} (do
  ^{:line 2935 :file "cli/agents-cli.bclj"} (System/exit status)))))
  ^{:line 2937 :file "cli/agents-cli.bclj"} (and ^{:line 2937 :file "cli/agents-cli.bclj"} (= 1 ^{:line 2937 :file "cli/agents-cli.bclj"} (count args)) ^{:line 2937 :file "cli/agents-cli.bclj"} (contains? ^{:line 2937 :file "cli/agents-cli.bclj"} #{"--help" "-h" "help"} ^{:line 2937 :file "cli/agents-cli.bclj"} (first args))) ^{:line 2938 :file "cli/agents-cli.bclj"} (cmd-spawn-help)
  :else ^{:line 2940 :file "cli/agents-cli.bclj"} (cmd-spawn args))
    "delegate" ^{:line 2941 :file "cli/agents-cli.bclj"} (cmd-delegate args)
    "bind-child-referent" ^{:line 2942 :file "cli/agents-cli.bclj"} (cmd-bind-child-referent args)
    "watch" ^{:line 2943 :file "cli/agents-cli.bclj"} (cmd-watch args)
    "msg" ^{:line 2944 :file "cli/agents-cli.bclj"} (let [status ^{:line 2944 :file "cli/agents-cli.bclj"} (cmd-tell-agent args)]
  ^{:line 2945 :file "cli/agents-cli.bclj"} (if ^{:line 2945 :file "cli/agents-cli.bclj"} (pos? status) ^{:line 2945 :file "cli/agents-cli.bclj"} (do
  ^{:line 2945 :file "cli/agents-cli.bclj"} (System/exit status))))
    "goal" ^{:line 2946 :file "cli/agents-cli.bclj"} (cmd-goal args)
    ^{:line 2947 :file "cli/agents-cli.bclj"} (do
  ^{:line 2947 :file "cli/agents-cli.bclj"} (println "usage: north {agents|templates|spawn|delegate|watch|msg|goal} ...")
  ^{:line 2948 :file "cli/agents-cli.bclj"} (System/exit 1)))
  (catch clojure.lang.ExceptionInfo error
    ^{:line 2950 :file "cli/agents-cli.bclj"} (if ^{:line 2950 :file "cli/agents-cli.bclj"} (or ^{:line 2950 :file "cli/agents-cli.bclj"} (north.topology-authority/denial? error) ^{:line 2951 :file "cli/agents-cli.bclj"} (:usage ^{:line 2951 :file "cli/agents-cli.bclj"} (ex-data error))) ^{:line 2952 :file "cli/agents-cli.bclj"} (do
  ^{:line 2952 :file "cli/agents-cli.bclj"} (binding [*out* *err*]
  ^{:line 2952 :file "cli/agents-cli.bclj"} (println ^{:line 2952 :file "cli/agents-cli.bclj"} (red ^{:line 2952 :file "cli/agents-cli.bclj"} (.getMessage error))))
  ^{:line 2953 :file "cli/agents-cli.bclj"} (System/exit ^{:line 2953 :file "cli/agents-cli.bclj"} (if ^{:line 2953 :file "cli/agents-cli.bclj"} (:usage ^{:line 2953 :file "cli/agents-cli.bclj"} (ex-data error)) 2 1))) ^{:line 2954 :file "cli/agents-cli.bclj"} (throw error)))))))
