(ns beagle.user
  (:require [babashka.process :as p]
            [clojure.string :as str]
            [clojure.set :as set]
            [clojure.java.io :as io]
            [clojure.walk :as walk]
            [cheshire.core :as json]))

(def HOME (System/getenv "HOME"))

(def NORTH (or (System/getenv "NORTH_HOME") (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str)))

(def NORTH-CLI (or (System/getenv "NORTH_BIN") (str NORTH "/bin/north")))

(def AGENT-MACHINERY (str NORTH "/agent-machinery"))

(def AGENT-RUNTIME (or (System/getenv "NORTH_AGENT_RUNTIME_HOME") (str NORTH "/agent-runtime/orchestration")))

(def AGENT-LOGDIR (str HOME "/.local/state/north/agents"))

(def AGENT-STREAMDIR (or (System/getenv "NORTH_STREAM_DIR") (str HOME "/code/agent-data")))

(def ORCHESTRATION-STAFFING (or (System/getenv "ORCHESTRATION_STAFFING_CATALOG") (str AGENT-MACHINERY "/staffing/catalog.json")))

(def PORT (or (System/getenv "NORTH_PORT") "7977"))

(def ROSTER-CONTRACT-VERSION "north:agent-roster:v1")

(def CODEX-CENSUS-CLI (str NORTH "/sdk/src/codex-census-cli.ts"))

(def STRUGGLE-POLICY-CLI (str NORTH "/sdk/src/struggle.ts"))

(def PROVIDER-CAPABILITY-ADMISSION-CLI (str NORTH "/sdk/src/provider-capability-admission-cli.ts"))

(def ROUTING-ECONOMICS-PREFLIGHT-CLI (str NORTH "/sdk/src/routing-economics-preflight-cli.ts"))

(def DELEGATION-RUN-DESIGN-TRANSPORT (or (System/getenv "NORTH_DELEGATION_RUN_DESIGN_TRANSPORT") (str NORTH "/sdk/src/providers/delegation-run-design-transport.ts")))

(def POLICY-BUN (or (System/getenv "NORTH_POLICY_BUN") "bun"))

(def PROVIDER-CAPABILITY-ADMISSION-SCHEMA "north:provider-capability-admission:v1")

(def msg-admission-timeout-ms 30000)

(load-file (str NORTH "/cli/spawn-process.clj"))

(load-file (str NORTH "/cli/coord.clj"))

(load-file (str NORTH "/cli/message-routing.clj"))

(load-file (str NORTH "/cli/topology-authority.clj"))

(load-file (str NORTH "/cli/managed-child-env.clj"))

(load-file (str NORTH "/cli/orchestration-staffing.clj"))

(def color? (and (nil? (System/getenv "NO_COLOR")) (some? (System/console))))

(defn- c [code s]
  (if color? (str "\u001b[" code "m" s "\u001b[0m") (str s)))

(defn dim [s]
  (c "2" s))

(defn bold [s]
  (c "1" s))

(defn grn [s]
  (c "32" s))

(defn red [s]
  (c "31" s))

(defn ylw [s]
  (c "33" s))

(defn cyn [s]
  (c "36" s))

(defn run [argv & $beagle$rest$host]
  (let [{:keys [timeout in env] :or {timeout 4000}} $beagle$rest$host]
  (try
  (let [proc (p/process argv (cond-> {:out :string :err :string} in (assoc :in in) env (assoc :env env)))
   res (deref proc timeout ::timeout)]
  (if (= res ::timeout) (do
  (p/destroy-tree proc)
  {:timeout true :ok false}) {:out (or (:out res) "") :err (or (:err res) "") :exit (:exit res) :ok (zero? (:exit res))}))
  (catch Exception e
    {:error (or (not-empty (str (.getMessage e))) (.getName (class e))) :ok false}))))

(defn echo-cmd [& $beagle$rest$host]
  (let [parts (vec $beagle$rest$host)]
  (println (dim (str "» " (str/join " " parts))))))

(defn resolve-struggle-policy! [topology]
  (let [result (run [POLICY-BUN "run" STRUGGLE-POLICY-CLI "policy" topology])
   raw (str/trim (or (:out result) ""))
   parsed (try
  (json/parse-string raw true)
  (catch Exception _
    nil))]
  (if (not (and (:ok result) (map? parsed) (= topology (:topology parsed)) (string? (:version parsed)) (every? pos-int? (map parsed [:errorStreak :loopRepeat :loopWindow :noProgressTurns])))) (do
  (binding [*out* *err*]
  (println (red (or (not-empty (str/trim (or (:err result) ""))) "could not resolve struggle detector policy"))))
  (System/exit 1)))
  (assoc parsed :canonical raw)))

