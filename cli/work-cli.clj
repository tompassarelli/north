(ns north.work-cli
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^String semantic-receipt-protocol "north.semantic-receipt")

(def ^String semantic-view-protocol "north.semantic-view")

(def ^String semantic-history-protocol "north.semantic-history")

(def ^String semantic-inbox-protocol "north.semantic-inbox")

(def ^String semantic-catalog-protocol "north.semantic-catalog")

(def semantic-protocol-version 1)

(defrecord CommandOption [flag field])

(defn commandoption-flag [r] (:flag r))

(defn commandoption-field [r] (:field r))

(defrecord CommandSpec [action kind positional-fields required-positional-count options receipt-fields])

(defn commandspec-action [r] (:action r))

(defn commandspec-kind [r] (:kind r))

(defn commandspec-positional-fields [r] (:positional-fields r))

(defn commandspec-required-positional-count [r] (:required-positional-count r))

(defn commandspec-options [r] (:options r))

(defn commandspec-receipt-fields [r] (:receipt-fields r))

(defrecord ParsedCommand [action kind arguments])

(defn parsedcommand-action [r] (:action r))

(defn parsedcommand-kind [r] (:kind r))

(defn parsedcommand-arguments [r] (:arguments r))

(defrecord CommandObservation [read-plan snapshot catalog])

(defn commandobservation-read-plan [r] (:read-plan r))

(defn commandobservation-snapshot [r] (:snapshot r))

(defn commandobservation-catalog [r] (:catalog r))

(defrecord CommandRuntime [observe! plan-mutation! plan-actions plan-expected-version transact! mutation-receipt! render-read!])

(defn commandruntime-observe! [r] (:observe! r))

(defn commandruntime-plan-mutation! [r] (:plan-mutation! r))

(defn commandruntime-plan-actions [r] (:plan-actions r))

(defn commandruntime-plan-expected-version [r] (:plan-expected-version r))

(defn commandruntime-transact! [r] (:transact! r))

(defn commandruntime-mutation-receipt! [r] (:mutation-receipt! r))

(defn commandruntime-render-read! [r] (:render-read! r))

(def command-specs {"track" (->CommandSpec "track" :mutation [:title] 1 [(->CommandOption "--tracked-by" :tracked-by)] [:referent]) "plan" (->CommandSpec "plan" :mutation [:referent] 1 [(->CommandOption "--path" :path) (->CommandOption "--endorsed-by" :endorsed-by)] [:referent :revision]) "start" (->CommandSpec "start" :mutation [:referent] 1 [(->CommandOption "--revision" :revision) (->CommandOption "--authorized-by" :authorized-by) (->CommandOption "--signature" :signature)] [:referent :occurrence]) "assign" (->CommandSpec "assign" :mutation [:referent] 1 [(->CommandOption "--to" :to) (->CommandOption "--assigned-by" :assigned-by)] [:referent :assignment]) "request" (->CommandSpec "request" :mutation [:about] 0 [(->CommandOption "--from" :from) (->CommandOption "--to" :to) (->CommandOption "--body" :body)] [:request]) "ack" (->CommandSpec "ack" :mutation [:request] 1 [(->CommandOption "--by" :by)] [:request :ack]) "result" (->CommandSpec "result" :mutation [:request] 1 [(->CommandOption "--result" :result) (->CommandOption "--reported-by" :reported-by) (->CommandOption "--outcome" :outcome) (->CommandOption "--summary" :summary)] [:request :result :outcome]) "ownership" (->CommandSpec "ownership" :mutation [] 0 [(->CommandOption "--transition" :transition)] [:transition :owner]) "settle" (->CommandSpec "settle" :mutation [:assignment] 1 [(->CommandOption "--transition" :accepted-transition) (->CommandOption "--by" :by) (->CommandOption "--outcome" :outcome) (->CommandOption "--summary" :summary)] [:assignment :acceptedTransition :settlement :outcome]) "show" (->CommandSpec "show" :read [:referent] 1 [] []) "history" (->CommandSpec "history" :read [:referent] 1 [] []) "inbox" (->CommandSpec "inbox" :read [:actor] 1 [] []) "catalog" (->CommandSpec "catalog" :catalog [] 0 [] [])})

