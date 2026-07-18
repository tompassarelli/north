;; msg-cli.clj — messaging-as-facts (North gate-2, primitive 3) + command-as-facts.
;; A message = @msg:<id> facts (human mail); a COMMAND = @cmd:<id> facts (op/target/args
;; each a separate fact, NEVER an opaque {:op :args} body blob). ack = a fact (acked_by);
;; inbox/done/pending = DERIVED queries. The coordinator STORES + (with scoped-subscribe)
;; NOTIFIES; it never ROUTES. Wire (daemon): :assert / :version / :query / :resolved.
(require '[cheshire.core :as json]
         '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str])

;; Reply-schema sidecar (the old rec4 JSON-Schema field + `validate` verb + schema-validate.clj)
;; is GONE (assessment §3.3): it reimplemented a JSON-Schema engine duplicating the coordinator's
;; own commit-time rule-check (closed-vocab/cardinality/dangling-ref). A reply is now just a FACT
;; — the coordinator's commit rule-check IS the validator; a rejected fact IS the invalid reply.

;; shared coord substrate: cardinality-typed write verbs + the command-as-facts
;; pending rule (move-C) live once in cli/coord.clj. append! = MULTI coexist; put! =
;; SINGLE last-writer-wins; pending-cmds = the single Datalog rule the reactor shares.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(def send-op north.coord/send-op)
(def append! north.coord/append!)
(def put!    north.coord/put!)
(def one     north.coord/resolved)
(def many    north.coord/many)

(defn fresh-id [from]   ; yyyyMMdd-HHmmss-<from>-<4hex>: ts prefix sorts, hex suffix dodges same-second aliasing
  (str (.format (java.time.LocalDateTime/now)
                (java.time.format.DateTimeFormatter/ofPattern "yyyyMMdd-HHmmss"))
       "-" from "-" (format "%04x" (rand-int 0x10000))))

;; --- command-as-facts --------------------------------------------------------
;; A command is NOT an opaque {:op :args} EDN blob in one `body` cell (the old cargo-cult,
;; whose parse-envelope parser was duplicated across this file + north-listen.clj "MUST
;; stay in sync"). It is FACTS on @cmd:<id>: `op` + `target` (routing handle) + one fact
;; per arg, so the graph can query/supersede/attach-provenance to each, and the reactor
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
  ;; Converge stale live vocab facts too. Older generations advertised peer
  ;; spawn/dispatch; code-owned support must fail closed even before this cleanup.
  (let [known (known-ops port)]
    (doseq [op (remove supported-ops known)]
      (north.coord/retract! port vocab-subj "known_op" op))
    (doseq [op (remove known default-ops)] (append! port vocab-subj "known_op" op))
    supported-ops))

(defn content-id
  "Stable id only when a caller explicitly supplies an idempotency key."
  [op args target idempotency-key]
  (let [canon (str idempotency-key " " op " " (pr-str (into (sorted-map) args)) " " target)
        bs    (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes canon "UTF-8"))]
    (apply str (map #(format "%02x" %) (take 8 bs)))))

(defn command-id [op args target idempotency-key]
  (if (str/blank? (str idempotency-key))
    (str (java.util.UUID/randomUUID))
    (content-id op args target idempotency-key)))

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

(defn encoded-arg [value]
  ;; Structured staffing values cross the fact bus as canonical JSON. `(str v)`
  ;; produced EDN maps/vectors that routingMetadataFromEnv could not parse.
  (cond
    (or (map? value) (sequential? value) (set? value)) (json/generate-string value)
    (keyword? value) (name value)
    :else (str value)))

(defn wake-command! [port command target]
  ;; Fram's scoped subscription contract routes only commits whose predicate is
  ;; `to` or `target`. A fresh wake subject preserves command history while its
  ;; target fact supplies the address-bearing activation edge.
  (let [wake (str "@cmd-wake:" (java.util.UUID/randomUUID))]
    (put! port wake "retry_command" command)
    (put! port wake "target" target)
    wake))

(let [[port verb & args] *command-line-args*
      port (Integer/parseInt port)]
  (case verb
    "send"        ; <from> <to> "<subject>" "<body>"  — human mail
    (let [[from to subj body] args
          e (str "@msg:" (fresh-id from))]
      ;; `north steer` labels its control message exactly `steer`. Ordinary
      ;; worker -> coordinator completion/death mail remains legal; peer control
      ;; does not become legal merely because the producer bypassed agents-cli.
      (when (= "steer" (some-> subj str str/trim str/lower-case))
        (north.topology-authority/require-coordination! "steer"))
      ;; write content facts first, `to` LAST: the listener triggers on `to`, so landing it
      ;; last means from/subject/body are already visible — no settle race, no sleep.
      (put! port e "from" from)              ; single — all message fields are write-once on a fresh @msg
      (put! port e "subject" (or subj ""))
      (put! port e "body" (or body ""))
      (put! port e "sent_at" (str (java.time.Instant/now)))
      ;; A broadcast's concrete recipients are durable facts, captured before
      ;; `to` lands. Sender exclusion is intentional: broadcast means peers.
      (let [broadcast-audience
            (when (= north.message-audience/broadcast-address to)
              (north.message-audience/snapshot-broadcast! port e from))]
        (put! port e "to" to)
        (println (str "sent " e " -> " to
                      (when broadcast-audience
                        (str " (" (count broadcast-audience)
                             " snapshotted recipients; sender excluded)"))))))

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
      (println (str "acked_by: " (str/join ", " (many port e "acked_by")))))

    "ack"         ; <me> <msg-id-or-cmd-id>  — works for @msg and @cmd subjects
    (let [[me id] args, e (if (str/starts-with? (str id) "@") id (str "@msg:" id))]
      (when (and (str/starts-with? e "@msg:")
                 (not (north.message-audience/deliverable?
                       port e (one port e "to") me #{me})))
        (println (str "REJECTED: " e " is not addressed to " me))
        (System/exit 2))
      (append! port e "acked_by" me)                       ; multi (many ackers)
      (put!    port e "acked_at" (str (java.time.Instant/now))) ; single
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
          ;; arg facts + provenance + op first; `target` (the routing key the reactor
          ;; triggers on) LAST → op/args already visible when it lands (no settle race).
          ;; All write-once (put!): a re-send re-asserts identical facts = idempotent no-op.
          (doseq [[k v] argm] (put! port e (arg-pred k) (encoded-arg v)))
          (put! port e "from" from)
          (put! port e "op" op)
          (put! port e "target" target)
          (println (str "sent cmd " e " op=" op " -> " target "  args=" (pr-str argm)))))))

    "retry"       ; <cmd-id> — explicit reactivation of a terminal failed command
    (do
      (north.topology-authority/require-coordination! "retry command")
      (let [[id] args
            e (if (str/starts-with? (str id) "@cmd:") id (str "@cmd:" id))
            failures (many port e "failed_by")
            retryable (one port e "retryable")
            target (one port e "target")
            requested (many port e "retry_requested")
            acknowledged (many port e "acked_by")]
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
          (do
            ;; Durable retry intent first. If this process dies while failed_by
            ;; remains, the command stays terminal. If it dies after clearing
            ;; failed_by, the recovery branch above republishes the wake.
            (append! port e "retry_requested" (str (java.time.Instant/now)))
            (doseq [predicate ["execution_status" "failed_at" "retryable" "reply"]
                    value (many port e predicate)]
              (north.coord/retract! port e predicate value))
            (doseq [value failures]
              (north.coord/retract! port e "failed_by" value))
            ;; Scoped subscribers match addresses on the commit itself; a
            ;; failed_by retraction carries no address and cannot wake them.
            ;; Publish an explicit addressed activation edge LAST.
            (wake-command! port e target)
            (println (str "retry requested for " e))))))

    "cmd"         ; <cmd-id>  — show ALL facts on a command (it is a queryable subject now)
    (let [[id] args, e (str "@cmd:" id)
          rows (:ok (send-op port {:op :query
                                   :query {:find "pv"
                                           :rules [{:head {:rel "pv" :args [{:var "p"} {:var "o"}]}
                                                    :body [{:rel "triple" :args [e {:var "p"} {:var "o"}]}]}]}}))]
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

    (do (println "usage: msg-cli.clj <port> {send|send-cmd|retry|cmd|cmds|inbox|thread|ack}") (System/exit 2))))
