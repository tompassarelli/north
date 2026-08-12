;; msg-cli.clj — messaging-as-facts (North gate-2, primitive 3) + command-as-facts.
;; A message = @msg:<id> facts (human mail); a COMMAND = @cmd:<id> facts (op/target/args
;; each a separate fact, NEVER an opaque {:op :args} body blob). ack = a fact (acked_by);
;; inbox/done/pending = derived queries. Fram stores and notifies; routing stays
;; in North.
(require '[cheshire.core :as json]
         '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str])

;; Reply validation uses Fram's commit-time closed-vocabulary, cardinality, and
;; dangling-reference rules. A rejected fact is an invalid reply.

;; Shared cardinality-aware publication and command queries live in cli/coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-id.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/command-id.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-contract.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-routing.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/agent-provenance.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/lifecycle-projection.clj"))
(def one     north.coord/resolved)
(def many    north.coord/many)

(def msg-control-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")
(def target-identity-manifest-predicate "target_identity_manifest_sha256")
(def max-msg-run-candidates 128)
(defn reject-message! [message]
  (binding [*out* *err*] (println (str "REJECTED: message " message)))
  (System/exit 2))
(defn reject-message-unavailable! [message]
  (binding [*out* *err*]
    (println (str "REJECTED: message read-unavailable: " message)))
  (System/exit 3))
(defn validate-message-input! [from to subject body]
  (when-let [problem
             (north.message-contract/input-problem from to subject body)]
    (reject-message! problem))
  true)
(defn admitted-message-recipient! [port requested-to dead-drop?]
  (if dead-drop?
    requested-to
    (let [route
          (north.message-routing/require-live-address port requested-to)]
      (when (= :unavailable (:live route))
        (reject-message-unavailable!
         (str "recipient " requested-to " liveness could not be read")))
      (when (false? (:live route))
        (reject-message!
         (str "recipient " (:recipient route) " has no live presence"
              (when (not= requested-to (:recipient route))
                (str " (addressed as alias " requested-to ")"))
              (when-let [alternative (:alternative route)]
                (str "; live same repo/role session: " alternative))
              "; pass --dead-drop only for deliberate delivery to an absent identity")))
      (:recipient route))))
(defn reject-msg! [message]
  (binding [*out* *err*] (println (str "REJECTED: msg " message)))
  (System/exit 2))

;; A read that could not be completed (coordinator error map, malformed envelope,
;; timeout, disconnect) is NOT evidence the target is dead. Distinguish it from a
;; genuine negative (offline/terminal/unsupported, exit 2) with its own exit code
;; so a transient projection failure never gets recorded as "target not live" and
;; a caller may retry rather than treat the lane as gone.
(defn reject-msg-unavailable! [message]
  (binding [*out* *err*]
    (println (str "REJECTED: msg read-unavailable: " message)))
  (System/exit 3))

(defn msg-agent-facts [port control]
  (try
    (north.lifecycle-projection/folded-agent-point-facts
     (fn [subject predicate] (many port subject predicate))
     (str "@agent:" control))
    (catch Exception _
      (reject-msg-unavailable! "target identity projection is unreadable"))))

(defn msg-run-entries [port control]
  (try
    (let [response
          (north.coord/query-page
           port
           {:find "msg_run_candidate"
            :rules
            [{:head {:rel "msg_run_candidate"
                     :args [{:var "e"}]}
              :body [{:rel "triple"
                      :args [{:var "e"} "agent" control]}]}]}
           max-msg-run-candidates nil)
          rows (:rows response)]
      (when-not
       (and (map? response) (vector? rows)
            (true? (:done? response))
            (nil? (:cursor response))
            (integer? (:served-version response))
            (<= (count rows) max-msg-run-candidates)
            (every? #(and (vector? %) (= 1 (count %))
                          (every? string? %))
                    rows))
        (reject-msg-unavailable! "target lifecycle projection is unreadable"))
      (->> rows
           (map first)
           (filter north.terminal-projection/valid-run-entity?)
           distinct
           sort
           (mapv
            (fn [subject]
              {:subject subject
               :facts
               (into {}
                     (keep (fn [predicate]
                             (let [values (set (many port subject predicate))]
                               (when (seq values) [predicate values]))))
                     north.terminal-projection/run-resolution-predicates)}))))
    (catch Exception _
      (reject-msg-unavailable! "target lifecycle projection is unreadable"))))

(defn require-live-msg! [port control]
  (when-not (and (string? control)
                 (re-matches msg-control-pattern control))
    (reject-msg! "target is malformed"))
  (let [facts (msg-agent-facts port control)
        resolution
        (north.terminal-projection/lane-resolution
         control facts (msg-run-entries port control))
        provider (get facts "provider")
        live-input (get facts "live_input")
        live-input-state (get facts "live_input_state")]
    (when-not (north.agent-provenance/managed-valid? facts)
      (reject-msg! "target is not one exact committed managed lane"))
    (when (= :resolved (:status resolution))
      (reject-msg! "target is terminal"))
    (when (= :indeterminate (:status resolution))
      (reject-msg! "target lifecycle is inconsistent"))
    (let [online?
          (try (north.coord/session-online? port control)
               (catch Exception _ ::unavailable))]
      (when (= ::unavailable online?)
        (reject-msg-unavailable! "target liveness projection is unreadable"))
      (when-not online?
        (reject-msg! "target is offline")))
    (when-not (contains? #{"streaming" "turn-framed"} live-input)
      (reject-msg!
       (str "target adapter does not support live input"
            (when (string? provider) (str " (provider " provider ")")))))
    (when-not (= "armed" live-input-state)
      (reject-msg!
       (str "target live input is not armed"
            (when (string? live-input-state)
              (str " (state " live-input-state ")")))))
    {:identity-manifest (get facts "identity_manifest_sha256")}))

(def fresh-id north.message-id/fresh-id)

;; --- command-as-facts --------------------------------------------------------
;; A command is NOT an opaque {:op :args} EDN blob in one `body` cell (the old cargo-cult,
;; whose parse-envelope parser was duplicated across this file + north-listen.clj "MUST
;; stay in sync"). It is FACTS on @cmd:<id>: `op` + `target` (routing handle) + one fact
;; per arg, so the graph can query/supersede/attach-provenance to each, and the consumer
;; drives off fact-patterns (a Datalog rule), never a string parse.
;;
;; Every invocation mints a fresh command id: two legitimate identical commands
;; are two executions. An optional explicit idempotency key derives a stable id
;; only for transport-level retry. `retry` reactivates the same command entity.
;;
;; known-ops = a CLOSED VOCAB held as facts (@cmd:vocab known_op …), validated at intake —
;; single-source + queryable, not a #{…} set duplicated in two files.
(def vocab-subj  "@cmd:vocab")
(def default-ops ["tell" "acquire"])
(def supported-ops (set default-ops))
(defn known-ops [port] (set (many port vocab-subj "known_op")))
(defn ensure-vocab! [port]
  (let [result
        (north.coord/publish!
         port
         [{:op :set :subject vocab-subj :predicate "known_op"
           :values default-ops :cardinality :many}])]
    (when (:reject result)
      (reject-message! (str "command vocabulary publication rejected: "
                            (:reject result))))
    supported-ops))

(def canonical-value north.command-id/canonical-value)
(def content-id north.command-id/content-id)
(def command-id north.command-id/command-id)

(defn arg-pred [k] (str/replace (name k) "-" "_"))   ; :ttl-ms -> "ttl_ms"

(defn parse-args
  "Read the <args-edn> map. The SDK's command_peer emits ref values (@id, @lease:x) RAW —
   valid north refs but not EDN (edn rejects a leading @), so quote bare @-tokens first;
   the @-string value is then stored as a fact and the engine's ref-shape makes it a link."
  [s]
  ;; Parse valid EDN first. Rewriting first corrupted already-quoted refs such as
  ;; `"@thread:x"` by consuming their closing quote and double-quoting them.
  (try
    (edn/read-string (str s))
    (catch Exception _
      (try (edn/read-string (str/replace (str s) #"@[^\s,}\]]+" #(str \" % \")))
           (catch Exception _ ::bad)))))

(defn canonical-json-value [value]
  (cond
    (map? value)
    (into
     (sorted-map-by
      #(compare (canonical-value %1) (canonical-value %2)))
     (map (fn [[key item]] [key (canonical-json-value item)]))
     value)
    (set? value)
    (mapv canonical-json-value
          (sort-by canonical-value value))
    (sequential? value)
    (mapv canonical-json-value value)
    (keyword? value) (name value)
    :else value))

(defn encoded-arg [value]
  ;; Structured staffing values cross the fact bus as canonical JSON. `(str v)`
  ;; produced EDN maps/vectors that routingMetadataFromEnv could not parse.
  (cond
    (or (map? value) (sequential? value) (set? value))
    (json/generate-string (canonical-json-value value))
    (keyword? value) (name value)
    :else (str value)))

(defn wake-command! [port command target]
  ;; Fram's scoped subscription contract routes only commits whose predicate is
  ;; `to` or `target`. A fresh wake subject preserves command history while its
  ;; target fact supplies the address-bearing activation edge.
  (let [wake (str "@cmd-wake:" (java.util.UUID/randomUUID))]
    (let [result
          (north.coord/publish!
           port
           [{:op :set :subject wake :predicate "retry_command"
             :values [command] :cardinality :one}
            {:op :set :subject wake :predicate "target"
             :values [target] :cardinality :one}])]
      (when (:reject result)
        (reject-message! (str wake " publication rejected: " (:reject result)))))
    wake))

(defn publish-facts! [port subject facts]
  (let [result
        (north.coord/publish!
         port
         (mapv (fn [[predicate value]]
                 {:op :set :subject subject :predicate predicate
                  :values [(str value)] :cardinality :one})
               facts))]
    (when (:reject result)
      (reject-message! (str subject " publication rejected: " (:reject result))))
    result))

(defn publish-message!
  "Publish one complete human-message envelope. EXTRA-FRONT-FACTS are committed
   in the same atomic batch as the ordinary envelope and therefore precede the
   final `to` delivery trigger. Existing send/msg/broadcast behavior remains
   on this one publication seam."
  [port dead-drop? from requested-to subj body extra-front-facts]
  (validate-message-input! from requested-to subj body)
  (let [to (admitted-message-recipient! port requested-to dead-drop?)
        msg? (= "msg" (some-> subj str str/trim str/lower-case))
        msg-admission (when msg?
                          (north.topology-authority/require-coordination! "msg")
                          (require-live-msg! port to))
        e (str "@msg:" (fresh-id from))
        ;; Canonicalize the managed control type. Ordinary subjects retain their
        ;; original spelling; every producer-admitted control message is exactly "msg".
        ;; All message fields are write-once on a fresh @msg. `to` lands LAST
        ;; (the listener trigger); the atomic publication guarantees completeness.
        front-facts
        (into [["from" from]
               ["subject" (if msg? "msg" (or subj ""))]
               ["body" (or body "")]
               ["sent_at" (str (java.time.Instant/now))]]
              extra-front-facts)
        complete-front-facts
        (cond-> front-facts
          msg-admission
          (conj [target-identity-manifest-predicate
                 (:identity-manifest msg-admission)]))]
    ;; `north msg` labels its control message exactly `msg`. Ordinary
    ;; worker -> coordinator completion/death mail remains legal; peer control
    ;; does not become legal merely because the producer bypassed agents-cli.
    (when msg-admission
      ;; Message's `to` lands through its own CAS below because route validation
      ;; must observe the exact version used by the publication.
      ;; Publish the complete front atomically first.
      (publish-facts! port e complete-front-facts))
    ;; A broadcast's concrete recipients are durable facts, captured before
    ;; `to` lands. Sender exclusion is intentional: broadcast means peers.
    (let [broadcast-audience
          (when (= north.message-audience/broadcast-address to)
            (north.message-audience/snapshot-broadcast! port e from))]
      (if msg-admission
        ;; This is the message acceptance linearization point. Every
        ;; load-bearing route read follows the global BASE capture, then Fram
        ;; compares BASE + lands `to` in one serialized writer turn. A freeze
        ;; between validation and this assert conflicts, retries the whole
        ;; route read, and cannot leave an accepted post-freeze message.
        (let [admitted-manifest (:identity-manifest msg-admission)
              result
              (north.coord/assert-after-read!
               port e "to" to
               (fn []
                 (let [current (require-live-msg! port to)
                       stored (one port e target-identity-manifest-predicate)]
                   (when-not (and (= admitted-manifest stored)
                                  (= admitted-manifest
                                     (:identity-manifest current)))
                     (reject-msg!
                      "target route changed during message admission"))
                   true)))]
          (when (:reject result)
            (reject-msg!
             "target route changed during message admission")))
        ;; Ordinary mail: every front fact plus `to` publishes as ONE
        ;; all-or-none unit.
        (publish-facts! port e (conj complete-front-facts ["to" to])))
      (println (str (if msg? "queued for live injection " "sent ") e " -> " to
                    (when broadcast-audience
                      (str " (" (count broadcast-audience)
                           " snapshotted recipients; sender excluded)"))))
      e)))

(def about-ref-pattern #"^@[A-Za-z0-9][A-Za-z0-9._:-]*$")

(defn parse-directed-attention!
  "Parse `<from> <recipient> [--about <@thread>] <body>`. Options are accepted
   only before the single body argument so malformed and duplicate forms have
   one deterministic rejection."
  [verb args]
  (let [[from requested-to & tail] args]
    (when (or (nil? from) (nil? requested-to))
      (reject-message!
       (str verb " requires from, recipient, optional --about <@thread>, and body")))
    (loop [remaining tail
           about nil]
      (let [arg (first remaining)]
        (cond
          (nil? arg)
          (reject-message!
           (str verb " requires exactly one non-option body argument"))

          (= "--about" arg)
          (cond
            about
            (reject-message! (str verb " received duplicate --about"))

            (or (nil? (second remaining))
                (str/starts-with? (second remaining) "--"))
            (reject-message! (str verb " --about requires an @thread value"))

            :else
            (recur (nnext remaining) (second remaining)))

          (str/starts-with? arg "--")
          (reject-message! (str verb " received unknown option " arg))

          (next remaining)
          (reject-message!
           (str verb " requires exactly one body argument after options"))

          :else
          {:from from :requested-to requested-to :about about :body arg})))))

(defn validate-about!
  [port about]
  (when about
    (let [kind (one port about "kind")]
      (when-not (and (string? about)
                     (<= (north.message-contract/utf8-bytes about)
                         north.message-contract/max-target-bytes)
                     (re-matches about-ref-pattern about)
                     (not (str/blank? (one port about "title")))
                     (or (nil? kind) (= "thread" kind)))
        (reject-message!
         (str "--about must be an exact @ref resolving to a title-bearing thread: "
              about)))))
  about)

(defn publish-directed-attention! [port verb args]
  (let [{:keys [from requested-to about body]}
        (parse-directed-attention! verb args)
        about (validate-about! port about)
        mention? (= verb "mention")
        subject (if mention? "mention" "URGENT")
        attention-kind (if mention? "mention" "interrupt")
        delivery-class (if mention? "inbox" "interrupt")
        extra-front-facts
        (cond-> [["attention_kind" attention-kind]
                 ["delivery_class" delivery-class]
                 ["requires_ack" "true"]]
          about (conj ["about" about]))]
    ;; Mention is a deliberate durable dead drop to a stable address. Interrupt
    ;; retains the canonical live-recipient admission gate.
    (publish-message! port mention? from requested-to subject body
                      extra-front-facts)))

(when-not (= "1" (System/getProperty "north.msg-cli.lib"))
 (let [[port verb & args] *command-line-args*
      port (Integer/parseInt port)]
  (case verb
    "send"        ; [--dead-drop] <from> <to> "<subject>" "<body>"  — human mail
    (let [dead-drop? (= "--dead-drop" (first args))
          args (if dead-drop? (vec (rest args)) args)
          [from requested-to subj body] args
          _ (when-not (= 4 (count args))
              (reject-message!
               "send requires [--dead-drop] plus exactly from, to, subject, and body"))
          _ (publish-message! port dead-drop? from requested-to subj body [])]
      nil)

    "mention"
    (publish-directed-attention! port "mention" args)

    "interrupt"
    (publish-directed-attention! port "interrupt" args)

    "inbox"       ; <me>  — direct-to-me OR finite broadcast audience, minus acked_by
    (let [[me] args]
      (println (format "%-28s %-10s %s" "MSG-ID" "FROM" "SUBJECT"))
      (doseq [e (sort (north.message-audience/pending-message-ids port me #{me}))]
        (println (format "%-28s %-10s %s" (subs e 5) (or (one port e "from") "?") (or (one port e "subject") "")))))

    "thread"      ; <msg-id>
    (let [[id] args, e (str "@msg:" id)]
      (doseq [p ["from" "to" "subject" "body" "sent_at"
                 north.message-audience/audience-version-predicate]]
        (println (format "%-9s %s" p (or (one port e p) "-"))))
      (println (str "broadcast_to: "
                    (str/join ", " (many port e north.message-audience/audience-predicate))))
      (println (str "acked_by: " (str/join ", " (many port e "acked_by"))))
      (println (str "delivery_rejected_by: "
                    (str/join ", " (many port e "delivery_rejected_by"))))
      (doseq [rejection (many port e "delivery_rejection")]
        (println (str "delivery_rejection: " rejection))))

    "ack"         ; <me> <msg-id-or-cmd-id>  — works for @msg and @cmd subjects
    (let [[me id] args, e (if (str/starts-with? (str id) "@") id (str "@msg:" id))]
      (when (and (str/starts-with? e "@msg:")
                 (not (north.message-audience/deliverable?
                       port e (one port e "to") me #{me})))
        (println (str "REJECTED: " e " is not addressed to " me))
        (System/exit 2))
      (let [result
            (north.coord/publish!
             port
             [{:op :assert :subject e :predicate "acked_by"
               :value me :cardinality :many}
              {:op :assert :subject e :predicate "acked_at"
               :value (str (java.time.Instant/now)) :cardinality :one}])]
        (when (:reject result)
          (reject-message! (str e " acknowledgement rejected: " (:reject result)))))
      (println (str me " acked " e)))

    "send-cmd"    ; <from> <target> <op> "<args-edn>" [idempotency-key]
    (do
      ;; This is the lowest command producer. Guard before ensure-vocab!: that
      ;; helper can itself seed facts, so even its idempotent write is too late.
      (north.topology-authority/require-coordination! "send-cmd")
      (let [[from target op args-edn idempotency-key] args
          ops  (ensure-vocab! port)
          argm (parse-args (or args-edn "{}"))]
      (cond
        (not (contains? ops op))
        (do (println (str "REJECTED: unknown op " (pr-str op) " (known: " (str/join " " (sort ops)) ")")) (System/exit 2))
        (= argm ::bad)
        (do (println "REJECTED: <args-edn> is not valid EDN") (System/exit 2))
        (not (map? argm))
        (do (println "REJECTED: <args-edn> must be an EDN map") (System/exit 2))
        :else
        (let [e (str "@cmd:" (command-id op argm target idempotency-key))]
          (publish-facts!
           port e
           (concat (map (fn [[k v]] [(arg-pred k) (encoded-arg v)]) argm)
                   [["from" from] ["op" op] ["target" target]]))
          (println (str "sent cmd " e " op=" op " -> " target "  args=" (pr-str argm)))))))

    "retry"       ; <cmd-id> — explicit reactivation of a terminal failed command
    (do
      (north.topology-authority/require-coordination! "retry command")
      (let [[id] args
            e (if (str/starts-with? (str id) "@cmd:") id (str "@cmd:" id))
            facts (get-in (north.coord/show-many port [e]) [:rows e])
            by-predicate (reduce (fn [values [predicate value]]
                                   (update values predicate (fnil conj []) value))
                                 {} facts)
            failures (get by-predicate "failed_by" [])
            retryable (first (get by-predicate "retryable"))
            target (first (get by-predicate "target"))
            requested (get by-predicate "retry_requested" [])
            acknowledged (get by-predicate "acked_by" [])]
        (cond
          (and (not (seq failures)) (seq requested) (not (seq acknowledged)) target)
          (do
            ;; Recovery for a producer that cleared failed_by and died before
            ;; publishing the addressed wake below. A repeated retry completes
            ;; the same activation rather than rejecting a now-markerless cmd.
            (wake-command! port e target)
            (println (str "retry wake replayed for " e)))
          (not (seq failures))
          (do (println (str "REJECTED: " e " is not terminal-failed")) (System/exit 2))
          (not= "true" retryable)
          (do (println (str "REJECTED: " e " is terminal non-retryable")) (System/exit 2))
          (str/blank? (str target))
          (do (println (str "REJECTED: " e " has no routing target")) (System/exit 2))
          :else
          (let [wake (str "@cmd-wake:" (java.util.UUID/randomUUID))
                result
                (north.coord/publish!
                 port
                 (concat
                  [{:op :assert :subject e :predicate "retry_requested"
                    :value (str (java.time.Instant/now)) :cardinality :many}]
                  (for [predicate ["execution_status" "failed_at" "retryable" "reply"]]
                    {:op :set :subject e :predicate predicate
                     :values [] :cardinality :many})
                  [{:op :set :subject e :predicate "failed_by"
                    :values [] :cardinality :many}
                   {:op :set :subject wake :predicate "retry_command"
                    :values [e] :cardinality :one}
                   {:op :set :subject wake :predicate "target"
                    :values [target] :cardinality :one}]))]
            (when (:reject result)
              (reject-message! (str e " retry publication rejected: "
                                    (:reject result))))
            (println (str "retry requested for " e))))))

    "cmd"         ; <cmd-id>  — show ALL facts on a command (it is a queryable subject now)
    (let [[id] args, e (str "@cmd:" id)
          rows (north.coord/query-rows
                port
                {:find "pv"
                 :rules [{:head {:rel "pv" :args [{:var "p"} {:var "o"}]}
                          :body [{:rel "triple" :args [e {:var "p"} {:var "o"}]}]}]})]
      (if (seq rows)
        (doseq [[p o] (sort rows)] (println (format "%-12s %s" p o)))
        (println (str "no facts on " e))))

    "cmds"        ; [target]  — list PENDING commands (no acked_by), optionally scoped to a target
    (let [rows (sort (or (north.coord/pending-cmds port) []))
          [tgt] args]
      (println (format "%-24s %-10s %s" "CMD" "OP" "TARGET"))
      (doseq [[c op t] rows]
        (when (or (nil? tgt) (= t tgt))
          (println (format "%-24s %-10s %s" c op t)))))

    (do
      (println
       "usage: msg-cli.clj <port> {send [--dead-drop]|mention|interrupt|send-cmd|retry|cmd|cmds|inbox|thread|ack}")
      (System/exit 2)))))