(def ^String usage-text (str "usage: north work <command> ... --json\n" "\n" "tracked things\n" "  track TITLE --tracked-by ACTOR --json\n" "  plan TRACKED-THING --path PATH --endorsed-by ACTOR --json\n" "  start TRACKED-THING --revision PLAN-REVISION" " --authorized-by ACTOR --signature SIGNATURE --json\n" "  assign TRACKED-THING --to ACTOR --assigned-by ACTOR --json\n" "  request [TRACKED-THING] --from ACTOR --to ACTOR --body BODY --json\n" "  ack REQUEST --by ACTOR --json\n" "  result REQUEST --result RESULT --reported-by ACTOR" " --outcome OUTCOME --summary SUMMARY --json\n" "  ownership --transition WORK-OWNERSHIP-V1-JSON --json\n" "  settle ASSIGNMENT --transition TRANSITION --by ACTOR" " --outcome OUTCOME --summary SUMMARY --json\n" "  show TRACKED-THING --json\n" "  history TRACKED-THING --json\n" "  inbox ACTOR --json\n" "  catalog --json\n" "\n" "views: Agents | Goals | All\n"))

(defn fail! [^String message data]
  (throw (ex-info message data)))

(defn ^String exact-text! [^String label value]
  (if (and (string? value) (not (empty? value)) (= value (str/trim value)) (not (str/includes? value "\u0000"))) value (fail! (str label " must be exact nonblank text") {:type :invalid-work-command-value :field label})))

(defn- ^Boolean exact-text? [value]
  (and (string? value) (not (empty? value)) (= value (str/trim value)) (not (str/includes? value "\u0000"))))

(defn- ^CommandSpec command-spec! [action]
  (let [^String action (exact-text! "command" action)
   spec (get command-specs action)]
  (if (some? spec) spec (fail! "unknown tracked-thing command" {:type :unknown-work-command :command action}))))

(defn- add-argument! [values field ^String label value]
  (if (contains? values field) (fail! (str label " may appear exactly once") {:type :duplicate-work-command-value :field field}) (assoc values field (exact-text! label value))))

(defn ^ParsedCommand parse-command! [argv]
  (if (empty? argv) (fail! "tracked-thing command is required" {:type :missing-work-command}) (let [^CommandSpec spec (command-spec! (first argv))]
  (loop [tokens (vec (rest argv))
   positional-index 0
   option-seen? false
   json-seen? false
   values {}]
  (if (empty? tokens) (do
  (if (not json-seen?) (fail! "--json is required for the stable command protocol" {:type :missing-json-protocol :command (:action spec)}) true)
  (if (or (< positional-index (:required-positional-count spec)) (> positional-index (count (:positional-fields spec)))) (fail! "tracked-thing command has the wrong positional arity" {:type :invalid-work-command-arity :command (:action spec)}) true)
  (let [missing (filterv (fn [^CommandOption option] (not (contains? values (:field option)))) (:options spec))]
  (if (seq missing) (fail! "tracked-thing command is missing required options" {:type :missing-work-command-options :command (:action spec) :fields (mapv (fn [^CommandOption option] (:field option)) missing)}) (->ParsedCommand (:action spec) (:kind spec) values)))) (let [^String token (first tokens)]
  (cond
  (= token "--json") (if json-seen? (fail! "--json may appear exactly once" {:type :duplicate-json-protocol}) (recur (vec (rest tokens)) positional-index true true values))
  (str/starts-with? token "--") (let [option (some (fn [^CommandOption candidate] (if (= token (:flag candidate)) (do
  candidate))) (:options spec))]
  (if (nil? option) (fail! "unknown tracked-thing command option" {:type :unknown-work-command-option :command (:action spec) :option token}) (if (< (count tokens) 2) (fail! "tracked-thing command option requires a value" {:type :missing-work-command-option-value :command (:action spec) :option token}) (let [^String value (second tokens)]
  (if (str/starts-with? value "--") (fail! "tracked-thing command option requires a value" {:type :missing-work-command-option-value :command (:action spec) :option token}) (recur (vec (drop 2 tokens)) positional-index true json-seen? (add-argument! values (:field option) token value)))))))
  option-seen? (fail! "positional values must precede tracked-thing options" {:type :misplaced-work-command-value :command (:action spec) :value token})
  (>= positional-index (count (:positional-fields spec))) (fail! "tracked-thing command has too many positional values" {:type :invalid-work-command-arity :command (:action spec)})
  :else (let [field (nth (:positional-fields spec) positional-index)]
  (recur (vec (rest tokens)) (inc positional-index) false json-seen? (add-argument! values field (name field) token))))))))))

