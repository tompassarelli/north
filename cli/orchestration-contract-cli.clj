;; orchestration-contract-cli.clj — publishes @contract:dispatch, the dispatch
;; wire contract as a QUERYABLE subject (thread 019f8f5c, design §3.3). This
;; dissolves the 019f8ebe trap classes by making the child-dispatch payload a
;; `north show @contract:dispatch` away instead of tribal knowledge:
;;   - one `payload_field` fact per field  (canonical JSON {name,required,doc})
;;   - one `example_payload` fact          (canonical JSON of a valid payload)
;;   - one `error_code` fact per rejection class (canonical JSON {code,doc})
;; kind = wire_contract (a registered entity-kind; the payload_field/example_
;; payload/error_code predicates were registered in Phase 0). Publication is
;; ADDITIVE + reversible (the `retract` verb), matching the Phase 0/1 vocabulary-
;; seed precedent; it is NOT a shape-subject edit and NOT destructive.
;;
;; usage:
;;   bb orchestration-contract-cli.clj <port> seed      publish/refresh @contract:dispatch
;;   bb orchestration-contract-cli.clj <port> show      print what is on the graph
;;   bb orchestration-contract-cli.clj <port> retract   remove it (rollback)
(require '[clojure.java.io :as io]
         '[cheshire.core :as json])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def SUBJECT "@contract:dispatch")

;; The child-dispatch payload. `required` fields must be present in every child
;; payload; the eight-field routing axes compose server-side from the template
;; named by composition.id
;; for the preset fast path (R7: fast path only at the derived minimum, elevation
;; stays long-form + coded exception). `thread` is required in EVERY payload so a
;; child's obligations never prebind (the 019f8ebe stray-binding lesson).
(def PAYLOAD-FIELDS
  [{"name" "thread"    "required" true  "doc" "explicit child thread id; obligations never prebind to the parent (019f8ebe)"}
   {"name" "role"      "required" true  "doc" "functional identity independent of composition kind and id"}
   {"name" "prompt"    "required" true  "doc" "the task text handed to the child lane"}
   {"name" "taskGrade" "required" true  "doc" "scope/autonomy/novelty prior; composed from the template for an unmodified preset"}
   {"name" "topology"  "required" true  "doc" "worker | orchestrator; fixed by a stock template — change it only via a bespoke composition"}
   {"name" "tier"      "required" true  "doc" "semantic model-capability floor; = derived minimum on the preset fast path"}
   {"name" "reasoning" "required" true  "doc" "deliberation budget; = derived minimum on the preset fast path"}
   {"name" "posture"   "required" true  "doc" "explore | evaluate | deliver | preserve | prune — what yields when values collide"}
   {"name" "domainRequirements" "required" true "doc" "provider-neutral capability requirements (array; [] when none)"}
   {"name" "composition" "required" true "doc" "{kind:template|bespoke, id, ...}; template overrides must record exactly the changed axes"}
   {"name" "signals"   "required" false "doc" "optional 7-signal routing assessment; required only to select ABOVE the derived minimum"}])

;; Rejection classes — the real admission failures a caller recovers from by
;; reading this contract (routing-metadata.ts / routing-admission.ts / orchestration-
;; staffing.ts). Dispatch rejection messages cite @contract:dispatch.
(def ERROR-CODES
  [{"code" "unknown-field"          "doc" "payload carries a field outside this contract"}
   {"code" "incomplete-request"     "doc" "the complete eight-field Agent Machinery run request is missing one or more axes"}
   {"code" "template-unknown"       "doc" "composition.kind=template names an id absent from the stock-template catalog"}
   {"code" "override-undeclared"    "doc" "a template axis changed without composition.overrides + overrideReason"}
   {"code" "topology-fixed"         "doc" "attempt to change a stock template's fixed topology through a preset"}
   {"code" "above-minimum-uncoded"  "doc" "selected exceeds the derived minimum without a coded exception (R7)"}
   {"code" "missing-thread"         "doc" "no explicit child thread id in the payload"}])

(def EXAMPLE-PAYLOAD
  {"thread" "2026-07-24-120000"
   "role" "verifier"
   "prompt" "Verify claim X against artifact Y; one adversarial verdict."
   "taskGrade" "senior"
   "topology" "worker"
   "tier" "senior"
   "reasoning" "high"
   "posture" "evaluate"
   "domainRequirements" []
   "composition" {"kind" "template" "id" "verifier" "overrides" []}})

;; canonical JSON (sorted keys) so a field/example/code fact is byte-stable.
(defn- canon [x]
  (cond
    (map? x)        (into (sorted-map) (map (fn [[k v]] [k (canon v)]) x))
    (sequential? x) (mapv canon x)
    :else           x))
(defn- cjson [x] (json/generate-string (canon x)))

(defn exact-facts [port subject]
  (->> (north.coord/query-rows!
        port
        {:find "p,v" :rules [{:head {:rel "p,v" :args [{:var "p"} {:var "v"}]}
                               :body [{:rel "triple" :args [subject {:var "p"} {:var "v"}]}]}]})
       (map (fn [row] [(nth row 0) (nth row 1)]))
       (sort-by (juxt first second))))

(defn set-action [subject predicate values cardinality]
  {:op :set :subject subject :predicate predicate :values (vec values)
   :cardinality cardinality})

(defn publish-actions! [port actions]
  (let [result (north.coord/publish! port (vec actions))]
    (when (:reject result)
      (throw (ex-info "Store RPC rejected agent-run contract publication"
                      {:type :contract-publication-rejected :result result})))
    result))

(defn seed! [port]
  (publish-actions!
   port
   (concat
    [(set-action SUBJECT "kind" ["wire_contract"] :one)
     (set-action SUBJECT "doc"
                 ["the child-dispatch payload contract; north show @contract:dispatch to recover a valid shape"]
                 :one)
     (set-action SUBJECT "example_payload" [(cjson EXAMPLE-PAYLOAD)] :one)
     (set-action SUBJECT "payload_field" (map cjson PAYLOAD-FIELDS) :many)
     (set-action SUBJECT "error_code" (map cjson ERROR-CODES) :many)]))
  (println (format "✓ published %s on :%d (%d payload_field, %d error_code, 1 example_payload)"
                   SUBJECT port (count PAYLOAD-FIELDS) (count ERROR-CODES))))

(defn retract-all! [port]
  (publish-actions!
   port
   (for [predicate ["kind" "doc" "payload_field" "error_code" "example_payload"]]
     (set-action SUBJECT predicate []
                 (if (contains? #{"payload_field" "error_code"} predicate)
                   :many :one))))
  (println (format "✓ retracted %s on :%d" SUBJECT port)))

(let [[ps verb] *command-line-args*
      port (Integer/parseInt (or ps "7977"))]
  (case verb
    "seed"    (seed! port)
    "retract" (retract-all! port)
    "show"    (doseq [[p v] (exact-facts port SUBJECT)] (println (format "  %-16s %s" p v)))
    (do (println "usage: orchestration-contract-cli.clj <port> {seed | show | retract}")
        (System/exit 2))))