(defn- require-pinned-provider-capabilities!
  "Run the SDK's exact pinned-provider gate before delegate referent, identity,\n   account, or provider state can be touched. Auto remains intentionally open:\n   execution may select any capability-compatible provider." [provider target capabilities]
  (if (and provider (not= provider "auto")) (do
  (let [argv (cond-> [POLICY-BUN "run" PROVIDER-CAPABILITY-ADMISSION-CLI provider (json/generate-string capabilities)] target (conj target))
   result (run argv)
   raw (str/trim (or (:out result) ""))
   parsed (try
  (json/parse-string raw true)
  (catch Exception _
    nil))
   expected-base (cond-> #{:schema :provider :capabilities :status} target (conj :requestedTarget))
   supported? (and (:ok result) (map? parsed) (= expected-base (set (keys parsed))) (= PROVIDER-CAPABILITY-ADMISSION-SCHEMA (:schema parsed)) (= "supported" (:status parsed)) (= provider (:provider parsed)) (= capabilities (:capabilities parsed)) (= target (:requestedTarget parsed)))
   unsupported-fields (into expected-base #{:code :processOutcome :reason :retrySafeBeforeAcceptance})
   unsupported? (and (= 3 (:exit result)) (map? parsed) (= unsupported-fields (set (keys parsed))) (= PROVIDER-CAPABILITY-ADMISSION-SCHEMA (:schema parsed)) (= "unsupported" (:status parsed)) (= provider (:provider parsed)) (= capabilities (:capabilities parsed)) (= target (:requestedTarget parsed)) (= true (:retrySafeBeforeAcceptance parsed)) (every? (fn [%1] (and (string? %1) (not (str/blank? %1)))) (map parsed [:code :processOutcome :reason])))]
  (cond
  supported? nil
  unsupported? (do
  (binding [*out* *err*]
  (println (red (str "provider capability admission rejected before side effects: " (:reason parsed)))))
  (println raw)
  (System/exit 1))
  :else (do
  (binding [*out* *err*]
  (println (red (or (not-empty (str/trim (or (:err result) ""))) "provider capability admission unavailable"))))
  (System/exit 1)))))))

(defn orchestration-catalog []
  (let [f (io/file ORCHESTRATION-STAFFING)]
  (if (.isFile f) (do
  (walk/keywordize-keys (north.orchestration-staffing/load-catalog (.getPath f)))))))

(defn orchestration-routing []
  (let [bind__12 (orchestration-catalog)]
  (if bind__12 (let [{:keys [presets defaults]} bind__12]
  (do
  (into {} (map (fn [r] (let [name (:name r)]
  [name (-> (merge defaults r) (assoc :role name :orchestration-preset true :composition {:kind "template" :id name :overrides []}))]))) presets))))))

(defn orchestration-templates []
  (let [bind__13 (orchestration-catalog)]
  (if bind__13 (let [{:keys [presets defaults]} bind__13]
  (do
  (mapv (fn [%1] (merge defaults %1)) presets))))))

(defn cmd-templates [args]
  (if (some #{"--help" "-h" "help"} args) (do
  (println "north agent templates — inspect Orchestration's reusable stock templates")
  (println)
  (println "Usage:")
  (println "  north agent templates             compact template catalog")
  (println "  north agent templates --verbose   include each template's selection boundary")
  (System/exit 0)))
  (let [unknown (first (remove (fn [arg] (contains? #{"--verbose"} arg)) args))]
  (if unknown (do
  (binding [*out* *err*]
  (println (red (str "unknown templates option: " unknown)))
  (println "usage: north agent templates [--verbose]"))
  (System/exit 2))))
  (let [verbose? (some #{"--verbose"} args)
   templates (orchestration-templates)]
  (if (seq templates) (do
  (println (bold "AGENT MACHINERY STOCK TEMPLATES — reusable starting points, not limits"))
  (println (dim "Selection ladder: exact template → justified axis override → bespoke composition."))
  (println (dim "Machine payloads retain composition.kind=template; this view uses the human word template."))
  (doseq [{:keys [name tagline taskGrade capabilityFloor serviceClass deliberation topology posture capabilities description]} templates]
  (println)
  (println (bold name) "—" tagline)
  (println (dim (str "  grade " taskGrade " · " capabilityFloor "/" serviceClass " · " deliberation " · " topology " · " posture)))
  (println (dim (str "  capabilities " (str/join " " capabilities))))
  (if verbose? (do
  (println (str "  " description)))))) (do
  (binding [*out* *err*]
  (println (red (str "Delegation run-composition catalog unavailable: " ORCHESTRATION-STAFFING))))
  (System/exit 1)))))

(defn dry-resolved-route [provider tier explicit-model reasoning]
  (if (and provider (not= provider "auto")) (do
  (try
  (let [entry (get-in (json/parse-string (slurp (io/file AGENT-RUNTIME "providers" (str provider ".json"))) true) [:tiers (keyword tier)])]
  {:provider provider :model (or explicit-model (:model entry)) :effort (or reasoning (:defaultEffort entry) (:defaultReasoning entry))})
  (catch Exception _
    {:provider provider :model explicit-model :effort reasoning})))))

(declare known semantic-handle)

(defn- agent-facts-one [id]
  (try
  (let [rows (north.coord/show-rows! (parse-long PORT) (str "@agent:" id))]
  (if (and (vector? rows) (every? (fn [%1] (and (vector? %1) (= 2 (count %1)) (every? string? %1))) rows)) (do
  (reduce (fn [acc [predicate value]] (north.agent-provenance/fold-fact acc predicate value)) {} rows))))
  (catch Exception _
    nil)))

(def control-id-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

(def max-control-id-bytes 256)

(def max-live-controls 256)

(def max-roster-fact-rows 32768)

(def max-roster-run-candidates 4096)

(def roster-conflict-key "__roster_conflicts")

(def lane-resolution-key ::lane-resolution)

(defn- valid-control-id? [value]
  (and (string? value) (<= (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8)) max-control-id-bytes) (boolean (re-matches control-id-pattern value))))

(defn- fold-roster-fact [facts predicate value]
  (let [prior-present? (contains? facts predicate)
   prior (get facts predicate)
   next (north.agent-provenance/fold-fact facts predicate value)
   prior-has-value? (cond
  (not prior-present?) false
  (set? prior) (contains? prior value)
  (and (sequential? prior) (not (string? prior))) (boolean (some (fn [%1] (= value %1)) prior))
  :else (= prior value))]
  (if (or (= predicate "holds") (not prior-present?) prior-has-value?) next (update next roster-conflict-key (fnil conj #{}) predicate))))

(defn- fold-roster-subjects [rows-by-subject allowed-subjects]
  (if (not (and (map? rows-by-subject) (every? (fn [[subject rows]] (and (string? subject) (contains? allowed-subjects subject) (vector? rows) (every? (fn [%1] (and (vector? %1) (= 2 (count %1)) (every? string? %1))) rows))) rows-by-subject) (<= (reduce + 0 (map (comp count val) rows-by-subject)) max-roster-fact-rows))) (do
  (throw (ex-info "agent subject projection was malformed" {}))))
  (reduce (fn [out [subject rows]] (assoc-in out [:agents (subs subject (count "@agent:"))] (reduce (fn [facts [predicate value]] (fold-roster-fact facts predicate value)) {} rows))) {:agents {} :sessions {}} rows-by-subject))

(defn roster-facts
  "Read exact live @agent subjects from the coordination origin in one bounded\n  query. Historical telemetry @session descriptors are not live identity and\n  never enter the machine roster." [ids]
  (let [ids (vec (distinct ids))]
  (cond
  (empty? ids) {:agents {} :sessions {}}
  (or (> (count ids) max-live-controls) (not-every? valid-control-id? ids)) {:err "liveness lease query returned an invalid or over-broad control set"}
  :else (let [subjects (mapv (fn [%1] (str "@agent:" %1)) ids)
   allowed-subjects (set subjects)]
  (let [response (try
  (north.coord/show-many-in-domain! (Integer/parseInt PORT) :coordination subjects)
  (catch Exception _
    ::unavailable))]
  (cond
  (= ::unavailable response) {:err "agent subject projection unavailable"}
  (not (and (map? response) (integer? (:version response)) (not (neg? (:version response))) (map? (:rows response)))) {:err "agent subject projection was malformed"}
  :else (try
  (fold-roster-subjects (:rows response) allowed-subjects)
  (catch Exception _
    {:err "agent subject projection was malformed"}))))))))

(defn- roster-run-entries-attempt
  "Resolve run candidates for IDS with one bounded telemetry query and one\n  batched exact-subject projection." [ids]
  (try
  (let [rules (mapv (fn [control] {:head {:rel "roster_run_candidate" :args [{:var "e"}]} :body [{:rel "triple" :args [{:var "e"} "agent" control]}]}) ids)
   response (north.coord/bounded-query-in-domain! (Integer/parseInt PORT) :telemetry {:find "roster_run_candidate" :rules rules} max-roster-run-candidates)
   rows (:rows response)]
  (if (and (map? response) (vector? rows) (<= (count rows) max-roster-run-candidates) (every? (fn [%1] (and (vector? %1) (= 1 (count %1)) (every? string? %1))) rows)) (let [subjects (->> rows (map first) (filter north.terminal-projection/valid-run-entity?) distinct sort vec)
   projected (if (seq subjects) (north.coord/show-many-in-domain! (Integer/parseInt PORT) :telemetry subjects) {:version (:served-version response) :rows {}})
   rows-by-subject (:rows projected)
   _ (if (not (and (map? projected) (integer? (:version projected)) (not (neg? (:version projected))) (map? rows-by-subject) (every? (fn [[subject fact-rows]] (and (contains? (set subjects) subject) (vector? fact-rows) (every? (fn [%1] (and (vector? %1) (= 2 (count %1)) (every? string? %1))) fact-rows))) rows-by-subject))) (do
  (throw (ex-info "run subject projection was malformed" {}))))
   entries (mapv (fn [subject] {:subject subject :facts (reduce (fn [facts [predicate value]] (if (contains? (set north.terminal-projection/run-resolution-predicates) predicate) (update facts predicate (fnil conj #{}) value) facts)) {} (get rows-by-subject subject []))}) subjects)]
  {:ok true :by-agent (into {} (map (fn [control] [control (filterv (fn [%1] (contains? (get-in %1 [:facts "agent"] #{}) control)) entries)])) ids)}) {:ok false :reason :run-projection-malformed}))
  (catch Exception _
    {:ok false :reason :run-projection-unavailable})))

(defn roster-run-entries
  "Read run candidates for every live control in one bounded attempt." [ids]
  (let [ids (vec (distinct ids))]
  (if (empty? ids) {:ok true :by-agent {}} (roster-run-entries-attempt ids))))

(defn attach-lane-resolutions [ids agents run-projection]
  (into {} (map (fn [control] (let [facts (get agents control {})
   managed? (= "lane" (get facts "kind"))
   agent-projection (get-in run-projection [:by-agent control])]
  [control (if managed? (let [resolution (cond
  (not (:ok run-projection)) {:status :indeterminate :reason (:reason run-projection)}
  (map? agent-projection) {:status :indeterminate :reason (:err agent-projection)}
  :else (north.terminal-projection/lane-resolution control facts (or agent-projection [])))]
  (assoc facts lane-resolution-key resolution)) facts)]))) ids))

(defn current-repo []
  (let [r (run ["git" "remote" "get-url" "origin"] :timeout 1500)]
  (if (:ok r) (some-> (:out r) str/trim (str/split #"[/:]") last (str/replace #"\.git$" "")) (some-> (System/getProperty "user.dir") (str/split #"/") last))))

(defn- known [value]
  (let [s (some-> value str str/trim)]
  (if (seq s) (do
  s))))

(defn- fact-one [facts predicate]
  (if (not (or (contains? (get facts north.agent-provenance/conflict-key #{}) predicate) (contains? (get facts roster-conflict-key #{}) predicate))) (do
  (known (get facts predicate)))))

(defn- slug [value]
  (or (some-> (known value) str/lower-case (str/replace #"[^a-z0-9]+" "-") (str/replace #"(^-|-$)" "") known) "unknown"))

(defn- model-display [model]
  (let [m (slug model)
   parts (set (str/split m #"-"))]
  (or (some (fn [%1] (if (parts %1) (do
  %1))) ["opus" "sonnet" "haiku" "fable" "sol" "terra" "luna"]) m)))

(defn- meaningful-task [value]
  (let [task (known value)]
  (if (not (#{"CONTEXT BRIEF:" "DELEGATE TASK:" "TASK:"} task)) (do
  task))))

(defn- axis-observation [facts predicate]
  (if (= "session" (fact-one facts "kind")) (north.agent-provenance/native-axis facts predicate) {:value (fact-one facts predicate) :conflict (or (contains? (get facts north.agent-provenance/conflict-key #{}) predicate) (contains? (get facts roster-conflict-key #{}) predicate))}))

(defn- composition-overrides [facts]
  (north.agent-provenance/composition-overrides facts))

(defn- orchestration-provenance [facts]
  (north.agent-provenance/orchestration-provenance facts))

(defn- provider-target-label [facts]
  (let [provider-observation (axis-observation facts "provider")
   vendor-observation (axis-observation facts "vendor")
   target-observation (axis-observation facts "provider_target")
   provider (or (:value provider-observation) (:value vendor-observation) "unknown")
   target (:value target-observation)]
  (cond
  (or (:conflict provider-observation) (and (nil? (:value provider-observation)) (:conflict vendor-observation))) "provider:conflict"
  (:conflict target-observation) (str provider ":target-conflict")
  target (str provider ":" (if (or (= target provider) (= target "ambient")) "ambient" target))
  :else provider)))

(defn- provider-axis-label [facts]
  (let [native? (= "session" (fact-one facts "kind"))
   provider-observation (axis-observation facts "provider")
   vendor-observation (axis-observation facts "vendor")
   provider-conflict? (or (:conflict provider-observation) (and (nil? (:value provider-observation)) (:conflict vendor-observation)))
   provider-value (or (:value provider-observation) (:value vendor-observation))]
  (cond
  provider-conflict? "provider:conflict"
  (and native? (nil? provider-value)) "provider:historical-unrecorded"
  (= provider-value "unobserved") "provider:unobserved"
  :else (provider-target-label facts))))

(defn- model-axis-label [facts]
  (let [native? (= "session" (fact-one facts "kind"))
   observation (axis-observation facts "model")
   value (:value observation)]
  (cond
  (:conflict observation) "model:conflict"
  (and native? (nil? value)) "model:historical-unrecorded"
  (= value "unobserved") "model:unobserved"
  :else (model-display (or value "unknown")))))

(defn- effort-axis-label [facts]
  (let [native? (= "session" (fact-one facts "kind"))
   observation (axis-observation facts "effort")
   value (:value observation)]
  (cond
  (:conflict observation) "effort:conflict"
  (and native? (nil? value)) "effort:historical-unrecorded"
  (= value "unobserved") "effort:unobserved"
  :else (slug (or value "unknown")))))

(defn- raw-provider [facts]
  (let [provider (axis-observation facts "provider")
   vendor (axis-observation facts "vendor")]
  (if (or (:conflict provider) (and (nil? (:value provider)) (:conflict vendor))) "conflict" (or (:value provider) (:value vendor) ""))))

(defn- raw-provider-target [facts]
  (let [observation (axis-observation facts "provider_target")]
  (if (:conflict observation) "conflict" (or (:value observation) ""))))

(defn- raw-model [facts]
  (let [observation (axis-observation facts "model")]
  (if (:conflict observation) "conflict" (or (:value observation) ""))))

(defn- live-input-label [facts]
  (let [observation (axis-observation facts "live_input")]
  (cond
  (:conflict observation) "conflict"
  (#{"streaming" "turn-messages" "unsupported"} (:value observation)) (:value observation)
  :else "unrecorded")))

(defn- live-input-state-label [facts]
  (let [observation (axis-observation facts "live_input_state")]
  (cond
  (:conflict observation) "conflict"
  (#{"pending" "armed" "frozen"} (:value observation)) (:value observation)
  :else "unrecorded")))

(defn- task-of [presence facts session]
  (or (meaningful-task (fact-one session "current_referent")) (meaningful-task (fact-one session "active_workflow")) (meaningful-task (fact-one session "task")) (meaningful-task (fact-one facts "current_referent")) (meaningful-task (fact-one facts "active_workflow")) (meaningful-task (fact-one facts "task")) (meaningful-task (fact-one facts "goal")) (meaningful-task (:focus presence)) (if (and (= "session" (fact-one facts "kind")) (fact-one facts "repo")) (do
  (str "native session in " (fact-one facts "repo")))) "unknown"))

(defn- terminal-state [presence facts]
  (let [resolution (or (get facts lane-resolution-key) (north.terminal-projection/lane-resolution (:id presence) facts []))
   process-outcome (if (= :resolved (:status resolution)) (do
  (:outcome resolution)))
   delivery-outcome (if (= :resolved (:status resolution)) (do
  (:delivery-outcome resolution)))
   delivery-label (or delivery-outcome "unrecorded")
   state (cond
  process-outcome "finished"
  (= :indeterminate (:status resolution)) "inconsistent"
  (fact-one facts "stalled") "stalled"
  (:online presence) "working"
  :else "offline")
   state-label (cond
  process-outcome (str "finished(process:" process-outcome ", delivery:" delivery-label ")")
  (= :indeterminate (:status resolution)) (str "inconsistent(lifecycle:" (name (:reason resolution)) ")")
  :else state)]
  {:process-outcome (or process-outcome "") :delivery-outcome (or delivery-outcome "") :resolution-status (:status resolution) :resolution-reason (:reason resolution) :state state :state-label state-label}))

(defn- role-axis [facts]
  (if (and (fact-one facts "role") (not (contains? #{"template" "bespoke"} (fact-one facts "composition_kind")))) (do
  (str " · role:" (slug (fact-one facts "role"))))))

(defn semantic-handle [id facts]
  (let [provider-axis (provider-target-label facts)
   composition (orchestration-provenance facts)
   model-observation (axis-observation facts "model")
   effort-observation (axis-observation facts "effort")
   model (if (:conflict model-observation) "model:conflict" (:value model-observation))
   effort (if (:conflict effort-observation) "effort:conflict" (:value effort-observation))
   suffix (last (str/split (str id) #"-"))]
  (str/join "-" [(slug provider-axis) (model-display model) (slug effort) (slug composition) (slug suffix)])))

(defn render-display-name [id facts]
  (let [goal (known (get facts "goal"))
   g (if goal (do
  (str " — " (if (> (count goal) 40) (str (subs goal 0 37) "…") goal))))]
  (str (semantic-handle id facts) g)))

(defn agent-primary-line
  ([presence facts]
    (agent-primary-line presence facts {}))
  ([presence facts session]
    (let [task (task-of presence facts session)
   state (:state-label (terminal-state presence facts))]
  (str (provider-axis-label facts) " · " (model-axis-label facts) " · " (effort-axis-label facts) " · " (orchestration-provenance facts) (role-axis facts) " · " state ": " task))))

(defn roster-json-row [presence facts session]
  (let [{:keys [process-outcome delivery-outcome state state-label resolution-status resolution-reason]} (terminal-state presence facts)
   task (task-of presence facts session)
   control (:id presence)]
  {"uuid" control "control_id" control "display_name" (agent-primary-line presence facts session) "display_handle" (semantic-handle control facts) "kind" (or (fact-one facts "kind") "unclassified") "provider" (raw-provider facts) "provider_target" (raw-provider-target facts) "provider_label" (provider-axis-label facts) "live_input" (live-input-label facts) "live_input_state" (live-input-state-label facts) "live_input_epoch" (or (fact-one facts "live_input_epoch") "") "model" (raw-model facts) "model_display" (model-axis-label facts) "effort" (effort-axis-label facts) "orchestration_provenance" (orchestration-provenance facts) "goal" (or (fact-one facts "goal") "") "task" task "state" state "state_label" state-label "lifecycle" state "lifecycle_resolution" (name resolution-status) "lifecycle_reason" (if resolution-reason (name resolution-reason) "") "process_outcome" process-outcome "delivery_outcome" delivery-outcome "online" (boolean (:online presence)) "expires_s" (:expires-s presence)}))

(defn roster-category [facts]
  (let [resolution (or (get facts lane-resolution-key) (north.terminal-projection/lane-resolution nil facts []))]
  (cond
  (= :resolved (:status resolution)) :recently-finished
  (= :indeterminate (:status resolution)) :inconsistent
  (= "lane" (fact-one facts "kind")) :active-agent
  (= "session" (fact-one facts "kind")) :native-session
  :else :unclassified)))

(defn presence-rows []
  (try
  (let [port (Integer/parseInt PORT)
   now (System/currentTimeMillis)
   sessions (north.coord/online-session-leases! port now)
   valid? (and (vector? sessions) (<= (count sessions) max-live-controls) (every? (fn [session] (and (= #{:handle :exp} (set (keys session))) (valid-control-id? (:handle session)) (integer? (:exp session)) (> (:exp session) now))) sessions) (= (count sessions) (count (set (map (fn [session] (:handle session)) sessions)))))]
  (if valid? {:agents (mapv (fn [{:keys [handle exp]}] (let [expires-s (quot (- (long exp) now) 1000)]
  {:id handle :online true :expires-s expires-s :expires (str expires-s "s")})) sessions)} {:err "liveness lease projection was malformed"}))
  (catch Exception _
    {:err "liveness lease projection unavailable"})))

(defn agent-online? [id]
  (try
  (north.coord/session-online?! (Integer/parseInt PORT) id)
  (catch Exception _
    false)))

(defn agents-usage []
  (println "north agent list — provider-neutral live roster")
  (println)
  (println "Usage:")
  (println "  north agent list")
  (println "  north agent list --verbose")
  (println "  north agent list --json")
  (println)
  (println "--json emits the versioned north:agent-roster:v1 machine contract."))

(defn- agents-error! [message]
  (binding [*out* *err*]
  (println (str "north agent list: " message))
  (println "run 'north agent list --help' for usage"))
  (System/exit 1))

(defn- parse-agents-options! [args]
  (loop [remaining (vec args)
   options {:mode :human :verbose false}]
  (if (empty? remaining) options (let [[arg & more] remaining]
  (cond
  (#{"--help" "-h" "help"} arg) (if (empty? more) (assoc options :help true) (agents-error! "help cannot be combined with other options"))
  (#{"--verbose" "--debug"} arg) (if (or (:verbose options) (not= :human (:mode options))) (agents-error! (str "conflicting or duplicate option " arg)) (recur (vec more) (assoc options :verbose true)))
  (= "--json" arg) (if (or (:verbose options) (not= :human (:mode options))) (agents-error! "conflicting or duplicate option --json") (recur (vec more) (assoc options :mode :json)))
  :else (agents-error! (str "unknown option " arg)))))))

(defn- roster-row-key [row]
  [(case (get row "state")
    "finished" 3
    (case (get row "kind")
    "lane" 0
    "session" 1
    2)) (get row "display_name") (get row "control_id")])

(defn- roster-contract [presence agents sessions]
  {"version" ROSTER-CONTRACT-VERSION "agents" (->> presence (mapv (fn [row] (roster-json-row row (get agents (:id row) {}) (get sessions (:id row) {})))) (sort-by roster-row-key) vec)})

(defn- configured-codex-accounts []
  (let [result (run [POLICY-BUN "run" CODEX-CENSUS-CLI] :timeout 4000)
   parsed (try
  (json/parse-string (str/trim (:out result)) false)
  (catch Exception _
    nil))]
  (if (and (:ok result) (sequential? parsed) (every? (fn [%1] (and (string? %1) (not (str/blank? %1)))) parsed)) (vec (sort (distinct parsed))) [])))

(defn- census-fact [facts predicate]
  (if (not (contains? (get facts roster-conflict-key #{}) predicate)) (do
  (known (get facts predicate)))))

(defn- codex-census [rows agents]
  (let [parent-of (fn [facts] (or (census-fact facts "coordinator") (census-fact facts "supervisor")))
   children (reduce (fn [out row] (let [facts (get agents (:id row) {})
   parent (parent-of facts)]
  (if (and (= "openai" (census-fact facts "provider")) parent) (update out parent (fnil conj []) (:id row)) out))) {} rows)]
  {"configured_accounts" (configured-codex-accounts) "sessions" (->> rows (keep (fn [row] (let [facts (get agents (:id row) {})
   parent (parent-of facts)]
  (if (= "openai" (census-fact facts "provider")) (do
  {"control_id" (:id row) "account_id" (or (census-fact facts "provider_target") "ambient") "session_identity" (:id row) "parent_control_id" (or parent "") "child_control_ids" (vec (sort (get children (:id row) []))) "activity_at" (or (census-fact facts "started_at") (census-fact facts "spawned_at") "") "freshness" (if (:online row) "fresh" "stale") "freshness_evidence" (if (:online row) "Store liveness lease" "Store liveness lease absent")}))))) (sort-by (fn [%1] (get %1 "control_id"))) vec)}))

(def comparable-roster-fields ["control_id" "display_handle" "kind" "provider" "provider_target" "provider_label" "model" "model_display" "effort" "orchestration_provenance"])

(defn- read-roster-snapshot []
  (let [presence (presence-rows)]
  (if (:err presence) {:err (:err presence)} (let [rows (vec (filter (fn [row] (:online row)) (:agents presence)))
   ids (mapv (fn [row] (:id row)) rows)
   facts (roster-facts ids)]
  (if (:err facts) {:err (:err facts)} (let [managed-ids (filterv (fn [%1] (= "lane" (get-in facts [:agents %1 "kind"]))) ids)
   run-projection (roster-run-entries managed-ids)
   agents (attach-lane-resolutions ids (:agents facts) run-projection)
   sessions (:sessions facts)]
  {:rows rows :agents agents :sessions sessions :snapshot (assoc (roster-contract rows agents sessions) "codex_census" (codex-census rows agents))}))))))

(defn- comparable-roster [snapshot]
  (if (and (= ROSTER-CONTRACT-VERSION (get snapshot "version")) (vector? (get snapshot "agents"))) (do
  (let [rows (get snapshot "agents")]
  (if (and (every? (fn [%1] (and (map? %1) (every? (fn [field] (contains? %1 field)) comparable-roster-fields))) rows) (= (count rows) (count (set (map (fn [%1] (get %1 "control_id")) rows))))) (do
  (->> rows (mapv (fn [%1] (select-keys %1 comparable-roster-fields))) (sort-by (fn [%1] (get %1 "control_id"))) vec)))))))

(defn cmd-agents! [args]
  (let [{:keys [mode verbose help]} (parse-agents-options! args)]
  (if help (agents-usage) (do
  (if verbose (do
  (println (dim (str "Store RPC liveness lease projection :" PORT)))))
  (let [loaded (read-roster-snapshot)]
  (if (:err loaded) (if (= mode :human) (println (ylw (:err loaded))) (agents-error! (:err loaded))) (let [rows (:rows loaded)
   af (:agents loaded)
   sf (:sessions loaded)
   snapshot (:snapshot loaded)]
  (if (= mode :json) (println (json/generate-string snapshot)) (let [categorized (group-by (fn [a] (roster-category (get af (:id a) {}))) rows)
   active-agents (vec (:active-agent categorized []))
   native-sessions (vec (:native-session categorized []))
   unclassified (vec (:unclassified categorized []))
   inconsistent (vec (:inconsistent categorized []))
   finished (vec (:recently-finished categorized []))
   active (+ (count active-agents) (count native-sessions) (count unclassified))
   render-section (fn [title note section] (if (seq section) (do
  (println)
  (if note (println (bold (str title " (" (count section) ")")) (dim note)) (println (bold (str title " (" (count section) ")"))))
  (doseq [a section]
  (let [facts (get af (:id a) {})
   session (get sf (:id a) {})
   handle (semantic-handle (:id a) facts)]
  (println (str "  " (grn "●") " " (agent-primary-line a facts session)))
  (println (dim (str "    " handle " · control " (:id a) " · live-input " (live-input-label facts) " · ttl " (:expires a)))))))))]
  (println (bold (str (count rows) " roster entries")) (dim (str "· " active " active · " (count inconsistent) " inconsistent · " (count finished) " recently finished")))
  (render-section "active agents" nil active-agents)
  (render-section "native sessions" "(active provider CLI sessions)" native-sessions)
  (render-section "unclassified lease" "(missing identity facts)" unclassified)
  (render-section "inconsistent lifecycle" "(terminal/run projection is incomplete, conflicting, or unavailable)" inconsistent)
  (render-section "recently finished" "(process is terminal; delivery evidence is shown separately; liveness lease has not lapsed)" finished)
  (let [known (set (map (fn [row] (:id row)) rows))
   orphans (->> (or (.listFiles (java.io.File. "/proc")) []) (filter (fn [%1] (re-matches #"\d+" (.getName %1)))) (keep (fn [d] (let [env (try
  (slurp (java.io.File. d "environ"))
  (catch Exception _
    ""))
   kv (into {} (keep (fn [e] (let [[k v] (str/split e #"=" 2)]
  (if v (do
  [k v])))) (str/split env (re-pattern "\u0000"))))
   id (get kv "AGENT_ID")]
  (if (and id (str/starts-with? (str id) "lane-") (not (contains? known id))) (do
  {:id id :pid (.getName d) :referent (get kv "AGENT_REFERENT")}))))) (reduce (fn [m o] (assoc m (:id o) o)) {}) ((fn [orphans-by-id] (mapv (fn [id] (get orphans-by-id id)) (sort (keys orphans-by-id))))))]
  (if (seq orphans) (do
  (println)
  (println (bold (str "orphaned processes (" (count orphans) ")")) (dim "(live AGENT_ID with no roster lease — reap or investigate)"))
  (doseq [o orphans]
  (println (str "  " (red "●") " pid " (:pid o) " · " (:id o)))
  (println (dim (str "    referent " (or (:referent o) "(unbound)") " · not in the roster: its lease lapsed while the process lived"))))))))))))))))

(def spawn-flags {"--notify" :notify "--provider" :provider "--target" :target "--taskGrade" :taskGrade "--task-grade" :taskGrade "--domain" :domain "--topology" :topology "--capability-floor" :capabilityFloor "--service-class" :serviceClass "--reasoning" :reasoning "--deliberation" :reasoning "--posture" :posture "--composition" :composition "--rationale" :rationale "--nearest" :nearest "--contract" :contract "--override-reason" :overrideReason "--model" :model "--assessment" :assessment "--routing-assessment" :assessment "--pin-evidence" :pinEvidence "--referent" :referent})

(defn cmd-spawn-help []
  (let [roles (sort (keys (or (orchestration-routing) {})))]
  (println "north agent spawn — start one managed lane with an explicit Orchestration composition")
  (println)
  (println "Usage:")
  (println "  north agent spawn <role> \"<prompt>\" [routing options] [--composition JSON|@file] [--dry-run]")
  (println)
  (println "Role and composition:")
  (println "  Role is functional identity, independent of composition kind and id.")
  (println "  Catalogued and novel roles may use either a template or bespoke composition.")
  (println "  A template composition hydrates task grade, capability floor, service class, reasoning, topology, posture, and capabilities.")
  (println "  Override an axis with --task-grade, --domain, --capability-floor, --service-class, --reasoning, or --posture;")
  (println "  any changed template axis requires --override-reason WHY. Exact templates carry no override reason.")
  (println "  Stock topology is fixed; --topology applies only to bespoke compositions.")
  (println "  Available templates:" (if (seq roles) (str/join " " roles) "(catalog unavailable)"))
  (println "  Inspect their full routing defaults with: north agent templates")
  (println)
  (println "Bespoke composition:")
  (println "  A bespoke composition requires a rationale and structured contract regardless of role identity.")
  (println "  Contract JSON contains exactly: responsibility, deliverable, capabilities, mayDecide,")
  (println "  mustEscalate, doneWhen, report. Text fields are nonblank; list fields are nonempty.")
  (println "  Canonical capabilities: filesystem.read filesystem.search filesystem.write shell")
  (println "                          shell.readonly web coordination")
  (println "  --nearest TEMPLATE is optional reference provenance, not inheritance.")
  (println "  Without --nearest, explicitly set task grade, topology, capability floor, service class, reasoning, and posture.")
  (println "  Domain requirements remain an explicit empty list when --domain is omitted.")
  (println "  --promotion-candidate nominates recurrence for human review; default is false.")
  (println "  --composition JSON|@file is the advanced full payload form (machine kinds: template|bespoke).")
  (println)
  (println "Routing and control:")
  (println "  Mutation-capable compositions default to a managed worktree lane.")
  (println "  SDK worktree=false is an explicit read-only opt-out; AGENT_WORKTREE=1 remains an explicit override.")
  (println "  --provider auto|anthropic|openai   provider preference (default auto)")
  (println "  --target ACCOUNT                  exact account pin; unavailable means no fallback")
  (println "  --model MODEL                     exact model pin")
  (println "  --assessment JSON|@file           canonical Orchestration selection-assessment sidecar")
  (println "  --pin-evidence JSON|@file         typed reason + <=24h expiry for provider/account/model pins")
  (println "  New explicit pins fail closed without --pin-evidence; reasoning=max requires --assessment.")
  (println "  --domain D[,D...]                 repeatable domain requirement")
  (println "  --reasoning low|medium|high|xhigh|max  (--deliberation is an alias)")
  (println "  --notify PEER                     completion/stall notifications")
  (println "  --dry-run                         validate pinned-provider capability authority; show identity only when supported")
  (println "  --doctor [--json]                 test every dispatch invariant at once; one PASS/FAIL row + fix per wall")
  (println "  --doctor --canary                 spawn one tiny read-only managed lane end to end and report its lifecycle")))

(defn- parse-spawn-args [args]
  (loop [xs args
   positionals []
   opts {:domains [] :seen #{}}]
  (let [x (first xs)]
  (if x (cond
  (= x "--dry-run") (recur (rest xs) positionals (assoc opts :dry? true))
  (= x "--ad-hoc") (recur (rest xs) positionals (assoc opts :ad-hoc? true))
  (#{"--promotion-candidate" "--nominate" "--no-promotion-candidate"} x) (if (:promotion-specified? opts) (do
  (println (red "choose exactly one promotion decision"))
  (System/exit 1)) (recur (rest xs) positionals (assoc opts :promotion-specified? true :promotionCandidate (not= x "--no-promotion-candidate"))))
  (spawn-flags x) (let [v (second xs)
   field (spawn-flags x)]
  (if (or (nil? v) (str/starts-with? (str v) "--")) (do
  (println (red (str x " requires a value")))
  (System/exit 1)))
  (if (and (not= field :domain) (contains? (:seen opts) field)) (do
  (println (red (str "duplicate spawn option for " (name field) ": " x)))
  (System/exit 1)))
  (recur (nnext xs) positionals (if (= :domain field) (update opts :domains into (remove str/blank? (map str/trim (str/split (str v) #",")))) (-> opts (assoc field v) (update :seen conj field)))))
  (str/starts-with? x "--") (do
  (println (red (str "unknown spawn option: " x)))
  (System/exit 1))
  :else (recur (rest xs) (conj positionals x) opts)) (assoc (dissoc opts :seen) :positionals positionals)))))

(defn- parse-json-input [label input]
  (if input (do
  (try
  (let [source (if (str/starts-with? (str input) "@") (slurp (subs (str input) 1)) input)]
  (json/parse-string source true))
  (catch Exception e
    (println (red (str label " must be valid JSON or @file: " (.getMessage e))))
    (System/exit 1))))))

(def routing-economics-preflight-timeout-ms 120000)

(defn- preflight-failure-message
  "Never let a preflight die anonymously. `run` reports three distinguishable\n   non-ok shapes and only one of them carries subprocess output; name the other\n   two explicitly rather than falling back to a bare adjective." [result timeout-ms]
  (let [out (str (:out result))
   err (str (:err result))
   joined (str/trim (str err (if (and (seq err) (seq out)) (do
  "\n")) out))]
  (cond
  (seq joined) joined
  (:timeout result) (str "routing economics preflight exceeded its " timeout-ms "ms budget and was killed before it could report a reason" " (a cold or contended coordinator on port " (or (System/getenv "NORTH_PORT") "7977") " is the usual cause; retry, and if it persists check that the" " coordinator is live and @catalog:current is imported)")
  (:error result) (str "routing economics preflight could not be started: " (:error result))
  :else (str "routing economics preflight exited " (:exit result) " without writing a reason to stdout or stderr"))))

(defn- preflight-routing-economics! [routing-metadata routing-assessment pin-evidence provider target model dry?]
  (let [payload (cond-> {:routingMetadata routing-metadata} routing-assessment (assoc :routingAssessment routing-assessment) pin-evidence (assoc :pinEvidence pin-evidence) provider (assoc :provider provider) target (assoc :target target) model (assoc :model model))
   result (run [POLICY-BUN "run" ROUTING-ECONOMICS-PREFLIGHT-CLI] :timeout routing-economics-preflight-timeout-ms :env (assoc (into {} (System/getenv)) "NORTH_STAFFING_SOURCE" "file") :in (json/generate-string payload))]
  (if (not (:ok result)) (do
  (println (red (preflight-failure-message result routing-economics-preflight-timeout-ms)))
  (System/exit 1)))
  (try
  (let [receipt (json/parse-string (str/trim (:out result)) true)]
  (if (not (= 1 (:version receipt))) (do
  (throw (ex-info "missing immutable admission receipt" {}))))
  receipt)
  (catch Exception _
    (println (red "routing economics preflight returned an invalid admission receipt"))
    (System/exit 1)))))

(defn- resolved-spawn-topology
  "Resolve the exact topology cmd-spawn will apply, including bespoke nearest\n   preset hydration. Delegate classification must inspect this value before it\n   may label a handoff atomic." [{:keys [topology nearest composition positionals]}]
  (let [role (first positionals)
   templates (or (orchestration-routing) {})
   supplied-composition (parse-json-input "--composition" composition)
   nearest-role (or nearest (:nearestTemplate supplied-composition))
   base (or (get templates role) (get templates nearest-role))]
  (or topology (:topology base))))

(def canonical-orchestration-capabilities ["filesystem.read" "filesystem.search" "filesystem.write" "shell" "shell.readonly" "web" "coordination"])

(def bespoke-fingerprint-version "v1")

(def bespoke-fingerprint-domain "north:bespoke-contract:v1")

(def edge-ascii-whitespace #"^[\u0009-\u000d\u0020]+|[\u0009-\u000d\u0020]+$")

(defn- canonical-contract-text [value]
  (-> value (str/replace #"\r\n?" "\n") (java.text.Normalizer/normalize java.text.Normalizer$Form/NFC) (str/replace edge-ascii-whitespace "")))

(defn- canonical-contract-list [values]
  (->> values (map canonical-contract-text) distinct sort vec))

(defn- canonical-bespoke-contract [contract]
  (let [requested-capabilities (set (map canonical-contract-text (:capabilities contract)))]
  (array-map :responsibility (canonical-contract-text (:responsibility contract)) :deliverable (canonical-contract-text (:deliverable contract)) :capabilities (vec (filter (fn [capability] (contains? requested-capabilities capability)) canonical-orchestration-capabilities)) :mayDecide (canonical-contract-list (:mayDecide contract)) :mustEscalate (canonical-contract-list (:mustEscalate contract)) :doneWhen (canonical-contract-list (:doneWhen contract)) :report (canonical-contract-text (:report contract)))))

(defn- utf8-segment [value]
  (str (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8)) ":" value))

(defn- utf8-list-segment [values]
  (str (count values) ":" (apply str (map utf8-segment values))))

(defn- canonical-bespoke-contract-payload [canonical]
  (str bespoke-fingerprint-domain "\n" "responsibility=" (utf8-segment (:responsibility canonical)) "\n" "deliverable=" (utf8-segment (:deliverable canonical)) "\n" "capabilities=" (utf8-list-segment (:capabilities canonical)) "\n" "mayDecide=" (utf8-list-segment (:mayDecide canonical)) "\n" "mustEscalate=" (utf8-list-segment (:mustEscalate canonical)) "\n" "doneWhen=" (utf8-list-segment (:doneWhen canonical)) "\n" "report=" (utf8-segment (:report canonical))))

(defn- bespoke-contract-sha256 [contract]
  (let [canonical (canonical-bespoke-contract contract)
   bytes (.digest (doto (java.security.MessageDigest/getInstance "SHA-256")
  (.update (.getBytes (canonical-bespoke-contract-payload canonical) java.nio.charset.StandardCharsets/UTF_8))))]
  (apply str (map (fn [%1] (format "%02x" (bit-and (int %1) 0xff))) bytes))))

(def routing-override-fields #{"taskGrade" "domainRequirements" "capabilityFloor" "serviceClass" "reasoning" "posture"})

(def routing-request-fields #{:role :taskGrade :domainRequirements :topology :capabilityFloor :serviceClass :reasoning :posture :composition})

(def bespoke-contract-fields #{:responsibility :deliverable :capabilities :mayDecide :mustEscalate :doneWhen :report})

(def role-id-pattern #"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")

(defn- valid-string-list? [value require-items?]
  (and (sequential? value) (or (not require-items?) (seq value)) (every? string? value) (let [normalized (mapv canonical-contract-text value)]
  (and (every? seq normalized) (= (count normalized) (count (set normalized)))))))

(defn- valid-contract-string-list? [value]
  (and (sequential? value) (seq value) (every? string? value) (let [normalized (mapv canonical-contract-text value)]
  (and (every? seq normalized) (= (count normalized) (count (set normalized)))))))

(defn- valid-contract-text? [value]
  (and (string? value) (seq (canonical-contract-text value))))

(defn- non-empty-string? [value]
  (and (string? value) (not (str/blank? value))))

(defn- topology-capability-problem [topology capabilities]
  (let [caps (set capabilities)
   missing-closure (fn [surface required] (let [missing (remove (fn [capability] (contains? caps capability)) required)]
  (if (and (caps surface) (seq missing)) (do
  (str "composition.contract.capabilities: capability list is not closed; missing implied " (str/join ", " missing))))))]
  (cond
  (and (caps "shell") (caps "shell.readonly")) "shell and shell.readonly are mutually exclusive"
  (and (= topology "orchestrator") (not (contains? caps "coordination"))) "orchestrator topology requires coordination capability"
  (and (= topology "orchestrator") (caps "filesystem.write")) "orchestrator topology forbids filesystem.write capability"
  (and (= topology "orchestrator") (caps "shell")) "orchestrator topology forbids unrestricted shell capability"
  (and (= topology "worker") (caps "coordination")) "worker topology forbids coordination capability"
  :else (or (missing-closure "shell" ["filesystem.read" "filesystem.search" "filesystem.write"]) (missing-closure "shell.readonly" ["filesystem.read" "filesystem.search"])))))

(def ^:dynamic *delegate-request* nil)

(def ^:dynamic *selected-routing-request* nil)

(def ^:dynamic *selected-routing-assessment* nil)

(declare resolve-delegate-referent! resolve-recursive-child-referent! delegate-brief)

(defn referent-title-verdict
  "A spawn attribution target exists only when the coordinator's exact-subject\n  projection contains exactly one entity_kind=referent and one nonblank title.\n  Read through the same :show projection as the daemon-first CLI; the independent\n  :resolved index can lag that projection and must not turn a visible Referent\n  into a false absence. A failed read is :unreadable — a degraded coordinator is\n  not an absent Referent." [id]
  (try
  (let [subject (str "@" (str/replace-first (str id) #"^@" ""))
   rows (north.coord/show-rows! (Integer/parseInt PORT) subject)
   kinds (mapv second (filter (fn [%1] (= "entity_kind" (first %1))) rows))
   titles (mapv second (filter (fn [%1] (= "title" (first %1))) rows))]
  (if (and (= ["referent"] kinds) (= 1 (count titles)) (not (str/blank? (first titles)))) :titled :untitled))
  (catch Exception _
    :unreadable)))

(defn title-bearing-referent? [id]
  (= :titled (referent-title-verdict id)))

(defn warn-unarmed-notify! [notify]
  (if notify (do
  (let [route (north.message-routing/require-live-address (Integer/parseInt PORT) notify)]
  (if (false? (:live route)) (do
  (println (ylw (str "NOTIFY TARGET " notify " HAS NO ARMED LISTENER — completions will not wake it; arm: north-arm " notify)))))))))

(defn cmd-spawn! [args]
  (north.topology-authority/require-coordination! "spawn")
  (let [{:keys [dry? notify provider target model taskGrade domains topology capabilityFloor serviceClass reasoning posture composition assessment pinEvidence rationale nearest contract overrideReason promotion-specified? promotionCandidate positionals referent ad-hoc?]} (parse-spawn-args args)
   selected-request *selected-routing-request*
   _ (if (and selected-request (or (not (map? selected-request)) (not= routing-request-fields (set (keys selected-request))))) (do
  (println (red "selected delegation run design must contain exactly the nine routing fields"))
  (System/exit 1)))
   [first-positional second-positional & remaining-positionals] positionals
   invoked-role (if selected-request (:role selected-request) first-positional)
   prompt (if selected-request first-positional second-positional)
   extra (if selected-request (rest positionals) remaining-positionals)
   _ (if (and referent ad-hoc?) (do
  (println (red "choose one: --referent <id> binds this run, --ad-hoc runs it unattributed"))
  (System/exit 2)))
   _ (if (not (or referent ad-hoc? *delegate-request* (str/blank? (str invoked-role)) (str/blank? (str prompt)))) (do
  (binding [*out* *err*]
  (println (red "North spawn requires --referent <id> so its effort is attributable"))
  (println (dim "  pass --referent <id> to bind this run to a workstream,"))
  (println (dim "  or --ad-hoc to deliberately run it unattributed.")))
  (System/exit 2)))
   _ (if (and referent (nil? (re-matches #"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|\d{4}-\d{2}-\d{2}-\d{6}" (str/replace-first (str referent) #"^@" "")))) (do
  (binding [*out* *err*]
  (println (red (str "--referent " referent " is not a canonical referent id")))
  (println (dim "  a prefix is recorded verbatim on the run and never joins back to its referent,"))
  (println (dim (str "  so the run would look bound and be orphaned. Resolve it first:")))
  (println (dim (str "    north fact show " referent "   # prints the full id, or names the ambiguity"))))
  (System/exit 2)))
   _ (if referent (do
  (let [bare (str/replace-first (str referent) #"^@" "")
   verdict (referent-title-verdict bare)]
  (if (= :unreadable verdict) (do
  (binding [*out* *err*]
  (println (red (str "--referent " bare " could not be read through the coordinator")))
  (println (dim "  the exact-subject projection failed; this is a degraded coordinator, not a missing referent."))
  (println (dim "  check `north system doctor`, then retry.")))
  (System/exit 75)))
  (if (= :untitled verdict) (do
  (binding [*out* *err*]
  (println (red (str "--referent " bare " names no exact Referent")))
  (println (dim "  it must carry exactly entity_kind=referent and one nonblank title."))
  (println (dim "  capture it first, or correct the id:"))
  (println (dim (str "    north fact show " (subs bare 0 (min 8 (count bare))) "   # find the real id by prefix"))))
  (System/exit 2))))))
   catalog (orchestration-catalog)
   dt (or (orchestration-routing) {})
   raw-supplied-composition (if selected-request (:composition selected-request) (parse-json-input "--composition" composition))
   routing-assessment (if selected-request *selected-routing-assessment* (parse-json-input "--assessment" assessment))
   pin-evidence (parse-json-input "--pin-evidence" pinEvidence)
   override-reason-conflict (and (= "template" (:kind raw-supplied-composition)) overrideReason (contains? raw-supplied-composition :overrideReason) (not= overrideReason (:overrideReason raw-supplied-composition)))
   supplied-composition (cond-> raw-supplied-composition (and (= "template" (:kind raw-supplied-composition)) overrideReason (not (contains? raw-supplied-composition :overrideReason))) (assoc :overrideReason overrideReason))
   supplied-template (if (= "template" (:kind supplied-composition)) (do
  (get dt (:id supplied-composition))))
   supplied-contract (parse-json-input "--contract" contract)
   canonical (get dt invoked-role)
   default-bespoke? (and invoked-role (nil? canonical))
   composition-kind (or (:kind supplied-composition) (if default-bespoke? "bespoke" "template"))
   bespoke? (= "bespoke" composition-kind)
   template? (= "template" composition-kind)
   bespoke-reason (or rationale (:bespokeReason supplied-composition))
   nearest-role (or nearest (:nearestTemplate supplied-composition))
   nearest-template (get dt nearest-role)
   contract-value (or supplied-contract (:contract supplied-composition))
   catalog-capability-order (vec (get-in catalog [:vocabulary :capabilities]))
   capability-values (set canonical-orchestration-capabilities)
   promotion-value (if promotion-specified? promotionCandidate (if (and (map? supplied-composition) (contains? supplied-composition :promotionCandidate)) (:promotionCandidate supplied-composition) false))
   template-base (if template? (do
  (or supplied-template canonical)))
   base (or template-base nearest-template)
   preset-grade (:taskGrade base)
   preset-capability-floor (:capabilityFloor base)
   preset-service-class (:serviceClass base)
   preset-posture (:posture base)
   preset-topology (:topology base)
   preset-deliberation (:deliberation base)
   selected-grade (if selected-request (:taskGrade selected-request) (or taskGrade preset-grade))
   selected-capability-floor (if selected-request (:capabilityFloor selected-request) (or capabilityFloor preset-capability-floor))
   selected-service-class (if selected-request (:serviceClass selected-request) (or serviceClass preset-service-class))
   selected-topology (if selected-request (:topology selected-request) (or topology preset-topology))
   selected-role (if selected-request (:role selected-request) invoked-role)
   selected-posture (if selected-request (:posture selected-request) (or posture preset-posture (:posture (:defaults catalog))))
   selected-reasoning (if selected-request (:reasoning selected-request) (or reasoning preset-deliberation))
   selected-domains (if selected-request (vec (:domainRequirements selected-request)) (vec (distinct domains)))
   missing-bespoke-axes (if (and bespoke? (nil? nearest-template) (not selected-request)) (do
  (seq (keep (fn [[label value]] (if (nil? value) (do
  label))) [["--task-grade" taskGrade] ["--topology" topology] ["--capability-floor" capabilityFloor] ["--service-class" serviceClass] ["--reasoning" reasoning] ["--posture" posture]]))))
   actual-overrides (if template? (do
  (vec (keep (fn [[field selected preset]] (if (not= selected preset) (do
  field))) [["taskGrade" selected-grade (:taskGrade template-base)] ["domainRequirements" selected-domains []] ["capabilityFloor" selected-capability-floor (:capabilityFloor template-base)] ["serviceClass" selected-service-class (:serviceClass template-base)] ["reasoning" selected-reasoning (:deliberation template-base)] ["posture" selected-posture (:posture template-base)]]))))
   generated-composition (if default-bespoke? (cond-> {:kind "bespoke" :id invoked-role :bespokeReason bespoke-reason :promotionCandidate promotion-value :contract contract-value} nearest-role (assoc :nearestTemplate nearest-role)) (cond-> {:kind "template" :id selected-role :overrides actual-overrides} (seq actual-overrides) (assoc :overrideReason overrideReason)))
   selected-composition (if selected-request (:composition selected-request) (or supplied-composition generated-composition))
   selected-capabilities (if template? (:capabilities template-base) (:capabilities contract-value))
   normalized-selected-capabilities (if (and (sequential? selected-capabilities) (every? string? selected-capabilities)) (do
  (mapv canonical-contract-text selected-capabilities)))
   capability-problem (if normalized-selected-capabilities (do
  (or (topology-capability-problem selected-topology normalized-selected-capabilities) (north.orchestration-staffing/posture-capability-problem selected-posture normalized-selected-capabilities))))
   allowed-composition-fields (case composition-kind
    "template" #{:kind :id :overrides :overrideReason}
    "bespoke" #{:kind :id :nearestTemplate :bespokeReason :promotionCandidate :contract}
    #{})
   unknown-composition-fields (if (map? selected-composition) (do
  (seq (remove (fn [field] (contains? allowed-composition-fields field)) (keys selected-composition)))))
   declared-overrides (if (map? selected-composition) (do
  (:overrides selected-composition)))
   contract-fields (if (map? (:contract selected-composition)) (do
  (set (keys (:contract selected-composition)))))]
  (cond
  (or (nil? invoked-role) (nil? prompt) (seq extra)) (do
  (println (red "usage:") "north agent spawn <role> \"<prompt>\" [--task-grade G] [--domain D] [--topology T] [--capability-floor F] [--service-class C] [--reasoning R] [--posture P] [--override-reason WHY] [--composition JSON|@file] [--assessment JSON|@file] [--rationale WHY] [--nearest PRESET] [--contract JSON|@file] [--promotion-candidate|--no-promotion-candidate] [--provider P] [--target ACCOUNT] [--model MODEL] [--pin-evidence JSON|@file] [--notify PEER] [--dry-run]")
  (println "role is functional identity independent of composition: catalogued and novel roles may use template or bespoke compositions")
  (println "roles:" (str/join " " (sort (keys dt)))))
  (#{"orchestrator" "worker"} invoked-role) (do
  (println (red (str invoked-role " is a topology, not a role")))
  (println (if (= invoked-role "orchestrator") "use director for decomposition/reconciliation, or choose a worker function for atomic work" "choose the worker function that names the deliverable, such as executor, implementer, integrator, or verifier"))
  (System/exit 1))
  (= invoked-role "researcher") (do
  (println (red "researcher is retired because it was ambiguous"))
  (println "use scout for source gathering, analyst for deep mechanism research, or scientist for cutting-edge inquiry")
  (System/exit 1))
  (and invoked-role (nil? (re-matches role-id-pattern invoked-role))) (do
  (println (red "role must be a lowercase kebab-case Orchestration role id"))
  (System/exit 1))
  (nil? catalog) (do
  (println (red (str "Delegation run-composition catalog unavailable: " ORCHESTRATION-STAFFING)))
  (System/exit 1))
  (not= canonical-orchestration-capabilities catalog-capability-order) (do
  (println (red "Orchestration capability vocabulary order disagrees with North's canonical fingerprint vocabulary"))
  (System/exit 1))
  (and template? (or rationale nearest contract promotion-specified?)) (do
  (println (red "--nearest, --rationale, --contract, and promotion decisions apply only to bespoke compositions"))
  (System/exit 1))
  (and template? (some? topology)) (do
  (println (red "--topology applies only to bespoke compositions; stock-template topology is fixed"))
  (System/exit 1))
  (and bespoke? overrideReason) (do
  (println (red "--override-reason applies only to template axis overrides"))
  (System/exit 1))
  override-reason-conflict (do
  (println (red "--override-reason conflicts with composition.overrideReason"))
  (System/exit 1))
  (and bespoke? nearest-role (nil? nearest-template)) (do
  (println (red (str "unknown nearest template: " nearest-role)))
  (System/exit 1))
  missing-bespoke-axes (do
  (println (red (str "bespoke composition without --nearest must explicitly set: " (str/join ", " missing-bespoke-axes))))
  (System/exit 1))
  (and bespoke? (not (non-empty-string? bespoke-reason))) (do
  (println (red (str "bespoke composition " (:id selected-composition) " requires --rationale or composition.bespokeReason")))
  (System/exit 1))
  (and bespoke? (nil? contract-value)) (do
  (println (red (str "bespoke composition " (:id selected-composition) " requires --contract JSON|@file or composition.contract")))
  (System/exit 1))
  (and supplied-composition rationale (not= rationale (:bespokeReason supplied-composition))) (do
  (println (red "--rationale conflicts with composition.bespokeReason"))
  (System/exit 1))
  (and supplied-composition nearest (not= nearest (:nearestTemplate supplied-composition))) (do
  (println (red "--nearest conflicts with composition.nearestTemplate"))
  (System/exit 1))
  (and supplied-composition supplied-contract (not= supplied-contract (:contract supplied-composition))) (do
  (println (red "--contract conflicts with composition.contract"))
  (System/exit 1))
  (and supplied-composition promotion-specified? (not= promotionCandidate (:promotionCandidate supplied-composition))) (do
  (println (red "promotion flag conflicts with composition.promotionCandidate"))
  (System/exit 1))
  (and target (str/blank? target)) (do
  (println (red "--target requires a non-empty account target"))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :taskGrades])) selected-grade)) (do
  (println (red (str "invalid taskGrade: " selected-grade)))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :topologies])) selected-topology)) (do
  (println (red (str "invalid topology: " selected-topology)))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :capabilityFloors])) selected-capability-floor)) (do
  (println (red (str "invalid capabilityFloor: " selected-capability-floor)))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :serviceClasses])) selected-service-class)) (do
  (println (red (str "invalid serviceClass: " selected-service-class)))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :deliberations])) selected-reasoning)) (do
  (println (red (str "invalid reasoning: " selected-reasoning)))
  (System/exit 1))
  (not (contains? (set (get-in catalog [:vocabulary :postures])) selected-posture)) (do
  (println (red (str "invalid posture: " selected-posture)))
  (System/exit 1))
  (not (map? selected-composition)) (do
  (println (red "composition must be a JSON object"))
  (System/exit 1))
  unknown-composition-fields (do
  (println (red (str "composition contains unknown fields: " (str/join ", " (map name unknown-composition-fields)))))
  (System/exit 1))
  (and (= "template" composition-kind) (nil? (get dt (:id selected-composition)))) (do
  (println (red (str "unknown template composition.id " (:id selected-composition))))
  (System/exit 1))
  (and template? (not (valid-string-list? declared-overrides false))) (do
  (println (red "template composition.overrides must be an array of unique routing-axis names"))
  (System/exit 1))
  (and template? (some (fn [%1] (not (contains? routing-override-fields %1))) declared-overrides)) (do
  (println (red (str "composition.overrides may contain only: " (str/join ", " (sort routing-override-fields)))))
  (System/exit 1))
  (and template? (not= (set actual-overrides) (set declared-overrides))) (do
  (println (red (str "composition.overrides must exactly record changed template axes: " (if (seq actual-overrides) (str/join ", " actual-overrides) "none"))))
  (System/exit 1))
  (and template? (seq actual-overrides) (not (non-empty-string? (:overrideReason selected-composition)))) (do
  (println (red (str "template axis override requires --override-reason (changed: " (str/join ", " actual-overrides) ")")))
  (System/exit 1))
  (and template? (empty? actual-overrides) (contains? selected-composition :overrideReason)) (do
  (println (red "unchanged preset must not carry --override-reason"))
  (System/exit 1))
  (and bespoke? (not (boolean? (:promotionCandidate selected-composition)))) (do
  (println (red "bespoke composition.promotionCandidate must be explicit boolean"))
  (System/exit 1))
  (and bespoke? (not= bespoke-contract-fields contract-fields)) (do
  (println (red "bespoke composition.contract must contain exactly responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, and report"))
  (System/exit 1))
  (and bespoke? (some (fn [%1] (not (valid-contract-text? (get-in selected-composition [:contract %1])))) [:responsibility :deliverable :report])) (do
  (println (red "bespoke composition.contract requires non-empty responsibility, deliverable, and report"))
  (System/exit 1))
  (and bespoke? (some (fn [%1] (not (valid-contract-string-list? (get-in selected-composition [:contract %1])))) [:mayDecide :mustEscalate :doneWhen])) (do
  (println (red "bespoke composition.contract requires non-empty mayDecide, mustEscalate, and doneWhen lists"))
  (System/exit 1))
  (and bespoke? (or (not (valid-contract-string-list? selected-capabilities)) (some (fn [%1] (not (contains? capability-values %1))) normalized-selected-capabilities))) (do
  (println (red "bespoke composition.contract capabilities must be non-empty and canonical"))
  (System/exit 1))
  capability-problem (do
  (println (red capability-problem))
  (System/exit 1))
  :else (let [canonical-contract (if bespoke? (do
  (canonical-bespoke-contract (:contract selected-composition))))
   _notify-warning (warn-unarmed-notify! notify)
   contract-sha256 (if canonical-contract (do
  (bespoke-contract-sha256 (:contract selected-composition))))
   spawn-composition (if selected-request selected-composition (if bespoke? (assoc selected-composition :contract canonical-contract) selected-composition))
   routing-metadata {:role selected-role :taskGrade selected-grade :domainRequirements selected-domains :topology selected-topology :capabilityFloor selected-capability-floor :serviceClass selected-service-class :reasoning selected-reasoning :posture selected-posture :composition spawn-composition}
   _receipt (preflight-routing-economics! routing-metadata routing-assessment pin-evidence provider target model dry?)
   _capabilities (require-pinned-provider-capabilities! provider target normalized-selected-capabilities)
   struggle-policy (resolve-struggle-policy! selected-topology)
   catalog-model (:model base)
   effective-model (or model (if (and (not (:semantic base)) (not (:orchestration-preset base))) (do
  catalog-model)))
   synthetic-effort (:effort base)
   synthetic-reasoning (:reasoning base)
   orchestration-preset (:orchestration-preset base)
   semantic (:semantic base)
   delegate-binding (cond
  *delegate-request* (resolve-delegate-referent! *delegate-request* dry?)
  (= "orchestrator" (north.topology-authority/current-topology)) (resolve-recursive-child-referent! prompt dry?))
   effective-prompt (if delegate-binding (delegate-brief *delegate-request* delegate-binding) prompt)
   aid (north.spawn-process/create-agent-id "lane")
   env (cond-> {"AGENT_ID" aid "NORTH_STAFFING_SOURCE" "file" "NORTH_STRUGGLE_POLICY_EXPECTED" (:canonical struggle-policy)} selected-role (assoc "AGENT_IDENTITY_ROLE" selected-role) selected-grade (assoc "AGENT_TASK_GRADE" selected-grade) selected-role (assoc "AGENT_DOMAIN_REQUIREMENTS" (json/generate-string selected-domains)) selected-topology (assoc "AGENT_TOPOLOGY" selected-topology) selected-capability-floor (assoc "AGENT_CAPABILITY_FLOOR" selected-capability-floor) selected-service-class (assoc "AGENT_SERVICE_CLASS" selected-service-class) selected-role (assoc "AGENT_ROLE" selected-role) selected-posture (assoc "AGENT_POSTURE" selected-posture) spawn-composition (assoc "AGENT_COMPOSITION" (json/generate-string spawn-composition)) effective-model (assoc "AGENT_MODEL" effective-model) selected-reasoning (assoc "AGENT_REASONING" selected-reasoning) provider (assoc "AGENT_PROVIDER" provider) target (assoc "AGENT_TARGET" target) routing-assessment (assoc "AGENT_ROUTING_ASSESSMENT" (json/generate-string routing-assessment)) pin-evidence (assoc "NORTH_ROUTING_PIN_EVIDENCE" (json/generate-string pin-evidence)) notify (assoc "AGENT_COORDINATOR" notify) referent (assoc "AGENT_REFERENT" referent "AGENT_REFERENT_PROVENANCE" "exact") ad-hoc? (assoc "AGENT_REFERENT_PROVENANCE" "ad-hoc") delegate-binding (assoc "NORTH_DELEGATE_REFERENT_ID" (:id delegate-binding)))
   immediate-coordinator (or notify (System/getenv "AGENT_ID") (System/getenv "NORTH_AGENT_ID"))
   child-env (north.managed-child-env/child (into {} (System/getenv)) immediate-coordinator env)
   spawn-js (str NORTH "/sdk/src/spawn.js")
   display-env (cond-> (dissoc env "NORTH_STRUGGLE_POLICY_EXPECTED") bespoke? (assoc "AGENT_COMPOSITION" "REDACTED_BESPOKE_CONTRACT") routing-assessment (assoc "AGENT_ROUTING_ASSESSMENT" "RECORDED") pin-evidence (assoc "NORTH_ROUTING_PIN_EVIDENCE" "RECORDED"))
   envs (str/join " " (map (fn [[k v]] (str k "=" v)) (sort display-env)))
   dry-route (dry-resolved-route provider selected-capability-floor effective-model selected-reasoning)
   fallback-base (into {} (remove (comp nil? val) {"kind" "lane" "role" selected-role "provider" (or (:provider dry-route) provider "auto") "provider_target" (or target (:provider dry-route) provider "auto") "live_input" (if (= "anthropic" (or (:provider dry-route) provider)) "streaming" "unsupported") "live_input_state" (if (= "anthropic" (or (:provider dry-route) provider)) "pending" "frozen") "live_input_epoch" (str (java.util.UUID/randomUUID)) "model" (or (:model dry-route) "unresolved") "effort" (or (:effort dry-route) selected-reasoning) "composition_kind" (:kind spawn-composition) "composition_id" (:id spawn-composition) "composition_overrides" (if (= "template" (:kind spawn-composition)) (do
  (json/generate-string (:overrides spawn-composition)))) "composition_override_reason" (if (= "template" (:kind spawn-composition)) (do
  (:overrideReason spawn-composition))) "bespoke_reason" (if (= "bespoke" (:kind spawn-composition)) (do
  (:bespokeReason spawn-composition))) "nearest_template" (if (= "bespoke" (:kind spawn-composition)) (do
  (:nearestTemplate spawn-composition))) "promotion_candidate" (if (= "bespoke" (:kind spawn-composition)) (do
  (str (:promotionCandidate spawn-composition)))) "composition_contract_sha256" contract-sha256 "composition_contract_fingerprint_version" (if contract-sha256 (do
  bespoke-fingerprint-version)) "composition_contract_fingerprint_domain" (if contract-sha256 (do
  bespoke-fingerprint-domain)) "repo" (current-repo) "goal" effective-prompt "spawned_at" (str (java.time.Instant/now)) "display_handle" "dry-run" "display_name" "dry-run"}))
   fallback-facts (assoc fallback-base "identity_manifest_sha256" (north.agent-provenance/manifest-sha256 fallback-base))]
  (println (dim "# orchestration dials for role") (bold invoked-role) (dim "->") (str "grade=" selected-grade " floor=" selected-capability-floor " service=" selected-service-class " reasoning=" selected-reasoning (if (and (not semantic) (not orchestration-preset) model) (do
  (str " model=" model))) (if selected-role (do
  (str " role=" selected-role))) (if selected-composition (do
  (str " selection=" (orchestration-provenance fallback-facts)))) (if target (do
  (str " target=" target))) (if selected-posture (do
  (str " posture=" selected-posture))) (if selected-topology (do
  (str " topology=" selected-topology))) (if (seq selected-domains) (do
  (str " domains=" (str/join "," selected-domains))))))
  (println (dim "# struggle observer ->") (str "policy=" (:version struggle-policy) " topology=" (:topology struggle-policy) " error-streak=" (:errorStreak struggle-policy) " loop-repeat=" (:loopRepeat struggle-policy) " loop-window=" (:loopWindow struggle-policy) " no-progress-turns=" (:noProgressTurns struggle-policy)))
  (if bespoke? (do
  (println (dim "# bespoke evidence ->") (str "version=" bespoke-fingerprint-version " domain=" bespoke-fingerprint-domain " sha256=" contract-sha256 " capabilities=" (str/join "," (:capabilities canonical-contract)) " reason=recorded"))))
  (echo-cmd envs POLICY-BUN "run" spawn-js (str "\"" effective-prompt "\""))
  (if dry? (do
  (println (ylw "[dry-run]") "not executed. semantic handle would be" (bold (semantic-handle aid fallback-facts)))
  (println "control:" (dim aid))
  (if (and selected-capability-floor (nil? dry-route)) (do
  (println "selected capability floor:" (bold selected-capability-floor) (dim "(Agent Machinery resolves the execution plan at spawn)"))))) (let [log (io/file AGENT-LOGDIR (str aid ".log"))]
  (.mkdirs (.getParentFile log))
  (let [process (north.spawn-process/launch-detached! [POLICY-BUN "run" spawn-js effective-prompt] child-env log)
   startup (north.spawn-process/await-startup process aid log agent-facts-one agent-online? :timeout-ms (north.spawn-process/startup-timeout-for-capabilities normalized-selected-capabilities))]
  (case (:status startup)
    :ready (do
  (println (grn "spawned") (bold (:handle startup)))
  (println "control:" (dim aid))
  (println "watch:" (cyn (str "north agent watch " aid))))
    :completed (do
  (println (grn "completed") (bold (:handle startup)) (dim (str "outcome=" (:outcome startup))))
  (println "control:" (dim aid))
  (println "log:" (dim (str log))))
    (do
  (binding [*out* *err*]
  (println (red (north.spawn-process/failure-message startup))))
  (System/exit 1))))))))))

(def cmd-spawn cmd-spawn!)

(defn- cmd-spawn-selected!
  "Admit one already-selected Agent Machinery routing request without encoding\n   any of its nine fields or its validated assessment as CLI flags. Only\n   North-owned runtime controls remain in CONTROLS." [routing-request routing-assessment task controls]
  (binding [*selected-routing-request* routing-request
   *selected-routing-assessment* routing-assessment]
  (cmd-spawn (into [task] controls))))

(def delegate-usage (str "north agent delegate \"<intent>\" [--role <worker-role> | --composite] " "[--referent <id>] [--context <file>] [spawn options]\n" "       north agent delegate --handoff <session-hard-cap.json> " "[--role <worker-role> | --composite] [spawn options]"))

(defn- delegate-die [message]
  (println (red message))
  (println (red "usage:") delegate-usage)
  (System/exit 1))

(def delegation-run-design-result-version "north:delegation-run-design-result:v1")

(def delegation-run-design-result-fields #{:version :routingRequest :routingAssessment})

(def delegation-run-design-timeout-ms (* 10 60 1000))

(defn- bounded-delegation-diagnostic [value]
  (let [text (str/trim (str (or value "")))]
  (if (seq text) (do
  (subs text 0 (min 2000 (count text)))))))

(defn- select-delegation-run-design!
  "Ask Agent Machinery to select and validate one portable route. North owns\n   only the single deliberation transport and carries the validated assessment\n   beside the exact nine-field request for concrete admission." [intent context]
  (let [payload (cond-> {:intent intent} (some? context) (assoc :context context))
   result (run [POLICY-BUN "run" DELEGATION-RUN-DESIGN-TRANSPORT] :timeout delegation-run-design-timeout-ms :in (json/generate-string payload))
   output (str/trim (str (or (:out result) "")))]
  (if (not (:ok result)) (do
  (delegate-die (or (bounded-delegation-diagnostic (:err result)) (if (:timeout result) (do
  (str "delegation run-design selection exceeded " delegation-run-design-timeout-ms "ms"))) (if (:error result) (do
  (str "delegation run-design selection could not start: " (:error result)))) (str "delegation run-design selection exited " (:exit result))))))
  (let [selected (try
  (json/parse-string output true)
  (catch Exception _
    nil))
   routing-request (:routingRequest selected)
   routing-assessment (:routingAssessment selected)]
  (if (not (and (map? selected) (= delegation-run-design-result-fields (set (keys selected))) (= delegation-run-design-result-version (:version selected)) (map? routing-request) (= routing-request-fields (set (keys routing-request))) (map? routing-assessment) (= "minimum-sufficient-v1" (:version routing-assessment)))) (do
  (delegate-die "delegation run-design transport returned a malformed result envelope")))
  {:routing-request routing-request :routing-assessment routing-assessment})))

(def delegate-referent-id-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

(def delegate-referent-title-max-utf8-bytes 160)

(def delegate-handoff-max-bytes (* 64 1024))

(def delegate-handoff-hard-cap-ms (* 60 60 1000))

(def delegate-handoff-next-action "Resume only this deliverable; inspect the named referent, worktree, branch, and session transcript before editing.")

(def delegate-handoff-required-keys #{:version :reason :writtenAt :hardCapMs :agentId :referentId :goal :repo :nextAction :completionClaimed})

(def delegate-handoff-optional-keys #{:worktree :branch})

(def delegate-handoff-private-permissions #{java.nio.file.attribute.PosixFilePermission/OWNER_READ java.nio.file.attribute.PosixFilePermission/OWNER_WRITE})

(def delegate-routing-override-flags #{"--taskGrade" "--task-grade" "--domain" "--topology" "--capability-floor" "--service-class" "--reasoning" "--deliberation" "--posture" "--composition" "--rationale" "--nearest" "--contract" "--override-reason" "--assessment" "--routing-assessment" "--promotion-candidate" "--nominate" "--no-promotion-candidate"})

(def capture-receipt-keys #{:id :referent :title :path :expected :committed :complete :reason})

(defn- canonical-delegate-referent [raw]
  (let [value (some-> raw str)
   bare (if value (do
  (str/replace-first value #"^@" "")))]
  (if (and value (= value (str/trim value)) (not (str/starts-with? (str bare) "@")) (<= (count (str bare)) 512) (re-matches delegate-referent-id-pattern (str bare))) (do
  bare))))

(defn- normalize-delegate-referent [raw]
  (or (canonical-delegate-referent raw) (delegate-die "--referent must be a bare or single-@ ASCII North referent id")))

(defn- delegate-handoff-text? [value]
  (and (string? value) (not (str/blank? value)) (not (str/includes? value "\u0000"))))

(defn- delegate-handoff-absolute-path? [value]
  (and (delegate-handoff-text? value) (= value (str/trim value)) (try
  (.isAbsolute (java.nio.file.Paths/get value (make-array String 0)))
  (catch Exception _
    false))))

(defn- delegate-handoff-written-at? [value]
  (and (string? value) (boolean (re-matches #"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$" value)) (try
  (java.time.Instant/parse value)
  true
  (catch java.time.format.DateTimeParseException _
    false))))

(defn- decode-delegate-handoff-utf8! [bytes path]
  (try
  (let [decoder (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
  (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
  (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
  (str (.decode decoder (java.nio.ByteBuffer/wrap bytes))))
  (catch java.nio.charset.CharacterCodingException _
    (delegate-die (str "session hard-cap artifact is not valid UTF-8: " path)))))

(defn- read-delegate-handoff! [raw-path]
  (let [file (io/file raw-path)
   path (.toPath file)
   display-path (.getPath file)
   no-follow (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])]
  (if (not (java.nio.file.Files/exists path no-follow)) (do
  (delegate-die (str "session hard-cap artifact not found: " display-path))))
  (let [attributes (try
  (java.nio.file.Files/readAttributes path java.nio.file.attribute.BasicFileAttributes no-follow)
  (catch java.io.IOException _
    (delegate-die (str "cannot inspect session hard-cap artifact: " display-path))))]
  (if (or (java.nio.file.Files/isSymbolicLink path) (not (.isRegularFile attributes))) (do
  (delegate-die (str "session hard-cap artifact must be a regular non-symlink file: " display-path))))
  (if (not (<= 1 (.size attributes) delegate-handoff-max-bytes)) (do
  (delegate-die (str "session hard-cap artifact must be between 1 and " delegate-handoff-max-bytes " bytes")))))
  (let [permissions (try
  (set (java.nio.file.Files/getPosixFilePermissions path no-follow))
  (catch UnsupportedOperationException _
    (delegate-die "session hard-cap adoption requires a POSIX private-file boundary")))]
  (if (not (= delegate-handoff-private-permissions permissions)) (do
  (delegate-die (str "session hard-cap artifact must have mode 0600: " display-path)))))
  (let [bytes (try
  (java.nio.file.Files/readAllBytes path)
  (catch java.io.IOException _
    (delegate-die (str "cannot read session hard-cap artifact: " display-path))))]
  (if (not (<= 1 (alength bytes) delegate-handoff-max-bytes)) (do
  (delegate-die (str "session hard-cap artifact must be between 1 and " delegate-handoff-max-bytes " bytes"))))
  (let [raw (decode-delegate-handoff-utf8! bytes display-path)
   document (try
  (json/parse-string raw true)
  (catch Exception _
    (delegate-die (str "session hard-cap artifact is not valid JSON: " display-path))))
   keys-present (if (map? document) (do
  (set (keys document))))
   allowed-keys (set/union delegate-handoff-required-keys delegate-handoff-optional-keys)
   canonical-referent (if (map? document) (do
  (canonical-delegate-referent (:referentId document))))]
  (if (not (and (map? document) (set/subset? delegate-handoff-required-keys keys-present) (set/subset? keys-present allowed-keys) (= 1 (:version document)) (= "session_hard_cap" (:reason document)) (delegate-handoff-written-at? (:writtenAt document)) (= delegate-handoff-hard-cap-ms (:hardCapMs document)) (valid-control-id? (:agentId document)) (= (:referentId document) canonical-referent) (delegate-handoff-text? (:goal document)) (delegate-handoff-absolute-path? (:repo document)) (or (not (contains? document :worktree)) (delegate-handoff-absolute-path? (:worktree document))) (or (not (contains? document :branch)) (delegate-handoff-text? (:branch document))) (= delegate-handoff-next-action (:nextAction document)) (false? (:completionClaimed document)))) (do
  (delegate-die "session hard-cap artifact does not match North's incomplete v1 handoff contract")))
  {:task (:goal document) :referent canonical-referent :context (str/trim raw)}))))

(defn- structured-facts? [facts]
  (and (sequential? facts) (every? (fn [%1] (and (map? %1) (= #{:predicate :value} (set (keys %1))) (string? (:predicate %1)) (string? (:value %1)))) facts)))

(defn- structured-subject-facts [subject]
  (mapv (fn [[predicate value]] {:predicate predicate :value value}) (north.coord/show-rows! (Integer/parseInt PORT) subject)))

(defn- parse-referent-facts! [id facts]
  (if (not (structured-facts? facts)) (do
  (delegate-die (str "referent @" id " returned an invalid structured fact projection"))))
  (let [kinds (mapv (fn [fact] (:value fact)) (filter (fn [%1] (= "entity_kind" (:predicate %1))) facts))
   titles (mapv (fn [fact] (:value fact)) (filter (fn [%1] (= "title" (:predicate %1))) facts))]
  (if (not (and (= ["referent"] kinds) (= 1 (count titles)) (not (str/blank? (first titles))))) (do
  (delegate-die (str "referent @" id " is not an exact title-bearing North Referent"))))
  {:id id :title (first titles) :facts facts :committed? (boolean (some (fn [%1] (= "committed" (:predicate %1))) facts)) :done-when (mapv (fn [fact] (:value fact)) (filter (fn [%1] (= "done_when" (:predicate %1))) facts))}))

(defn- read-delegate-referent! [raw]
  (let [id (normalize-delegate-referent raw)
   facts (try
  (structured-subject-facts (str "@" id))
  (catch Exception e
    (delegate-die (str "cannot prove delegate referent @" id " through North's structured read boundary: " (or (not-empty (str (.getMessage e))) (.getName (class e)))))))]
  (parse-referent-facts! id facts)))

(defn- fact-set [facts]
  (reduce (fn [acc {:keys [predicate value]}] (update acc predicate (fnil conj #{}) value)) {} facts))

(defn- utf8-byte-count [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn- utf8-prefix [value max-bytes]
  (loop [end 0]
  (if (>= end (.length value)) value (let [next (+ end (long (Character/charCount (.codePointAt value end))))]
  (if (> (utf8-byte-count (subs value 0 next)) max-bytes) (subs value 0 end) (recur next))))))

(defn delegate-referent-title [task]
  (let [lines (str/split (str task) #"\R" -1)
   line (or (first (remove str/blank? (map str/trim lines))) "Delegated task")
   collapsed (-> line (str/replace #"[\p{javaWhitespace}\p{Z}]+" " ") str/trim)]
  (utf8-prefix collapsed delegate-referent-title-max-utf8-bytes)))

(defn- managed-referent-binding []
  (let [ambient-run (System/getenv "NORTH_RUN_ID")
   referent (System/getenv "NORTH_REFERENT_ID")
   capability (System/getenv "NORTH_RUN_CAPABILITY")
   agent (System/getenv "AGENT_ID")
   values [ambient-run referent capability agent]
   present (mapv (fn [%1] (boolean (known %1))) values)]
  (if (every? true? present) (try
  (let [run-id (canonical-delegate-referent ambient-run)
   referent-id (canonical-delegate-referent referent)
   rows (if (and run-id referent-id) (do
  (structured-subject-facts (str "@" run-id))))
   facts (if (structured-facts? rows) (do
  (fact-set rows)))
   reporter (str "@agent:" (str/replace-first agent #"^@?agent:" ""))]
  (if (and run-id referent-id facts (north.terminal-projection/run-reservation-valid? facts) (= #{(str "@" referent-id)} (get facts "run_reservation_referent")) (= #{reporter} (get facts "run_reservation_agent")) (= #{(north.terminal-projection/sha256 capability)} (get facts "run_capability_sha256"))) {:kind :complete :referent referent-id} {:kind :none :residue? true}))
  (catch Exception _
    {:kind :none :residue? true})) {:kind :none :residue? (some true? present)})))

(def delegate-capture-timeout-ms 180000)

(defn- capture-delegate-referent! [task]
  (let [title (delegate-referent-title task)
   capture-env (assoc (into {} (System/getenv)) "NORTH_CAPTURE_STRUCTURED" "1")
   result (run [NORTH-CLI "work" "capture" title] :timeout delegate-capture-timeout-ms :env capture-env)]
  (if (not (:ok result)) (do
  (delegate-die "North could not capture a durable delegate referent")))
  (let [receipt (try
  (json/parse-string (str/trim (:out result)) true)
  (catch Exception _
    (delegate-die "North capture did not return its exact structured receipt")))]
  (if (not (and (map? receipt) (= capture-receipt-keys (set (keys receipt))) (string? (:id receipt)) (string? (:referent receipt)) (string? (:title receipt)) (string? (:path receipt)) (integer? (:expected receipt)) (integer? (:committed receipt)) (boolean? (:complete receipt)) (string? (:reason receipt)))) (do
  (delegate-die "North capture returned a malformed structured receipt")))
  (let [id (normalize-delegate-referent (:id receipt))]
  (if (not (and (:complete receipt) (= "captured" (:reason receipt)) (= (str "@" id) (:referent receipt)) (= title (:title receipt)) (pos? (:expected receipt)) (= (:expected receipt) (:committed receipt)))) (do
  (delegate-die "North capture was partial; delegate spawn refused before provider execution")))
  (let [referent (read-delegate-referent! id)]
  (if (not (and (= title (:title referent)) (:committed? referent))) (do
  (delegate-die "captured delegate referent failed exact title/commit readback")))
  (assoc referent :source :captured))))))

(def delegate-link-timeout-ms 45000)

(defn- capture-recursive-child-referent! [task parent]
  (let [captured (capture-delegate-referent! task)
   child (:id captured)
   linked (run [NORTH-CLI "fact" "tell" child "part_of" parent] :timeout delegate-link-timeout-ms)]
  (if (not (:ok linked)) (do
  (run [NORTH-CLI "fact" "tell" child "abandoned" "recursive child binding failed before provider execution"] :timeout delegate-link-timeout-ms)
  (delegate-die (str "North could not link recursive child @" child " part_of @" parent))))
  (let [verified (read-delegate-referent! child)
   parents (->> (:facts verified) (filter (fn [%1] (= "part_of" (:predicate %1)))) (map (fn [fact] (:value fact))) set)]
  (if (not (= #{(str "@" parent)} parents)) (do
  (run [NORTH-CLI "fact" "tell" child "abandoned" "recursive child link failed exact readback before provider execution"] :timeout delegate-link-timeout-ms)
  (delegate-die (str "recursive child @" child " did not read back exact parent @" parent))))
  (assoc verified :source :recursive-child :parent parent))))

(defn resolve-recursive-child-referent! [task dry?]
  (let [{:keys [kind referent residue?]} (managed-referent-binding)]
  (if (not (= kind :complete)) (do
  (if residue? (do
  (binding [*out* *err*]
  (println (ylw "recursive spawn found unverified parent run residue")))))
  (delegate-die "recursive orchestrator spawn requires its exact parent run/referent reservation")))
  (if dry? {:id "recursive-child-on-execution" :title (delegate-referent-title task) :facts [{:predicate "entity_kind" :value "referent"} {:predicate "title" :value (delegate-referent-title task)} {:predicate "committed" :value "dry-run"} {:predicate "part_of" :value (str "@" referent)}] :committed? true :done-when [] :source :dry-recursive-child :parent referent} (capture-recursive-child-referent! task referent))))

(defn resolve-delegate-referent! [{:keys [task explicit-referent handoff?]} dry?]
  (cond
  explicit-referent (let [referent (read-delegate-referent! explicit-referent)
   terminal-predicates (->> (:facts referent) (map (fn [fact] (:predicate fact))) (filter (fn [predicate] (contains? #{"outcome" "abandoned"} predicate))) set)]
  (if (and handoff? (seq terminal-predicates)) (do
  (delegate-die (str "session hard-cap artifact referent @" (:id referent) " is already terminal (" (str/join ", " (sort terminal-predicates)) ")"))))
  (assoc referent :source :explicit))
  :else (let [{:keys [kind referent residue?]} (managed-referent-binding)]
  (case kind
    :complete (if dry? (resolve-recursive-child-referent! task true) (capture-recursive-child-referent! task referent))
    :none (do
  (if residue? (do
  (binding [*out* *err*]
  (println (ylw "ignoring unverified ambient North run/referent residue; a fresh delegate referent is required")))))
  (if dry? {:id "capture-on-execution" :title task :facts [{:predicate "entity_kind" :value "referent"} {:predicate "title" :value task} {:predicate "committed" :value "dry-run"}] :committed? true :done-when [] :source :dry-capture} (capture-delegate-referent! task)))
    (throw (IllegalArgumentException. (str "No matching clause: " kind)))))))

(defn cmd-bind-child-referent! [[task & extra]]
  (north.topology-authority/require-coordination! "bind recursive child referent")
  (if (or (nil? task) (seq extra)) (do
  (delegate-die "internal bind-child-referent requires exactly one task argument")))
  (let [{:keys [id parent]} (resolve-recursive-child-referent! task false)]
  (println (json/generate-string {:referent id :parent parent}))))

(defn- parse-delegate-args [args]
  (let [handoff? (= "--handoff" (first args))
   handoff (if handoff? (do
  (second args)))
   _ (if (and handoff? (or (nil? handoff) (str/starts-with? (str handoff) "--"))) (do
  (delegate-die "--handoff requires a session hard-cap artifact")))
   task (if (not handoff?) (do
  (first args)))
   _ (if (and (not handoff?) (or (nil? task) (str/starts-with? (str task) "--"))) (do
  (delegate-die "delegate requires one quoted intent or --handoff")))
   remaining (if handoff? (nnext args) (rest args))]
  (loop [xs remaining
   parsed (cond-> {:task task :forward []} handoff? (assoc :handoff handoff))]
  (let [x (first xs)]
  (if x (case x
    "--role" (let [role (second xs)]
  (if (or (nil? role) (str/starts-with? (str role) "--")) (do
  (delegate-die "--role requires a Orchestration worker role")))
  (if (:mode parsed) (do
  (delegate-die "choose exactly one delegation mode: --role or --composite")))
  (recur (nnext xs) (assoc parsed :mode :atomic :role role)))
    "--composite" (do
  (if (:mode parsed) (do
  (delegate-die "choose exactly one delegation mode: --role or --composite")))
  (recur (rest xs) (assoc parsed :mode :composite)))
    "--context" (let [path (second xs)]
  (if (or (nil? path) (str/starts-with? (str path) "--")) (do
  (delegate-die "--context requires a brief file")))
  (if (:handoff parsed) (do
  (delegate-die "--handoff supplies its exact context; omit --context")))
  (recur (nnext xs) (assoc parsed :context path)))
    "--referent" (let [referent (second xs)]
  (if (or (nil? referent) (str/starts-with? (str referent) "--")) (do
  (delegate-die "--referent requires a North referent id")))
  (if (:handoff parsed) (do
  (delegate-die "--handoff supplies its exact referent; omit --referent")))
  (if (:referent parsed) (do
  (delegate-die "delegate accepts exactly one --referent")))
  (recur (nnext xs) (assoc parsed :referent referent)))
    "--handoff" (delegate-die "--handoff must replace the task and may appear exactly once")
    (recur (rest xs) (update parsed :forward conj x))) parsed)))))

(defn delegate-brief [{:keys [task mode context]} {:keys [id committed? done-when]}]
  (let [context-block (if context (do
  (str "CONTEXT BRIEF:\n" context "\n\n")))
   proof-block (cond
  (seq done-when) (str "North has prebound this lane and its immutable starting done_when set to @" id ". Run each exact bar, then record its observation with " "`north-delivery-evidence record \"<exact bar>\" \"<observed result>\"`; " "provider success without those records is not delivery evidence.")
  committed? (str "North has prebound this accepted, currently barless referent to @" id ". FIRST ACT: define exact probe + expected-result criteria with " "`north fact tell " id " done_when \"<probe + expected result>\"`. " "After each probe, use `north-delivery-evidence record \"<exact bar>\" " "\"<observed result>\"`; provider success alone is not delivery evidence.")
  :else (str "North has prebound this title-bearing referent to @" id ". Record exact done_when criteria before claiming completion, then use " "`north-delivery-evidence record \"<exact bar>\" \"<observed result>\"` after each probe."))]
  (str context-block "DELEGATE TASK: " task "\n\n" "NORTH DELIVERY CONTRACT: " proof-block "\n\n" (if (= mode :composite) (str "COMPOSITE INTAKE: @" id " is the aggregate reduction/checkpoint referent. Create a distinct " "title-bearing child referent linked `part_of @" id "` for every terminal piece, and bind each child run to its own referent; " "never make workers prove the aggregate bar set. Keep the North listener/" "continuation live, checkpoint each result as it arrives, and reconcile " "every child before publishing the aggregate outcome.") (str "ATOMIC INTAKE: use @" id " as the single durable work/evidence referent and return one evidence-backed result.")))))

(defn cmd-delegate! [args]
  (north.topology-authority/require-coordination! "delegate")
  (let [{:keys [task mode role context referent handoff forward]} (parse-delegate-args args)
   expert-mode mode
   adopted (if handoff (do
  (read-delegate-handoff! handoff)))
   task (or (:task adopted) task)
   referent (or (:referent adopted) referent)
   ctx-file context
   ctx (or (:context adopted) (if ctx-file (do
  (let [f (io/file ctx-file)]
  (if (not (.exists f)) (do
  (delegate-die (str "context file not found: " ctx-file))))
  (str/trim (slurp f))))))
   routing-override (if (not expert-mode) (do
  (some delegate-routing-override-flags forward)))
   _ (if routing-override (do
  (delegate-die (str routing-override " is a routing override; use --role or --composite to bypass intent selection"))))
   bare-spawn (if (not expert-mode) (do
  (parse-spawn-args (into [task] forward))))
   _ (if (and bare-spawn (not= [task] (:positionals bare-spawn))) (do
  (delegate-die "bare delegate accepts exactly one intent and runtime controls")))
   selected (if (not expert-mode) (do
  (select-delegation-run-design! task ctx)))
   routing-request (:routing-request selected)
   routing-assessment (:routing-assessment selected)
   mode (or expert-mode (case (:topology routing-request)
    "worker" :atomic
    "orchestrator" :composite
    (delegate-die "Agent Machinery selected an unknown delegation topology")))
   spawn-role (if expert-mode (do
  (if (= expert-mode :composite) "director" role)))
   parsed-spawn (if expert-mode (do
  (parse-spawn-args (into [spawn-role task] forward))))
   effective-topology (if parsed-spawn (do
  (resolved-spawn-topology parsed-spawn)))
   _ (if (and (= expert-mode :atomic) (= "orchestrator" effective-topology)) (do
  (delegate-die "--role is an atomic terminal-worker handoff; use --composite for orchestrator work")))
   inherited-notify (and (not (contains? (set forward) "--notify")) (System/getenv "NORTH_NOTIFY"))
   controls (cond-> forward inherited-notify (into ["--notify" inherited-notify]))]
  (binding [*delegate-request* {:task task :mode mode :context ctx :explicit-referent referent :handoff? (boolean adopted)}]
  (if expert-mode (cmd-spawn (into [spawn-role task] controls)) (cmd-spawn-selected! routing-request routing-assessment task controls)))))

(def watch-usage "north agent watch <agent-id> [--control]")

(def wire-watch-version "north:wire:v2")

(def wire-watch-kinds #{"run.started" "run.progress" "message.recorded" "model-call.started" "model-call.completed" "tool.admitted" "tool.progress" "tool.terminal" "artifact.published" "resource.pressure" "run.terminated"})

(def max-watch-json-line-bytes (* 2 1024 1024))

(def max-watch-output-columns 180)

(def max-watch-field-codepoints 40)

(def max-watch-output-codepoints (quot max-watch-output-columns 2))

(defn watch-safe-text
  "Collapse terminal controls and whitespace, then bound one display field by\n  Unicode code points. The final line is bounded again after composition."
  ([value]
    (watch-safe-text value max-watch-field-codepoints))
  ([value limit]
    (let [bound (max 1 (long limit))
   source (str (or value ""))
   sampled (.toArray (.limit (.codePoints source) (inc bound)))
   truncated? (> (alength sampled) bound)
   retained (if truncated? (dec bound) bound)
   bounded (if (> (alength sampled) retained) (java.util.Arrays/copyOf sampled retained) sampled)
   cleaned (int-array (map (fn [codepoint] (if (or (Character/isISOControl codepoint) (Character/isWhitespace codepoint) (= Character/FORMAT (Character/getType codepoint))) (int \space) codepoint)) bounded))
   text (str/trim (str/replace (String. cleaned 0 (alength cleaned)) #" +" " "))]
  (str text (if truncated? (do
  "…"))))))

(defn- watch-display-path [value]
  (let [raw (str (or value ""))
   home (if (and HOME (not (str/blank? HOME))) (do
  (try
  (.getCanonicalPath (io/file HOME))
  (catch Exception _
    HOME))))
   home-prefix (if home (do
  (str home java.io.File/separator)))
   shortened (cond
  (= raw home) "~"
  (and home-prefix (str/starts-with? raw home-prefix)) (str "~/" (subs raw (count home-prefix)))
  :else raw)]
  (watch-safe-text shortened max-watch-field-codepoints)))

(defn- watch-json-summary [value]
  (try
  (watch-safe-text (json/generate-string value))
  (catch Exception _
    "<unrenderable>")))

(defn- watch-kv [label value]
  (if (some? value) (do
  (str " " label "=" (watch-safe-text value)))))

(defn- watch-event-prefix [event]
  (str "[" (if (and (integer? (:sequence event)) (not (neg? (:sequence event)))) (:sequence event) "?") "] "))

(defn- watch-progress-detail [progress]
  (str (watch-kv "action" (:currentAction progress)) (watch-kv "retry" (some-> (:retry progress) :attempt)) (watch-kv "fallback" (some-> (:fallback progress) :reason)) (watch-kv "compactions" (:compactions progress))))

(defn- render-known-wire-event [event]
  (let [kind (:kind event)
   prefix (watch-event-prefix event)
   rendered (case kind
    "run.started" (str prefix "▶ run started" (watch-kv "owner" (:owner event)))
    "run.progress" (str prefix "… run " (watch-safe-text (:lifecycle event)) (watch-progress-detail (:progress event)))
    "message.recorded" (str prefix "message " (watch-safe-text (:role event)) "/" (watch-safe-text (:stage event)) (watch-kv "content" (if (contains? event :content) (do
  (watch-json-summary (:content event))))))
    "model-call.started" (str prefix "model ▶ " (watch-safe-text (get-in event [:model :provider])) (watch-kv "tier" (get-in event [:model :tier])) (watch-kv "effort" (:effort event)) (watch-kv "attempt" (:attempt event)))
    "model-call.completed" (str prefix "model " (if (= "succeeded" (:status event)) "✓" "✗") " " (watch-safe-text (:status event)) (watch-kv "origin" (:origin event)) (watch-kv "input" (get-in event [:usage :lifetime :inputTokens])) (watch-kv "output" (get-in event [:usage :lifetime :outputTokens])) (watch-kv "error" (:errorCode event)))
    "tool.admitted" (str prefix "tool ▶ " (watch-safe-text (:name event)) (watch-kv "id" (:toolCallId event)))
    "tool.progress" (str prefix "tool …" (watch-kv "id" (:toolCallId event)) (watch-kv "progress" (if (contains? event :progress) (do
  (watch-json-summary (:progress event))))))
    "tool.terminal" (str prefix "tool " (if (= "succeeded" (:status event)) "✓" "✗") " " (watch-safe-text (:status event)) (watch-kv "id" (:toolCallId event)) (watch-kv "result" (:resultPreview event)) (watch-kv "error" (:errorCode event)))
    "artifact.published" (str prefix "artifact published" (watch-kv "label" (:label event)) (watch-kv "bytes" (:bytes event)) (watch-kv "media" (:mediaType event)))
    "resource.pressure" (str prefix "resource " (if (:advisory event) "advisory" "pressure") (watch-kv "name" (:resource event)) (watch-kv "used" (:used event)) (watch-kv "limit" (:limit event)))
    "run.terminated" (str prefix "■ run " (watch-safe-text (:lifecycle event)) (watch-kv "reason" (get-in event [:reason :code])) (watch-kv "detail" (get-in event [:reason :detail]))))]
  (watch-safe-text rendered max-watch-output-codepoints)))

(defn render-watch-wire-line
  "Project one canonical JSONL envelope for a terminal. TypeScript remains the\n   semantic validator; this display boundary only parses enough shape to render\n   known events and to label malformed or future input visibly." [line]
  (cond
  (> (alength (.getBytes (str line) java.nio.charset.StandardCharsets/UTF_8)) max-watch-json-line-bytes) "! malformed wire JSONL: line exceeds display bound"
  :else (try
  (let [event (json/parse-string line true)
   version (:version event)
   kind (:kind event)
   essential (:essential event)]
  (cond
  (not (and (map? event) (string? version) (string? kind))) "! malformed wire JSONL: event envelope is not displayable"
  (and (= wire-watch-version version) (contains? wire-watch-kinds kind) (= true essential)) (render-known-wire-event event)
  (and (= wire-watch-version version) (contains? wire-watch-kinds kind)) "! malformed wire JSONL: known event must be essential"
  (= false essential) (watch-safe-text (str (watch-event-prefix event) "○ opaque nonessential" (watch-kv "kind" kind) (watch-kv "version" version)) max-watch-output-codepoints)
  :else (watch-safe-text (str "! unsupported essential wire event" (watch-kv "kind" kind) (watch-kv "version" version)) max-watch-output-codepoints)))
  (catch Exception _
    "! malformed wire JSONL: invalid JSON"))))

(defn- watch-contained-file [root child-name]
  (try
  (let [directory (.getCanonicalFile (io/file root))
   child (.getCanonicalFile (io/file directory child-name))]
  (if (= directory (.getParentFile child)) (do
  child)))
  (catch Exception _
    nil)))

(defn- watch-file-observation [file]
  (let [present? (.isFile file)
   bytes (if present? (do
  (.length file)))]
  {:path (str file) :present? present? :bytes bytes :modified-at (if present? (do
  (str (java.time.Instant/ofEpochMilli (.lastModified file)))))}))

(defn watch-plan
  "Resolve the canonical event stream and sparse process/control diagnostic for\n   one exact agent ID. The optional roots arity keeps path/status behavior\n   directly testable without consulting live agent state."
  ([args]
    (watch-plan args AGENT-STREAMDIR AGENT-LOGDIR))
  ([args stream-dir control-dir]
    (let [[id option & extra] args]
  (cond
  (or (nil? id) (seq extra)) {:error watch-usage}
  (contains? #{"--help" "-h" "help"} id) {:help watch-usage}
  (not (valid-control-id? id)) {:error (str "invalid agent ID; expected " control-id-pattern " and at most " max-control-id-bytes " UTF-8 bytes")}
  (and option (not= option "--control")) {:error (str "unknown watch option: " option)}
  :else (let [stream-file (watch-contained-file stream-dir (str "agent-" id ".stream.jsonl"))
   control-file (watch-contained-file control-dir (str id ".log"))]
  (if (and stream-file control-file) {:id id :mode (if (= option "--control") :control :stream) :stream (watch-file-observation stream-file) :control (watch-file-observation control-file)} {:error "watch path escapes its configured data root"}))))))

(defn- watch-status [label data-kind {:keys [path present? bytes modified-at]}]
  (watch-safe-text (str label ": " (watch-display-path path) " — " (cond
  (not present?) "not present yet"
  (zero? bytes) "present but empty"
  :else (str data-kind " present (" bytes " bytes, modified " modified-at ")"))) max-watch-output-codepoints))

(defn watch-status-lines [{:keys [mode stream control]}]
  [(str "watch target: " (if (= mode :stream) "canonical WireEvent stream" "process/control diagnostics (explicit opt-in)")) (watch-status "canonical WireEvent stream" "event data" stream) (watch-status "process/control diagnostics" "diagnostic data" control) (str "liveness guardrail: process/control logs are sparse diagnostics; " "their silence is not evidence that a worker stalled or died.")])

(defn- follow-watch-file! [mode path]
  (let [tail (p/process ["tail" "-n" "40" "-F" "--sleep-interval=0.2" "--max-unchanged-stats=1" "--" path] {:out :stream :err :inherit})]
  (try
  (with-open [reader (io/reader (:out tail))]
  (doseq [line (line-seq reader)]
  (println (if (= mode :stream) (render-watch-wire-line line) (watch-safe-text line max-watch-output-codepoints)))
  (flush)))
  (let [result (deref tail)]
  (if (not (zero? (:exit result))) (do
  (System/exit (:exit result)))))
  (finally
    (if (.isAlive ^Process (:proc tail)) (do
  (p/destroy-tree tail)))))))

(defn cmd-watch! [args]
  (let [{:keys [error help mode stream control] :as plan} (watch-plan args)]
  (cond
  help (println "usage:" help)
  error (do
  (binding [*out* *err*]
  (println (red error))
  (println (red "usage:") watch-usage))
  (System/exit 2))
  :else (let [target (if (= mode :control) control stream)]
  (doseq [line (watch-status-lines plan)]
  (println line))
  (if (not (:present? target)) (do
  (println (ylw "waiting for selected file to appear; absence alone is not a terminal verdict."))))
  (if (= mode :stream) (do
  (println "explicit diagnostics mode:" (cyn (str "north agent watch " (:id plan) " --control")))))
  (echo-cmd "tail -n 40 -F --sleep-interval=0.2 --max-unchanged-stats=1 --" (watch-display-path (:path target)) (if (= mode :stream) "| WireEvent projection" "| sanitized diagnostics"))
  (follow-watch-file! mode (:path target))))))

(defn cmd-tell-agent [args]
  (north.topology-authority/require-coordination! "msg")
  (let [rest0 (vec (remove (fn [arg] (contains? #{"--dry-run"} arg)) args))
   dry? (some #{"--dry-run"} args)
   from-idx (.indexOf rest0 "--from")
   from (if (>= from-idx 0) (nth rest0 (inc from-idx) nil) (or (System/getenv "NORTH_AGENT_ID") "north-cli"))
   pos (if (>= from-idx 0) (keep-indexed (fn [%1 %2] (if (not (#{from-idx (inc from-idx)} %1)) (do
  %2))) rest0) rest0)
   [id msg] pos]
  (if (or (nil? id) (nil? msg)) (do
  (binding [*out* *err*]
  (println (red "usage:") "north agent send <agent-id> \"<msg>\" [--from <me>]"))
  2) (let [argv ["bb" (str NORTH "/cli/msg-cli.clj") PORT "send" from id "msg" msg]]
  (echo-cmd (str/join " " argv))
  (if dry? (do
  (println (ylw "[dry-run]") "not sent; target capability and liveness were not checked.")
  0) (let [r (run argv :timeout msg-admission-timeout-ms)]
  (if (:ok r) (do
  (println (grn (or (known (:out r)) "queued for live injection")))
  0) (do
  (binding [*out* *err*]
  (println (red (or (known (:err r)) (known (:out r)) "msg admission unavailable"))))
  (let [status (:exit r)]
  (if (and (integer? status) (pos? status)) status 2))))))))))

(def max-safe-fence-epoch 9007199254740991)

(defn- decode-fence-utf8! [bytes path]
  (try
  (let [decoder (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
  (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
  (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
  (str (.decode decoder (java.nio.ByteBuffer/wrap bytes))))
  (catch java.nio.charset.CharacterCodingException error
    (throw (ex-info "saved liveness fence is not valid UTF-8" {:type :invalid-saved-liveness-fence :path path} error)))))

(defn- saved-presence-fence-json!
  ([bare]
    (saved-presence-fence-json! bare (or (System/getenv "NORTH_AGENT_LOGS_DIR") (str HOME "/.local/state/north/agents"))))
  ([bare directory]
    (if (not (valid-control-id? bare)) (do
  (throw (ex-info "north agent goal requires a safe agent id" {:type :invalid-saved-liveness-fence :agent bare}))))
    (let [directory-file (.getCanonicalFile (io/file directory))
   file (io/file directory-file (str bare ".liveness-fence.json"))
   path (.toPath file)
   no-follow (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])]
  (if (or (java.nio.file.Files/isSymbolicLink path) (not (java.nio.file.Files/isRegularFile path no-follow))) (do
  (throw (ex-info "north agent goal requires a regular saved liveness fence" {:type :invalid-saved-liveness-fence :path (.getPath file)}))))
  (let [permissions (java.nio.file.Files/getPosixFilePermissions path no-follow)
   expected-permissions #{java.nio.file.attribute.PosixFilePermission/OWNER_READ java.nio.file.attribute.PosixFilePermission/OWNER_WRITE}]
  (if (not (= expected-permissions (set permissions))) (do
  (throw (ex-info "saved liveness fence must have mode 0600" {:type :invalid-saved-liveness-fence :path (.getPath file)})))))
  (let [bytes (java.nio.file.Files/readAllBytes path)]
  (if (not (<= 1 (alength bytes) 512)) (do
  (throw (ex-info "saved liveness fence has an invalid size" {:type :invalid-saved-liveness-fence :path (.getPath file)}))))
  (let [raw (decode-fence-utf8! bytes (.getPath file))
   parsed (try
  (json/parse-string raw)
  (catch Exception error
    (throw (ex-info "saved liveness fence is not valid JSON" {:type :invalid-saved-liveness-fence :path (.getPath file)} error))))
   epoch (get parsed "epoch")
   expected (array-map "resource" (str "session:" bare) "holder" bare "epoch" epoch)]
  (if (not (and (map? parsed) (= #{"resource" "holder" "epoch"} (set (keys parsed))) (= (get expected "resource") (get parsed "resource")) (= bare (get parsed "holder")) (integer? epoch) (<= 1 epoch max-safe-fence-epoch) (= (str (json/generate-string expected) "\n") raw))) (do
  (throw (ex-info "saved liveness fence does not exactly match the agent" {:type :invalid-saved-liveness-fence :path (.getPath file) :agent bare}))))
  (json/generate-string expected))))))

(defn cmd-goal! [[id goal & _]]
  (north.topology-authority/require-coordination! "goal")
  (if (or (nil? id) (nil? goal)) (println (red "usage:") "north agent goal <agent-id> \"<new-goal>\"") (let [subj (str "agent:" (str/replace-first id #"^@?(agent:)?" ""))
   bare (subs subj (count "agent:"))
   facts (assoc (or (agent-facts-one bare) {}) "goal" goal)
   dn (render-display-name bare facts)
   update (json/generate-string {"goal" goal "display_name" dn})
   presence-fence (saved-presence-fence-json! bare)
   result (run ["bb" (str NORTH "/cli/agent-fact-internal.clj") PORT "goal" subj update "" "" "" "" "" presence-fence] :timeout 10000)]
  (if (:ok result) (do
  (println (grn "goal set") (bold bare))
  (println "  " dn)) (do
  (println (red "goal update failed"))
  (println (str/trim (str (:out result) (:err result)))))))))

(def cmd-agents cmd-agents!)

(def cmd-bind-child-referent cmd-bind-child-referent!)

(def cmd-delegate cmd-delegate!)

(def cmd-watch cmd-watch!)

(def cmd-goal cmd-goal!)

(if (not (or (= (System/getenv "NORTH_AGENTS_LIB") "1") (= (System/getProperty "north.agents.lib") "1"))) (do
  (let [[cmd & args] *command-line-args*]
  (try
  (case cmd
    "agents" (cmd-agents args)
    "templates" (cmd-templates args)
    "spawn" (cond
  (some #{"--doctor"} args) (do
  (load-file (str NORTH "/cli/spawn-doctor.clj"))
  (let [status ((resolve 'north.spawn-doctor/run!) args)]
  (if (pos? status) (do
  (System/exit status)))))
  (and (= 1 (count args)) (contains? #{"--help" "-h" "help"} (first args))) (cmd-spawn-help)
  :else (cmd-spawn args))
    "delegate" (cmd-delegate args)
    "bind-child-referent" (cmd-bind-child-referent args)
    "watch" (cmd-watch args)
    "msg" (let [status (cmd-tell-agent args)]
  (if (pos? status) (do
  (System/exit status))))
    "goal" (cmd-goal args)
    (do
  (println "usage: north {agents|templates|spawn|delegate|watch|msg|goal} ...")
  (System/exit 1)))
  (catch clojure.lang.ExceptionInfo error
    (if (or (north.topology-authority/denial? error) (:usage (ex-data error))) (do
  (binding [*out* *err*]
  (println (red (.getMessage error))))
  (System/exit (if (:usage (ex-data error)) 2 1))) (throw error)))))))