(defn ^String command-argument! [^ParsedCommand command field]
  (let [value (get (:arguments command) field)]
  (if (string? value) value (fail! "parsed command omitted a required value" {:type :malformed-parsed-command :command (:action command) :field field}))))

(defn ^Boolean mutation-command? [^ParsedCommand command]
  (= :mutation (:kind command)))

(defn ^Boolean catalog-command? [^ParsedCommand command]
  (= :catalog (:kind command)))

(defn- ^Boolean exact-map-keys? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn- ^Boolean nonnegative-store-version? [value]
  (and (integer? value) (not (neg? value))))

(defn- ^String required-receipt-id! [receipt field]
  (exact-text! (name field) (get receipt field)))

(defn- mutation-receipt-fields [^CommandSpec spec ^ParsedCommand command receipt]
  (if (or (and (= "request" (:action command)) (contains? (:arguments command) :about)) (and (= "result" (:action command)) (map? receipt) (contains? receipt :referent))) (conj (:receipt-fields spec) :referent) (:receipt-fields spec)))

(defn validate-mutation-receipt! [^ParsedCommand command receipt]
  (let [^CommandSpec spec (command-spec! (:action command))
   receipt-fields (mutation-receipt-fields spec command receipt)
   expected (set (concat [:protocol :version :action :storeVersion] receipt-fields))]
  (if (and (mutation-command? command) (exact-map-keys? receipt expected) (= semantic-receipt-protocol (:protocol receipt)) (= semantic-protocol-version (:version receipt)) (= (:action command) (:action receipt)) (nonnegative-store-version? (:storeVersion receipt))) (do
  (doseq [field receipt-fields]
  (required-receipt-id! receipt field))
  receipt) (fail! "semantic mutation returned an invalid committed receipt" {:type :invalid-semantic-receipt :command (:action command)}))))

(defn- validate-read-receipt! [^ParsedCommand command receipt]
  (let [^String action (:action command)
   expected (cond
  (= action "show") #{:protocol :version :referent :facts :derived}
  (= action "history") #{:protocol :version :referent :occurrences}
  (= action "inbox") #{:protocol :version :actor :requests}
  :else #{})
   ^String protocol (cond
  (= action "show") semantic-view-protocol
  (= action "history") semantic-history-protocol
  (= action "inbox") semantic-inbox-protocol
  :else "")
   identity-field (if (= action "inbox") :actor :referent)
   collection-field (cond
  (= action "show") :facts
  (= action "history") :occurrences
  :else :requests)]
  (if (and (not (mutation-command? command)) (exact-map-keys? receipt expected) (= protocol (:protocol receipt)) (= semantic-protocol-version (:version receipt)) (vector? (get receipt collection-field)) (or (not= action "show") (vector? (:derived receipt)))) (do
  (required-receipt-id! receipt identity-field)
  receipt) (fail! "semantic read returned an invalid committed view" {:type :invalid-semantic-read :command action}))))

(def catalog-row-fields #{:id :title :desiredOutcome :agent :work :plan :project :task :assignee :assigneeTitle :status})

(defn- ^Boolean nullable-string? [value]
  (or (nil? value) (string? value)))

(defn- ^Boolean valid-catalog-row? [row]
  (and (exact-map-keys? row catalog-row-fields) (exact-text? (:id row)) (exact-text? (:title row)) (nullable-string? (:desiredOutcome row)) (boolean? (:agent row)) (boolean? (:work row)) (boolean? (:plan row)) (boolean? (:project row)) (boolean? (:task row)) (nullable-string? (:assignee row)) (nullable-string? (:assigneeTitle row)) (nullable-string? (:status row))))

(defn validate-catalog-receipt! [receipt]
  (let [expected #{:protocol :version :storeSpace :storeVersion :trackedThings}
   rows (:trackedThings receipt)
   ids (if (vector? rows) (mapv (fn [row] (:id row)) rows) [])]
  (if (and (exact-map-keys? receipt expected) (= semantic-catalog-protocol (:protocol receipt)) (= semantic-protocol-version (:version receipt)) (exact-text? (:storeSpace receipt)) (nonnegative-store-version? (:storeVersion receipt)) (vector? rows) (every? valid-catalog-row? rows) (= ids (vec (sort ids))) (= (count ids) (count (set ids)))) receipt (fail! "semantic catalog returned an invalid committed view" {:type :invalid-semantic-catalog}))))

(defn- ^String fresh-id [^String kind]
  (str "@" kind ":" (java.util.UUID/randomUUID)))

(def preallocated-occurrence-fields {"request" :request-occurrence "ack" :ack-occurrence "settle" :settlement-occurrence})

(defn- ^ParsedCommand prepare-command! [^ParsedCommand command]
  (let [field (get preallocated-occurrence-fields (:action command))]
  (if (keyword? field) (assoc command :arguments (assoc (:arguments command) field (fresh-id "occurrence"))) command)))

(defn invoke-command! [^CommandRuntime runtime port ^ParsedCommand command]
  (let [^ParsedCommand command (prepare-command! command)
   observation ((:observe! runtime) port command)]
  (cond
  (mutation-command? command) (let [plan ((:plan-mutation! runtime) command observation)
   actions ((:plan-actions runtime) plan)
   expected-version ((:plan-expected-version runtime) plan)
   committed ((:transact! runtime) port actions expected-version)
   receipt ((:mutation-receipt! runtime) command plan committed)]
  (validate-mutation-receipt! command receipt))
  (catalog-command? command) (validate-catalog-receipt! ((:render-read! runtime) command observation))
  :else (validate-read-receipt! command ((:render-read! runtime) command observation)))))

(defn ^String render-command-json! [^CommandRuntime runtime port argv]
  (str (json/generate-string (invoke-command! runtime port (parse-command! argv))) "\n"))

(defn coordination-port! []
  (let [parsed (parse-long (or (System/getenv "NORTH_PORT") "7977"))]
  (if (and (integer? parsed) (<= 1 parsed 65535)) (int parsed) (fail! "NORTH_PORT must be an integer from 1 through 65535" {:type :invalid-coordination-port}))))

(def work-cli-dir (.getParentFile (io/file *file*)))

(def work-root (.getParentFile work-cli-dir))

(def runtime-module-paths {"north.coord" (str work-cli-dir "/coord.clj") "north.referents" (str work-root "/src/north/referents.clj") "north.work-occurrences" (str work-root "/src/north/work_occurrences.clj") "north.work-catalog" (str work-cli-dir "/work-catalog.clj")})

(def runtime-loaded-modules (atom #{}))

(defn- load-runtime-module! [^String module]
  (do
  (if (not (contains? (deref runtime-loaded-modules) module)) (do
  (let [path (get runtime-module-paths module)]
  (if (not (and (string? path) (.isFile (io/file path)))) (do
  (fail! "semantic runtime module is unavailable" {:type :semantic-runtime-module-unavailable :namespace module :path path})))
  (clojure.core/load-file path)
  (swap! runtime-loaded-modules conj module))))
  nil))

(defn- ensure-runtime-loaded! [^String namespace]
  (do
  (cond
  (= "north.coord" namespace) (load-runtime-module! "north.coord")
  (= "store.types" namespace) (load-runtime-module! "north.coord")
  (= "north.work-occurrences" namespace) (doseq [module ["north.coord" "north.referents" "north.work-occurrences"]]
  (load-runtime-module! module))
  (= "north.work-catalog" namespace) (doseq [module ["north.coord" "north.referents" "north.work-catalog"]]
  (load-runtime-module! module))
  :else (fail! "semantic runtime namespace is unavailable" {:type :semantic-runtime-namespace-unavailable :namespace namespace}))
  nil))

(defn- runtime-function! [^String namespace ^String operation]
  (do
  (ensure-runtime-loaded! namespace)
  (let [resolved (clojure.core/ns-resolve (symbol namespace) (symbol operation))]
  (if (some? resolved) (clojure.core/var-get resolved) (fail! "semantic runtime function is unavailable" {:type :semantic-runtime-function-unavailable :namespace namespace :operation operation})))))

(defn- runtime-call! [^String namespace ^String operation arguments]
  (apply (runtime-function! namespace operation) arguments))

(defn- work-call! [^String operation arguments]
  (runtime-call! "north.work-occurrences" operation arguments))

(defn- coord-call! [^String operation arguments]
  (runtime-call! "north.coord" operation arguments))

(defn- ^String now-text []
  (str (java.time.Instant/now)))

(defn- query-variable [^String name]
  {:var name})

(defn- triple-literal [subject predicate value]
  {:rel "triple" :args [subject predicate value]})

(defn- query-rule [^String relation subject predicate value body]
  {:head {:rel relation :args [subject predicate value]} :body body})

(defn- direct-subject-rules [^String relation subjects predicates]
  (vec (for [subject subjects
   predicate predicates]
  (let [value (query-variable "value")]
  (query-rule relation subject predicate value [(triple-literal subject predicate value)])))))

(defn- anchored-rules [^String relation ^String anchor ^String identity predicates]
  (vec (for [predicate predicates]
  (let [subject (query-variable "subject")
   value (query-variable "value")]
  (query-rule relation subject predicate value [(triple-literal subject anchor identity) (triple-literal subject predicate value)])))))

(defn- followed-subject-rules [^String relation subjects follow-predicates followed-predicates]
  (vec (for [subject subjects
   follow-predicate follow-predicates
   followed-predicate followed-predicates]
  (let [followed (query-variable "followed")
   value (query-variable "value")]
  (query-rule relation followed followed-predicate value [(triple-literal subject follow-predicate followed) (triple-literal followed followed-predicate value)])))))

(defn- followed-anchor-rules [^String relation ^String anchor ^String identity follow-predicates followed-predicates]
  (vec (for [follow-predicate follow-predicates
   followed-predicate followed-predicates]
  (let [subject (query-variable "subject")
   followed (query-variable "followed")
   value (query-variable "value")]
  (query-rule relation followed followed-predicate value [(triple-literal subject anchor identity) (triple-literal subject follow-predicate followed) (triple-literal followed followed-predicate value)])))))

(defn- read-plan-query! [plan]
  (let [^String relation "north_work_snapshot"
   mode (:mode plan)
   ^String identity (exact-text! "read identity" (:identity plan))
   subjects (:subjects plan)
   predicates (:predicates plan)
   follow-predicates (:follow-predicates plan)
   followed-predicates (:followed-predicates plan)
   rules (cond
  (= :subjects mode) (vec (concat (direct-subject-rules relation subjects predicates) (followed-subject-rules relation subjects follow-predicates followed-predicates)))
  (= :about mode) (vec (concat (anchored-rules relation "about" identity predicates) (followed-anchor-rules relation "about" identity follow-predicates followed-predicates)))
  (= :inbox mode) (vec (concat (anchored-rules relation "to" identity predicates) (followed-anchor-rules relation "to" identity follow-predicates followed-predicates)))
  :else (fail! "semantic read plan has an unsupported mode" {:type :invalid-semantic-read-plan :mode mode}))]
  {:find relation :rules rules}))

(defn- snapshot-triple! [row]
  (if (and (vector? row) (= 3 (count row)) (every? string? row)) (runtime-call! "store.types" "triple" row) (fail! "semantic Store query returned a malformed fact" {:type :malformed-semantic-store-fact :row row})))

(defn- ^String coordination-store-space! [port]
  (let [status (coord-call! "status!" [port])]
  (exact-text! "coordination Store space" (:space-id status))))

(defn- execute-read-plan! [port plan]
  (let [limit (:limit plan)]
  (if (not (and (integer? limit) (pos? limit))) (do
  (fail! "semantic read plan has an invalid row bound" {:type :invalid-semantic-read-plan :limit limit})))
  (let [^String store-space (coordination-store-space! port)
   response (coord-call! "bounded-query!" [port (read-plan-query! plan) limit])
   store-version (:served-version response)
   rows (:rows response)]
  (if (not (vector? rows)) (do
  (fail! "semantic Store query omitted its fact rows" {:type :malformed-semantic-store-read})))
  (work-call! "canonical-snapshot!" [store-space store-version (mapv snapshot-triple! rows)]))))

(defn- command-read-plan! [^ParsedCommand command]
  (let [^String action (:action command)]
  (cond
  (= "request" action) (work-call! "request-read-plan!" [(command-argument! command :request-occurrence)])
  (= "start" action) (work-call! "start-read-plan!" [(command-argument! command :referent) (command-argument! command :revision)])
  (= "ack" action) (work-call! "ack-read-plan!" [(command-argument! command :request) (command-argument! command :ack-occurrence)])
  (= "result" action) (work-call! "result-read-plan!" [(command-argument! command :request) (command-argument! command :result)])
  (= "settle" action) (work-call! "settle-read-plan!" [(command-argument! command :assignment) (command-argument! command :accepted-transition) (command-argument! command :settlement-occurrence)])
  (= "show" action) (work-call! "show-read-plan!" [(command-argument! command :referent)])
  (= "history" action) (work-call! "history-read-plan!" [(command-argument! command :referent)])
  (= "inbox" action) (work-call! "inbox-read-plan!" [(command-argument! command :actor)])
  :else nil)))

(defn- ^CommandObservation observe-command! [port ^ParsedCommand command]
  (if (catalog-command? command) (->CommandObservation nil nil (runtime-call! "north.work-catalog" "catalog-envelope!" [port])) (let [read-plan (command-read-plan! command)
   snapshot (if (some? read-plan) (execute-read-plan! port read-plan) (work-call! "canonical-snapshot!" [(coordination-store-space! port) (coord-call! "cur-ver!" [port]) []]))]
  (->CommandObservation read-plan snapshot nil))))

(defn- plan-command-mutation! [^ParsedCommand command observation]
  (let [^String action (:action command)
   snapshot (:snapshot observation)
   ^String occurred-at (now-text)]
  (cond
  (= "track" action) (work-call! "track-plan!" [(fresh-id "referent") (command-argument! command :title) (command-argument! command :tracked-by) occurred-at snapshot])
  (= "plan" action) (work-call! "plan-revision-plan!" [(command-argument! command :referent) (fresh-id "occurrence") (command-argument! command :path) (command-argument! command :endorsed-by) occurred-at snapshot])
  (= "start" action) (let [^String referent (command-argument! command :referent)
   ^String revision (command-argument! command :revision)
   plan (work-call! "decode-plan-snapshot!" [snapshot referent revision])]
  (work-call! "start-plan!" [(fresh-id "occurrence") plan revision (command-argument! command :authorized-by) (command-argument! command :signature) occurred-at snapshot]))
  (= "assign" action) (work-call! "assignment-plan!" [(fresh-id "occurrence") (command-argument! command :referent) (command-argument! command :assigned-by) (command-argument! command :to) occurred-at snapshot])
  (= "request" action) (work-call! "request-plan!" [(command-argument! command :request-occurrence) (:about (:arguments command)) (command-argument! command :from) (command-argument! command :to) (command-argument! command :body) occurred-at snapshot])
  (= "ack" action) (let [request (work-call! "decode-request-snapshot!" [snapshot (command-argument! command :request)])]
  (work-call! "ack-plan!" [(command-argument! command :ack-occurrence) request (command-argument! command :by) occurred-at snapshot]))
  (= "result" action) (let [request (work-call! "decode-request-snapshot!" [snapshot (command-argument! command :request)])]
  (work-call! "result-plan!" [(command-argument! command :result) request (command-argument! command :reported-by) (command-argument! command :outcome) (command-argument! command :summary) occurred-at snapshot]))
  (= "ownership" action) (let [transition (work-call! "decode-ownership-transition!" [(json/parse-string (command-argument! command :transition))])]
  (work-call! "ownership-transition-plan!" [(fresh-id "occurrence") transition occurred-at snapshot]))
  (= "settle" action) (let [assignment (work-call! "decode-assignment-snapshot!" [snapshot (command-argument! command :assignment)])
   accepted (work-call! "decode-ownership-occurrence!" [snapshot (command-argument! command :accepted-transition)])]
  (work-call! "settlement-plan!" [(command-argument! command :settlement-occurrence) assignment accepted (command-argument! command :by) (command-argument! command :outcome) (command-argument! command :summary) occurred-at snapshot]))
  :else (fail! "semantic mutation planner does not support the command" {:type :unsupported-semantic-mutation :command action}))))

(defn- planned-actions! [plan]
  (let [actions (work-call! "publication-actions" [plan])]
  (if (vector? actions) actions (fail! "semantic mutation plan omitted its Store actions" {:type :invalid-semantic-mutation-plan}))))

(defn- planned-version! [plan]
  (let [options (work-call! "publication-options" [plan])
   version (:expected-version options)]
  (if (and (= #{:expected-version} (set (keys options))) (nonnegative-store-version? version)) version (fail! "semantic mutation plan omitted its exact Store version" {:type :invalid-semantic-mutation-plan}))))

(defn- transact-plan! [port actions expected-version]
  (coord-call! "transact!" [port actions {:expected-version expected-version}]))

(defn- committed-receipt! [^ParsedCommand _command plan committed]
  (work-call! "semantic-receipt!" [plan (work-call! "publication-options" [plan]) committed]))

(defn- render-semantic-read! [^ParsedCommand command observation]
  (let [^String action (:action command)
   read-plan (:read-plan observation)
   snapshot (:snapshot observation)]
  (cond
  (= "catalog" action) (:catalog observation)
  (= "show" action) (work-call! "semantic-view!" [read-plan snapshot])
  (= "history" action) (work-call! "semantic-history!" [read-plan snapshot])
  (= "inbox" action) (work-call! "semantic-inbox!" [read-plan snapshot])
  :else (fail! "semantic read projector does not support the command" {:type :unsupported-semantic-read :command action}))))

(defn ^CommandRuntime default-runtime! []
  (->CommandRuntime observe-command! plan-command-mutation! planned-actions! planned-version! transact-plan! committed-receipt! render-semantic-read!))

(def ^:dynamic ^CommandRuntime *command-runtime* (default-runtime!))

(defn run-cli! [argv]
  (if (or (= argv ["help"]) (= argv ["--help"])) (print usage-text) (print (render-command-json! *command-runtime* (coordination-port!) argv))))

(def ^String script-file (.getCanonicalPath (io/file *file*)))

(defn- ^Boolean direct-invocation? []
  (let [candidate (System/getProperty "babashka.file")]
  (and (string? candidate) (= script-file (.getCanonicalPath (io/file candidate))))))

(if (direct-invocation?) (do
  (try
  (run-cli! (vec *command-line-args*))
  (catch Exception error
    (do
  (binding [*out* *err*]
  (println (str "north work: " (.getMessage error))))
  (System/exit 2))))))
