;; concern-cli.clj — concern-level coordination for parallel agents, not locks.
;; Concerns coexist. Their canonical path footprints are published to the
;; coordination SpaceId, and overlap is derived from indexed predicate/object
;; reads. A separate code store is admitted only through an explicit Store RPC
;; endpoint; the current port/log-only configuration fails closed.
;;
;; usage (port = north threads, 7977):
;;   declare <agent> <repo> "<intent>" <foot,foot,...> [--about <@thread>]
;;                                                     mint a concern (+ shows overlaps)
;;       footprint entries are canonical paths.
;;   overlap <concern-id> [--landing]   who else is in my path footprint;
;;       likely-to-land entries are MARKED — build against them.
;;       --landing filters to likely-to-land only.
;;   ls [<repo>]              active concerns
;;   candidate <concern-id> [<git-rev>] record an exact commit and reach likely-to-land
;;   status  <concern-id> <exploring|building|likely-to-land|landed>   append a maturity level;
;;       likely-to-land records the selected candidate revision first.
;;   done    <concern-id>     reach `landed`
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[clojure.set :as set] '[clojure.java.shell :as shell]
         '[cheshire.core :as json])

;; Shared coordination substrate. Reads and writes use the canonical Store RPC
;; facade; this CLI never constructs transport operations itself.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/concern-spool.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/concern-spool-reconcile.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/attention.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/../out/north/worker_policy.clj"))
(def many     north.coord/many!)
(def resolved north.coord/resolved!)

(defn- require-publication! [message response]
  (when (:reject response)
    (throw
     (ex-info message
              {:type :concern-write-rejected
               :response response})))
  (when-not (and (map? response)
                 (integer? (:ok response))
                 (boolean? (:changed? response))
                 (vector? (:results response)))
    (throw
     (ex-info "Store RPC acknowledgement for concern write is ambiguous"
              {:type :concern-write-transport-unknown
               :response response})))
  response)

(defn- assertion [subject predicate value]
  {:op :assert :subject subject :predicate predicate :value value})

(defn- retraction [subject predicate value]
  {:op :retract :subject subject :predicate predicate :value value})

(defn- publish-action [operation subject predicate value cardinality]
  {:op operation
   :subject subject
   :predicate predicate
   :value value
   :cardinality cardinality})

(defn- transaction-ack? [response action-count]
  (and (map? response)
       (integer? (:ok response))
       (boolean? (:changed? response))
       (vector? (:results response))
       (= action-count (count (:results response)))
       (= (range action-count) (map :input-index (:results response)))))

(defn- transact-facts! [port subject facts options]
  (let [actions
        (mapv
         (fn [{:keys [p r]}]
           (when-not (and (string? p) (not (str/blank? p))
                          (string? r))
             (throw
              (ex-info "concern transaction requires string predicate/value facts"
                       {:type :invalid-concern-transaction-fact
                        :subject subject})))
           (assertion subject p r))
         facts)
        response (north.coord/transact! port actions options)]
    (if (:reject response)
      response
      (if (transaction-ack? response (count actions))
        response
        (throw
         (ex-info "Store RPC acknowledgement for concern transaction is ambiguous"
                  {:type :concern-write-transport-unknown
                   :response response}))))))

(defn- transact-after-read!
  [port subject plan!]
  (north.coord/retry-conflicts-until!
   (north.coord/retry-deadline-ns)
   (fn []
     (let [base (:version (north.coord/show-envelope! port subject))
           planned (plan!)]
       (if (contains? planned :done)
         planned
         (let [facts (:facts planned)]
           (when-not (and (vector? facts) (seq facts))
             (throw
              (ex-info "concern transition requires a non-empty planned transaction"
                       {:type :invalid-concern-transition-plan
                        :subject subject})))
           (transact-facts! port subject facts {:expected-version base})))))))

;; ---- liveness-derived concern DECAY (design 019f4418) -----------------------
;; A concern's owner is judged live by the SAME renewable-lease rule the presence
;; roster uses. When the owner's presence has LAPSED we don't hide or delete — we
;; DECAY the render at read time (pure projection, no write): a building concern
;; goes STALE (dim + "owner lapsed <ago>"); a likely-to-land concern instead
;; renders as ORPHANED (prominent) — a RETAINED RECOVERY CANDIDATE that survives
;; owner death, a signal to the next agent to adopt (or prepare an explicit
;; handoff), not stranded WIP and NOT evidence a handoff procedure occurred.
;; Terminal stale-concern janitor verdict
;; `reached=abandoned-stale` (owner dead >24h) renders abandoned + hides by default.
(def ^:private use-color? (some? (System/console)))   ; ANSI only on a real TTY; piped/captured stays plain
(defn- dim  [] (if use-color? "\033[2m" ""))
(defn- bold [] (if use-color? "\033[1m" ""))
(defn- rst  [] (if use-color? "\033[0m" ""))

(defn ago
  "Humanize a lapse duration in ms -> \"<n>{s,m,h,d}\"; nil (no lease ever) -> \"?\"."
  [ms]
  (if (nil? ms) "?"
      (let [s (quot ms 1000)]
        (cond (< s 60)    (str s "s")
              (< s 3600)  (str (quot s 60) "m")
              (< s 86400) (str (quot s 3600) "h")
              :else       (str (quot s 86400) "d")))))

;; declare-time embedded in the id (@concern-<epoch-ms>-<hex>): the lapse lower bound
;; when a dead owner never held a lease (pre-presence agents). Matches the janitor rule.
(defn concern-mint-ms [c]
  (some-> (re-find #"concern-(\d{10,})" (str c)) second parse-long))

(defn owner-lease-liveness
  "Project one owner's canonical lease status. A lease for another holder or a
   missing expiry is absence, so lapse age falls back to concern age."
  [concern handle status now]
  (if (and (= handle (:holder status)) (integer? (:exp status)))
    (if (true? (:online? status))
      {:online true :lapsed-ago-ms nil}
      {:online false :lapsed-ago-ms (max 0 (- now (:exp status)))})
    {:online false
     :lapsed-ago-ms
     (when-let [minted (concern-mint-ms concern)]
       (max 0 (- now minted)))}))

(defn owner-liveness
  "-> {:online bool :lapsed-ago-ms nil-or-ms} for a concern meta. An agent-less
   concern can't lapse (nothing to renew) so it renders live. When an offline owner
   never held a lease, the concern's own age is the staleness lower bound (so STALE
   shows a real duration, not \"?\")."
  [port m]
  (let [a (:agent m)]
    (if (str/blank? a)
      {:online true :lapsed-ago-ms nil}
      (let [h (if (str/starts-with? a "@") (subs a 1) a)
            l (north.coord/lease-status! port (str "session:" h))
            now (System/currentTimeMillis)]
        (owner-lease-liveness (:id m) h l now)))))

(defn with-liveness [port m] (merge m (owner-liveness port m)))

(defn configuration-error! [message]
  (binding [*out* *err*]
    (println (str "concern: " message)))
  (System/exit 2))
(defn ->port [value]
  (when-not (nil? value)
    (let [text (str value)]
      (when-not (re-matches #"[0-9]+" text)
        (configuration-error!
         (str "code-store port must be an integer from 1 through 65535, got "
              (pr-str text))))
      (let [port (parse-long text)]
        (when-not (and port (<= 1 port 65535))
          (configuration-error!
           (str "code-store port must be an integer from 1 through 65535, got "
                (pr-str text))))
        (int port)))))
(defn env-value [name]
  (let [value (System/getenv name)]
    (when-not (str/blank? value) value)))

;; A code-store port is not an identity. The wrapper must supply the exact log
;; served on that port; accepting only one half would let a concern write into
;; whichever corpus happened to be listening there.
(def raw-code-port (env-value "NORTH_CODE_PORT"))
(def raw-code-log (env-value "NORTH_CODE_LOG"))
(when (not= (boolean raw-code-port) (boolean raw-code-log))
  (configuration-error!
   "NORTH_CODE_PORT and NORTH_CODE_LOG must be supplied together"))
(when (and raw-code-log (not (.isAbsolute (io/file raw-code-log))))
  (configuration-error! "NORTH_CODE_LOG must be an absolute path"))
(def code-port (some-> raw-code-port ->port))
(def code-log (some-> raw-code-log north.coord/canonical-log-path))

(def ^:dynamic *throw-code-store-errors?* false)
(defn code-store-error! [message]
  (if *throw-code-store-errors?*
    (throw
     (ex-info
      (str "concern: code-store safety check failed: " message)
      {:type :concern-code-store-error}))
    (do
      (binding [*out* *err*]
        (println (str "concern: code-store safety check failed: " message)))
      (System/exit 3))))

(defn code-store-unavailable! [log]
  (when-not (and (string? log) (.isAbsolute (io/file log)))
    (code-store-error! "code log identity must be an absolute path"))
  (throw
   (ex-info
    "code-store concern routing requires an explicit canonical Store RPC SpaceId"
    {:type :store-rpc-routing-unavailable
     :target-log log})))

(defn validate-code-store! [_port log]
  (code-store-unavailable! log))

;; concern-id args arrive from humans/agents in either form; every fact subject in
;; the log carries the @ sigil, so a bare id here writes to a PHANTOM bare node —
;; the split-brain that stranded `reached landed` facts invisibly (2026-07-02).
(defn norm-cid [c] (if (or (nil? c) (str/starts-with? c "@")) c (str "@" c)))

;; one-column datalog query: bind ?e in `body`, return the column. Reads through
;; coord/query-rows! so a coordinator error map fails closed (typed throw) instead
;; of masquerading as an empty concern list.
(defn q-col [port body]
  (->> (north.coord/query-rows!
        port {:find "e"
              :rules [{:head {:rel "e" :args [{:var "e"}]} :body body}]})
       (map first)))

(def automatic-index-row-limit 4096)

(defn indexed-subjects-for
  "Bounded predicate/object lookup for automatic workers. This must stay on
   Beagle Store's predicate/object index; an automatic path may not warm the
   whole-corpus query cache."
  [port predicate object]
  (->> (north.coord/bounded-query-in-domain!
        port
        :coordination
        {:find "indexed_subject"
         :rules
         [{:head {:rel "indexed_subject" :args [{:var "entity"}]}
           :body
           [{:rel "triple"
             :args [{:var "entity"} predicate object]}]}]}
        automatic-index-row-limit)
       :rows
       (mapv first)))

(defn indexed-predicate-rows
  "Bounded [subject value] lookup for one predicate."
  [port predicate]
  (:rows
   (north.coord/bounded-query-in-domain!
    port
    :coordination
    {:find "indexed_predicate_rows"
     :rules
     [{:head
       {:rel "indexed_predicate_rows"
        :args [{:var "entity"} {:var "value"}]}
       :body
       [{:rel "triple"
         :args [{:var "entity"} predicate {:var "value"}]}]}]}
    automatic-index-row-limit)))

;; ---- monotone maturity (decision 8: status is DERIVED, never SET) -----------
;; `reached` is an append-only, multi-valued ladder fact; status = the MAX level reached.
;; Double-report is idempotent; full history is retained; no set-single! retract-then-put.
(def maturity ["exploring" "building" "likely-to-land" "landed"])
(def maturity-idx (into {} (map-indexed (fn [i m] [m i]) maturity)))
(def concern-stale-ms (* 24 60 60 1000))
(def attention-event-intent-predicate "attention_event_intent")
(def attention-event-settled-predicate "attention_event_settled")
(def attention-reconcile-pending-predicate "attention_reconcile_pending")
(def attention-event-intent-schema
  "north-concern-attention-event-intent-v1")
(def attention-event-intent-max-bytes (* 16 1024))
(def attention-event-reconcile-limit 64)
(def terminal-attention-statuses #{"landed" "abandoned-stale"})
(def usage
  "usage: concern-cli.clj <port> {declare <agent> <repo> \"<intent>\" <foot,> [--about <@thread>] | reconcile-local | overlap <id> [--landing] | ls [repo] | list-json [repo] | candidate <id> [git-rev] | status <id> <exploring|building|likely-to-land|landed> | done <id>}")
(defn usage-error! [message]
  (binding [*out* *err*]
    (println (str "concern: " message))
    (println usage))
  (System/exit 2))
(defn existing-concern! [port raw]
  (when (str/blank? raw)
    (usage-error! "a concern id is required"))
  (let [c (norm-cid raw)]
    (when-not (= "concern" (resolved port c "kind"))
      (usage-error! (str c " is not an existing concern")))
    c))
(defn norm-ref [raw]
  (when-not (str/blank? raw)
    (if (str/starts-with? raw "@") raw (str "@" raw))))
(defn existing-thread! [port raw]
  (let [thread (norm-ref raw)
        kind (when thread (resolved port thread "kind"))]
    (when (or (nil? thread)
              (str/blank? (resolved port thread "title"))
              (and kind (not= "thread" kind)))
      (usage-error! (str (or thread raw) " is not a title-bearing thread")))
    thread))
(defn parse-declare-args!
  "Preserve the four-position declaration contract and admit one trailing
   --about ref. Parsing and canonical intent construction happen before the
   first transport turn; live validation remains a separate precondition."
  [args]
  (when-not (or (= 4 (count args))
                (and (= 6 (count args)) (= "--about" (nth args 4))))
    (usage-error!
     "declare requires <agent> <repo> <intent> <foot,> and optional --about <@thread>"))
  (let [[agent repo intent files] args
        about-raw (when (= 6 (count args)) (nth args 5))]
    {:agent agent
     :repo repo
     :intent intent
     :files files
     :about-raw about-raw
     :about (norm-ref about-raw)}))
(defn resolve-candidate! [raw]
  (let [requested (or raw "HEAD")
        revision-result
        (shell/sh "git" "rev-parse" "--verify" "--end-of-options"
                  (str requested "^{commit}"))
        git-dir-result
        (shell/sh "git" "rev-parse" "--path-format=absolute" "--git-common-dir")
        revision (str/trim (:out revision-result))
        git-dir (str/trim (:out git-dir-result))]
    (when-not (zero? (:exit revision-result))
      (usage-error!
       (str "candidate revision " (pr-str requested)
            " is not a commit in the current repository")))
    (when-not (re-matches #"(?:[0-9a-f]{40}|[0-9a-f]{64})" revision)
      (usage-error! "git did not resolve the candidate to one exact commit id"))
    (when-not (and (zero? (:exit git-dir-result))
                   (.isAbsolute (io/file git-dir))
                   (.isDirectory (io/file git-dir)))
      (usage-error! "git did not resolve one durable common repository directory"))
    {:revision revision :git-dir git-dir}))

(defn candidate-landed? [{:keys [candidate-rev candidate-git-dir]}]
  (and
   (re-matches #"(?:[0-9a-f]{40}|[0-9a-f]{64})" (or candidate-rev ""))
   (.isAbsolute (io/file (or candidate-git-dir "")))
   (.isDirectory (io/file (or candidate-git-dir "")))
   (some
    (fn [target]
      (zero?
       (:exit
        (shell/sh "git" (str "--git-dir=" candidate-git-dir)
                  "merge-base" "--is-ancestor" candidate-rev target))))
    ["refs/remotes/origin/main" "refs/heads/main"])))

(defn with-derived-landing [concern]
  (if (and (= "likely-to-land" (:status concern))
           (candidate-landed? concern))
    (assoc concern :status "landed" :derived-landed true)
    concern))
(defn status-of [port c]
  (let [reached (many port c "reached")]
    (if (seq reached)
      (->> reached (sort-by #(get maturity-idx % -1)) last)
      "building")))

;; Terminal stale-concern verdict: owner dead >24h while still building. Off the maturity
;; ladder (orthogonal to progress), so it flags the concern without shadowing status.
(defn abandoned? [port c] (contains? (set (many port c "reached")) "abandoned-stale"))

;; ---- spine reads (:7977 board) ----------------------------------------------
(defn all-concerns [port]
  (distinct (q-col port [{:rel "triple" :args [{:var "e"} "kind" "concern"]}])))

(defn touches-of [port c]
  (set (q-col port [{:rel "triple" :args [c "touches" {:var "e"}]}])))

(defn meta-of [port c]
  (with-derived-landing
   {:id c
    :kind (resolved port c "kind")
    ;; canonical ref: the board is agent-writable, so a hand-seeded concern can
    ;; carry a bare handle where `declare` would have written "@" <agent>.
    :agent (norm-cid (resolved port c "agent"))
    :about (resolved port c "about")
    :repo (resolved port c "repo")
    :intent (resolved port c "intent")
    :candidate-rev (resolved port c "candidate_rev")
    :candidate-git-dir (resolved port c "candidate_git_dir")
    :status (status-of port c)
    :abandoned (abandoned? port c)
    :code-port (resolved port c "code_port")
    :code-log (resolved port c "code_log")
    :touches (touches-of port c)}))

;; Bound to a bulk index for the duration of one CAS read phase; every other
;; caller keeps the per-subject round-trip read.
(def ^:dynamic *concern-metas* nil)

(defn concern-meta
  "One concern's meta: the bound bulk index when it holds an exact entry for
   this subject, else the per-subject read."
  [port c]
  (or (get *concern-metas* c) (meta-of port c)))

(defn active-concern?
  "Situational coordination exists only while both concerns remain open."
  [m]
  (and (= "concern" (:kind m))
       (not (:abandoned m))
       (not= "landed" (:status m))))

(defn canonical-overlap
  "One order-independent overlap record. LEFT/RIGHT and SOURCE-CONCERNS are
   canonical by concern id; SHARED is canonical set evidence."
  [mine peer shared evidence]
  (let [[left right] (sort-by :id [mine peer])
        sources [(:id left) (:id right)]]
    {:pair-key (str "concern-overlap:" (str/join ":" (map #(str/replace-first % #"^@" "") sources)))
     :source-concerns sources
     :left left
     :right right
     :shared (vec (sort (set shared)))
     :evidence evidence}))

(defn other-concern [overlap id]
  (let [{:keys [left right]} overlap]
    (cond (= id (:id left)) right
          (= id (:id right)) left
          :else nil)))

(defn transition-kind [before after]
  (cond
    (and (not (active-concern? before)) (active-concern? after))
    "overlap-entered"

    (and (active-concern? before)
         (active-concern? after)
         (not= "likely-to-land" (:status before))
         (= "likely-to-land" (:status after)))
    "likely-to-land"

    (and (active-concern? before) (not (active-concern? after)))
    "overlap-left"

    :else nil))

(defn event-recipients
  "Attention is advisory; the maturity ladder is authority. A peer whose owner
   fact is unroutable even after canonicalization loses its notification rather
   than wedging the transition that would have sent it."
  [attention-kind after overlap]
  (let [routable (fn [m] (let [a (:agent m)] (when (north.attention/ref-value? a) a)))
        both (->> [(:left overlap) (:right overlap)]
                  (keep routable)
                  distinct
                  sort)
        peer (some-> (other-concern overlap (:id after)) routable)]
    (if (= "overlap-entered" attention-kind)
      both
      (if peer [peer] []))))

(defn pair-about
  "An entered overlap is about a thread only when the pair supplies one
   unambiguous non-nil thread ref: one side names it, or both name the same one."
  [{:keys [left right]}]
  (let [abouts (set (keep :about [left right]))]
    (when (= 1 (count abouts)) (first abouts))))

(defn event-about [attention-kind after overlap]
  (if (= "overlap-entered" attention-kind)
    (pair-about overlap)
    (:about after)))

(defn attention-events-for-transition
  "Pure concern→attention integration seam. OVERLAPS are canonical records
   returned by overlap discovery. The attention publisher hashes EVENT-KEY
   with TO, so repeats converge on the same per-recipient notification."
  [before after overlaps]
  (if-let [attention-kind (transition-kind before after)]
    (->> overlaps
         (filter #(some #{(:id after)} (:source-concerns %)))
         (mapcat
          (fn [{:keys [pair-key source-concerns] :as overlap}]
            (for [to (event-recipients attention-kind after overlap)]
              {:event-key
               (str pair-key ":" attention-kind
                    (when-not (= "overlap-entered" attention-kind)
                      (str ":" (str/replace-first (:id after) #"^@" ""))))
               :to to
               :about (event-about attention-kind after overlap)
               :attention-kind attention-kind
               :delivery "notify"
               :subject
               (case attention-kind
                 "overlap-entered" "Concern overlap entered"
                 "likely-to-land" "Overlapping concern is likely to land"
                 "overlap-left" "Concern overlap ended")
               :body
               ;; The immutable body is identity-only. Shared evidence changes as
               ;; footprints evolve and therefore cannot enter an idempotency claim.
               (str/join " ↔ " source-concerns)
               :source-concerns source-concerns})))
         (sort-by (juxt :event-key :to))
         vec)
    []))

(defn utf8-byte-count [value]
  (alength
   (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn terminal-event-key [concern source-concerns]
  (str
   "concern-overlap:"
   (str/join
    ":"
    (map #(str/replace-first % #"^@" "")
         (sort source-concerns)))
   ":overlap-left:"
   (str/replace-first concern #"^@" "")))

(defn canonical-terminal-attention-event
  "Validate the exact event shape admitted to the durable terminal outbox.
   This runs both before encoding and after every read."
  [port concern trigger-status event]
  (when-not (contains? terminal-attention-statuses trigger-status)
    (throw
     (ex-info "unsupported concern attention terminal status"
              {:type :invalid-concern-attention-intent
               :trigger-status trigger-status})))
  (let [spec (north.attention/canonical-notification-spec event)
        source-concerns (:source-concerns spec)
        expected-key (terminal-event-key concern source-concerns)
        trigger-state (concern-meta port concern)
        peer-id (first (remove #{concern} source-concerns))
        peer-state (when peer-id (concern-meta port peer-id))]
    (when-not
     (and (= "overlap-left" (:attention-kind spec))
          (= "notify" (:delivery spec))
          (= "Concern overlap ended" (:subject spec))
          (= 2 (count source-concerns))
          (= 2 (count (set source-concerns)))
          (some #{concern} source-concerns)
          (= "concern" (:kind trigger-state))
          (= "concern" (:kind peer-state))
          (= (:agent peer-state) (:recipient spec))
          (= (:about trigger-state) (:about spec))
          (= expected-key (:event-key spec))
          (= (str/join " ↔ " source-concerns) (:body spec)))
      (throw
       (ex-info "concern attention terminal event is not canonical"
                {:type :invalid-concern-attention-intent
                 :concern concern
                 :trigger-status trigger-status})))
    spec))

(defn attention-event-intent-value
  "Encode one bounded canonical EDN vector. A vector avoids map-order aliases,
   and every field is revalidated when reconciliation reads it."
  [port concern trigger-status event]
  (let [{:keys [event-key recipient about attention-kind delivery subject body
                source-concerns]}
        (canonical-terminal-attention-event port concern trigger-status event)
        encoded
        (pr-str
         [attention-event-intent-schema trigger-status event-key recipient about
          attention-kind delivery subject body source-concerns])]
    (when (> (utf8-byte-count encoded) attention-event-intent-max-bytes)
      (throw
       (ex-info "concern attention event intent exceeds its byte bound"
                {:type :invalid-concern-attention-intent
                 :max-bytes attention-event-intent-max-bytes})))
    encoded))

(defn parse-attention-event-intent
  "Decode and canonicalize an outbox value. Tagged values, oversized input,
   shape aliases, and semantically forged terminal events fail closed."
  [port concern encoded]
  (try
    (when-not (and (string? encoded)
                   (<= (utf8-byte-count encoded)
                       attention-event-intent-max-bytes))
      (throw
       (ex-info "concern attention event intent is missing or too large" {})))
    (let [value
          (edn/read-string
           {:readers {}
            :default
            (fn [tag _]
              (throw
               (ex-info "tagged values are forbidden in attention intents"
                        {:tag tag})))}
           encoded)]
      (when-not (and (vector? value)
                     (= 10 (count value))
                     (= attention-event-intent-schema (nth value 0)))
        (throw
         (ex-info "concern attention event intent has the wrong shape" {})))
      (let [[_ trigger-status event-key recipient about attention-kind delivery
             subject body source-concerns]
            value
            event
            {:event-key event-key
             :to recipient
             :about about
             :attention-kind attention-kind
             :delivery delivery
             :subject subject
             :body body
             :source-concerns source-concerns}
            canonical
            (canonical-terminal-attention-event
             port concern trigger-status event)]
        (when-not (= encoded
                     (attention-event-intent-value
                      port concern trigger-status event))
          (throw
           (ex-info "concern attention event intent is not canonical EDN" {})))
        {:trigger-status trigger-status
         :event
         {:event-key (:event-key canonical)
          :to (:recipient canonical)
          :about (:about canonical)
          :attention-kind (:attention-kind canonical)
          :delivery (:delivery canonical)
          :subject (:subject canonical)
          :body (:body canonical)
          :source-concerns (:source-concerns canonical)}}))
    (catch Exception error
      (if (= :invalid-concern-attention-intent
             (:type (ex-data error)))
        (throw error)
        (throw
         (ex-info "invalid durable concern attention event intent"
                  {:type :invalid-concern-attention-intent
                   :concern concern}
                  error))))))

(defn publish-attention-events! [port events]
  (mapv #(north.attention/publish-notification! port %) events))

(defn publish-transition!
  [port before after before-overlaps after-overlaps]
  (let [kind (transition-kind before after)
        overlaps (if (= "overlap-left" kind)
                   before-overlaps
                   after-overlaps)]
    (publish-attention-events!
     port
     (attention-events-for-transition before after overlaps))))

(defn desired-events-for-overlap
  "Materialize the durable current-state attention projection for one active
   pair: one entered event per owner, plus one peer event for each side that is
   currently likely-to-land."
  [overlap]
  (let [{:keys [left right]} overlap
        entered (attention-events-for-transition nil left [overlap])
        likely
        (mapcat
         (fn [concern]
           (when (= "likely-to-land" (:status concern))
             (attention-events-for-transition
              (assoc concern :status "building")
              concern
              [overlap])))
         [left right])]
    (vec (concat entered likely))))

;; `ls` is a whole-corpus view. Reading seven fields per concern made its runtime
;; grow linearly with historical concern count (>8s in the live corpus). Fetch
;; each required predicate once from LIVE coordinator state instead. This keeps
;; declared-single supersession exact and preserves all live multi values.
(def concern-list-predicates
  ["kind" "agent" "repo" "intent" "candidate_rev" "candidate_git_dir" "reached" "code_port" "code_log"
   "touches"])

;; meta-of's exact field set: `about` reaches attention events, and owner leases
;; are not concern facts — liveness is recomputed per render, never indexed.
(def concern-meta-predicates
  (conj concern-list-predicates "about"))

;; Cardinality-single on the board: the bulk projection equals meta-of's
;; resolved read only while each of these has at most one live value.
(def concern-single-predicates
  ["kind" "agent" "about" "repo" "intent" "candidate_rev" "candidate_git_dir" "code_port" "code_log"])

(defn add-live-rows [facts predicate rows]
  (reduce (fn [current [entity value]]
            (update-in current [entity predicate] (fnil conj #{}) value))
          facts rows))

(defn concern-list-facts
  ([port]
   (let [facts (concern-list-facts port concern-list-predicates)
         handles
         (->> facts
              (keep
               (fn [[_ predicates]]
                 (when (= #{"concern"} (get predicates "kind"))
                   (let [agents (get predicates "agent" #{})]
                     (when (= 1 (count agents))
                       (let [agent (first agents)]
                         (if (str/starts-with? agent "@")
                           (subs agent 1)
                           agent)))))))
              distinct
              vec)
         statuses (if (seq handles)
                    (:sessions (north.coord/sessions-status! port handles))
                    {})]
     (assoc facts ::owner-statuses statuses)))
  ([port predicates]
   (reduce
    (fn [facts predicate]
      (add-live-rows
       facts predicate
       (north.coord/agg-rows!
        port ["e" "r"]
        [{:rel "triple" :args [{:var "e"} predicate {:var "r"}]}])))
    {}
    predicates)))

(defn singleton-live [facts subject predicate]
  (let [values (get-in facts [subject predicate] #{})]
    (when (= 1 (count values)) (first values))))

(defn status-from-live [facts concern]
  (let [reached (get-in facts [concern "reached"] #{})]
    (if (seq reached)
      (last (sort-by #(get maturity-idx % -1) reached))
      "building")))

(defn liveness-from-live [facts concern agent now]
  (if (str/blank? agent)
    {:online true :lapsed-ago-ms nil}
    (let [handle (if (str/starts-with? agent "@") (subs agent 1) agent)
          status (get-in facts [::owner-statuses handle])]
      (owner-lease-liveness concern handle status now))))

(defn meta-from-live [facts concern now]
  (let [agent (singleton-live facts concern "agent")
        reached (get-in facts [concern "reached"] #{})]
    (with-derived-landing
     (merge
      {:id concern
       :kind (singleton-live facts concern "kind")
       :about (singleton-live facts concern "about")
       :reached reached
       :agent agent
       :repo (singleton-live facts concern "repo")
       :intent (singleton-live facts concern "intent")
       :candidate-rev (singleton-live facts concern "candidate_rev")
       :candidate-git-dir (singleton-live facts concern "candidate_git_dir")
       :status (status-from-live facts concern)
       :abandoned (contains? reached "abandoned-stale")
       :code-port (singleton-live facts concern "code_port")
       :code-log (singleton-live facts concern "code_log")
       :touches (get-in facts [concern "touches"] #{})}
      (liveness-from-live facts concern agent now)))))

(defn concerns-from-live [facts]
  (->> facts
       (keep (fn [[entity predicates]]
               (when (= #{"concern"} (get predicates "kind")) entity)))
       distinct))

(defn bulk-meta-exact? [facts concern]
  (every? #(<= (count (get-in facts [concern %] #{})) 1)
          concern-single-predicates))

(defn concern-meta-from-exact-rows [concern rows]
  (let [facts
        (reduce
         (fn [current [predicate value]]
           (update-in current [concern predicate] (fnil conj #{}) value))
         {}
         rows)]
    (when (and (= #{"concern"} (get-in facts [concern "kind"] #{}))
               (bulk-meta-exact? facts concern))
      (-> (meta-from-live facts concern (System/currentTimeMillis))
          (update :agent norm-cid)
          (dissoc :online :lapsed-ago-ms)))))

(defn concern-meta-index
  "One bulk read -> {concern-id meta}, so a read phase costs a fixed number of
   queries instead of one nine-round-trip meta-of per concern. A subject whose
   single-valued facts are ambiguous is OMITTED: concern-meta then falls back to
   the resolved read, so the index can never disagree with meta-of."
  [port]
  (let [facts (concern-list-facts port concern-meta-predicates)
        now (System/currentTimeMillis)]
    (reduce
     (fn [index c]
       (if (bulk-meta-exact? facts c)
         (assoc index c (-> (meta-from-live facts c now)
                            (update :agent norm-cid)
                            (dissoc :online :lapsed-ago-ms)))
         index))
     {}
     (concerns-from-live facts))))

;; ---- strict versioned MACHINE projection (design 019f4418) ------------------
;; The liveness class a machine consumer (the dashboard) needs, derived from the
;; SAME lease decay the human render uses, but emitted as a strict versioned JSON
;; document so no consumer ever scrapes rendered text. The class is exactly one of:
;;   live     — owner online
;;   stale    — owner lapsed, still building (dead-agent WIP)
;;   orphaned — owner lapsed, likely-to-land: a RETAINED RECOVERY CANDIDATE that
;;              survives owner death (formerly mislabeled HANDOFF — an owner
;;              disappearing is NOT evidence a handoff procedure occurred)
;;   retired  — janitor verdict abandoned-stale (owner dead >24h)
;; `retired` is also carried as an explicit boolean so a consumer can exclude
;; retired rows without re-deriving the class. landed concerns are OMITTED (done).
(def concern-projection-version 1)

(defn classification-of [m]
  (cond
    (:abandoned m)                   "retired"
    (:online m)                      "live"
    (= (:status m) "likely-to-land") "orphaned"
    :else                            "stale"))

(defn projection-row [m]
  {:id (:id m)
   :agent (:agent m)
   :repo (:repo m)
   :intent (:intent m)
   :maturity (:status m)
   :classification (classification-of m)
   :online (boolean (:online m))
   :retired (boolean (:abandoned m))
   :touches (vec (sort (:touches m)))})

(defn concern-projection [port repo]
  (let [facts (concern-list-facts port)
        now (System/currentTimeMillis)
        rows (->> (concerns-from-live facts)
                  (map #(meta-from-live facts % now))
                  (remove #(= (:status %) "landed"))
                  (filter #(or (nil? repo) (= (:repo %) repo)))
                  (sort-by (juxt :repo #(str (:agent %))))
                  (mapv projection-row))]
    {:version concern-projection-version :concerns rows}))

(defn fmt [m]
  (str
   (format "  %-12s %-14s %-10s {%s}\n     ↳ %s  (%s)"
           (or (:agent m) "?") (or (:status m) "?") (or (:repo m) "?")
           (str/join " " (sort (:touches m))) (or (:intent m) "") (:id m))
   (when (= "likely-to-land" (:status m))
     (str "\n       candidate "
          (or (:candidate-rev m) "<missing — candidate is not actionable>")))))

;; Render one concern with liveness DECAY applied. m must carry :online/:lapsed-ago-ms
;; (via with-liveness) + :abandoned. Live -> plain; lapsed building -> STALE (dim);
;; lapsed likely-to-land -> ORPHANED (prominent, retained recovery candidate);
;; abandoned-stale -> retired (dim).
(defn decorate [m]
  (let [base (fmt m)]
    (cond
      (:abandoned m)
        (str (dim) base "\n       (ABANDONED-STALE: owner dead >24h — auto-retired by stale-concern janitor)" (rst))
      (:online m) base
      (= (:status m) "likely-to-land")
        (str (bold) "» ORPHANED  " base
             "\n       ⇒ owner lapsed " (ago (:lapsed-ago-ms m))
             " — likely-to-land orphaned; retained recovery candidate, ADOPT this or prepare a handoff" (rst))
      :else
        (str (dim) base
             "\n       (STALE: owner lapsed " (ago (:lapsed-ago-ms m)) ")" (rst)))))

;; ---- overlap discovery + render ---------------------------------------------
;; Discovery returns canonical data; rendering and the attention publisher share
;; it instead of independently reimplementing overlap semantics.
;; Candidate peers come only from the bounded touches predicate/object index;
;; automatic overlap checks never enumerate the concern corpus.
(defn path-overlap-data [port c]
  (let [mine (concern-meta port c)
        peer-ids
        (->> (:touches mine)
             (mapcat #(indexed-subjects-for port "touches" %))
             (remove #(= % c))
             distinct
             sort)
        hits
        (->> peer-ids
             (map #(concern-meta port %))
             (keep
              (fn [peer]
                (let [shared
                      (set/intersection (:touches mine) (:touches peer))]
                  (when (and (active-concern? mine)
                             (active-concern? peer)
                             (seq shared))
                    (canonical-overlap mine peer shared "path")))))
             (sort-by :pair-key)
             vec)]
    {:mine mine :overlaps hits}))

(defn render-overlap-data [spine {:keys [mine overlaps]} statuses none-msg]
  (let [hits (filter #(or (nil? statuses)
                          (statuses (-> (other-concern % (:id mine)) :status)))
                     overlaps)]
    (if (empty? hits)
      (println
       (str "  (none) — " none-msg
            " {" (str/join " " (sort (:touches mine))) "}"))
      (doseq [overlap hits]
        (let [m (with-liveness spine (other-concern overlap (:id mine)))]
          (println (decorate m))
          (when (and (:online m) (= (:status m) "likely-to-land"))
            (if-let [revision (:candidate-rev m)]
              (println (str "       [candidate " revision "] — build against this exact commit"))
              (println "       [candidate missing] — do not integrate blind")))
          (println
           (str "       SHARES: " (str/join " " (:shared overlap)))))))))

;; A configured code-store footprint requires an explicit canonical endpoint.
;; Until that endpoint includes a SpaceId, path overlap is the only live mode.
(defn overlaps-for [spine c]
  (let [mine (concern-meta spine c)
        stored-port (some-> (:code-port mine) ->port)
        cport (or stored-port code-port)
        clog (when cport
               (or (:code-log mine) code-log))]
    (when (and cport (str/blank? clog))
      (code-store-error!
       (str c " has a code_port but no explicit code_log identity")))
    (if cport
      (code-store-unavailable! clog)
      (path-overlap-data spine c))))

(defn validate-attention-index-rows! [rows]
  (when-not
   (and (vector? rows)
        (every?
         #(and (vector? %)
               (= 2 (count %))
               (every? string? %)
               (str/starts-with? (first %) "@concern-"))
         rows))
    (throw
     (ex-info "malformed concern attention index rows"
              {:type :malformed-concern-attention-index})))
  rows)

(defn pending-attention-event-intents [port raw]
  (let [concern (when raw (existing-concern! port raw))
        candidates
        (if concern
          (mapv (fn [encoded] [concern encoded])
                (many port concern attention-event-intent-predicate))
          (validate-attention-index-rows!
           (indexed-predicate-rows port attention-event-intent-predicate)))
        pending (->> candidates distinct sort vec)
        rows (vec (take attention-event-reconcile-limit pending))]
    {:more (> (count pending) attention-event-reconcile-limit)
     :intents
     (mapv
      (fn [[subject encoded]]
        (merge
         {:concern subject :encoded encoded}
         (parse-attention-event-intent port subject encoded)))
      rows)}))

(defn terminal-intent-eligible? [port concern trigger-status]
  (let [state (meta-of port concern)]
    (and
     (= "concern" (:kind state))
     (case trigger-status
       "landed" (= "landed" (:status state))
       "abandoned-stale" (:abandoned state)
       false))))

(defn settle-attention-event-intent! [port concern encoded]
  (require-publication!
   "concern attention event settlement was rejected"
   (north.coord/publish!
    port
    [(publish-action :assert concern attention-event-settled-predicate
                     encoded :many)
     (publish-action :retract concern attention-event-intent-predicate
                     encoded :many)]))
  (when-not (and
             (contains?
              (set (many port concern attention-event-settled-predicate))
              encoded)
             (not
              (contains?
               (set (many port concern attention-event-intent-predicate))
               encoded)))
    (throw
     (ex-info "concern attention event settlement read-back mismatch"
              {:type :concern-attention-settlement-readback-mismatch
               :concern concern})))
  true)

(defn publish-pending-attention-event-intents!
  "Publish each eligible outbox record idempotently, then settle its exact
   canonical value. A crash after publication but before settlement safely
   republishes the same deterministic notification on the next pass."
  [port raw]
  (let [{:keys [intents more]}
        (pending-attention-event-intents port raw)]
    {:more more
     :notifications
     (reduce
      (fn [published
           {:keys [concern encoded trigger-status event]}]
        (if (terminal-intent-eligible? port concern trigger-status)
          (let [notification
                (north.attention/publish-notification! port event)]
            (settle-attention-event-intent! port concern encoded)
            (conj published notification))
          published))
      []
      intents)}))

(defn pending-attention-reconciliations [port raw]
  (let [concern (when raw (existing-concern! port raw))
        candidates
        (if concern
          (mapv (fn [token] [concern token])
                (many port concern attention-reconcile-pending-predicate))
          (validate-attention-index-rows!
           (indexed-predicate-rows
            port attention-reconcile-pending-predicate)))
        pending (->> candidates distinct sort vec)
        limit north.worker-policy/attention-reconcile-limit
        rows (vec (take limit pending))]
    {:more (> (count pending) limit)
     :rows rows}))

(defn settle-attention-reconciliations! [port rows]
  (doseq [[concern token] rows]
    (require-publication!
     "concern attention reconciliation settlement was rejected"
     (north.coord/publish!
      port
      [(publish-action :retract concern attention-reconcile-pending-predicate
                       token :many)]))
    (when
     (contains?
      (set (many port concern attention-reconcile-pending-predicate))
      token)
      (throw
       (ex-info "concern attention reconciliation settlement read-back mismatch"
                {:type :concern-attention-reconciliation-settlement-readback-mismatch
                 :concern concern}))))
  true)

(defn reconciliation-overlaps
  "Discover current pairs for exactly one concern. Automatic callers supply
   durable pending subjects; this function never enumerates the corpus."
  [spine raw]
  (when-not raw
    (throw
     (ex-info "attention overlap reconciliation requires one concern"
              {:type :unscoped-attention-reconciliation})))
  (let [concern (existing-concern! spine raw)]
    (binding [*throw-code-store-errors?* true]
      (:overlaps (overlaps-for spine concern)))))

(defn reconcile-attention!
  "Idempotently materialize pending current-overlap work plus durable terminal
   intents. The automatic form reads only live indexed outbox rows."
  ([spine] (reconcile-attention! spine nil))
  ([spine raw]
   (let [{terminal-notifications :notifications
          terminal-more :more}
         (publish-pending-attention-event-intents! spine raw)
         {pending-rows :rows active-more :more}
         (pending-attention-reconciliations spine raw)
         concerns
         (if raw
           [(existing-concern! spine raw)]
           (->> pending-rows (map first) distinct sort vec))
         attempts
         (mapv
          (fn [concern]
            (try
              {:concern concern
               :overlaps (reconciliation-overlaps spine concern)}
              (catch Exception error
                (binding [*out* *err*]
                  (println
                   (str "concern: attention reconciliation deferred for "
                        concern ": " (.getMessage error))))
                {:concern concern :error error :overlaps []})))
          concerns)
         successful-concerns
         (set (map :concern (remove :error attempts)))
         settled-rows
         (filterv #(contains? successful-concerns (first %)) pending-rows)
         overlaps
         (->> attempts
              (mapcat :overlaps)
              (reduce (fn [pairs overlap]
                        (assoc pairs (:pair-key overlap) overlap))
                      {})
              vals
              (sort-by :pair-key)
              vec)
         events
         (->> overlaps
              (mapcat desired-events-for-overlap)
              (reduce (fn [unique event]
                        (assoc unique [(:event-key event) (:to event)] event))
                      {})
              vals
              (sort-by (juxt :event-key :to))
              vec)
         current-notifications (publish-attention-events! spine events)
         _ (settle-attention-reconciliations! spine settled-rows)
         notifications
         (vec (concat terminal-notifications current-notifications))]
     {:overlaps (count overlaps)
      :events (count events)
      :terminal-events (count terminal-notifications)
      :terminal-more terminal-more
      :active-more active-more
      :deferred (count (filter :error attempts))
      :notifications notifications})))

(defn transition-state [before trigger-status]
  (if (= "abandoned-stale" trigger-status)
    (assoc before :abandoned true)
    (assoc before :status trigger-status)))

(defn terminal-state [before trigger-status]
  (case trigger-status
    "landed" (assoc before :status "landed")
    "abandoned-stale" (assoc before :abandoned true)))

(defn terminal-status-present? [state trigger-status]
  (case trigger-status
    "landed" (= "landed" (:status state))
    "abandoned-stale" (:abandoned state)
    false))

(defn transition-status-present? [port state trigger-status]
  (contains? (set (many port (:id state) "reached")) trigger-status))

(defn stale-building-concern? [port state]
  (let [reached (set (many port (:id state) "reached"))
        {:keys [online lapsed-ago-ms]} (owner-liveness port state)]
    (and
     (contains? reached "building")
     (not (contains? reached "likely-to-land"))
     (not (contains? reached "landed"))
     (not (contains? reached "abandoned-stale"))
     (not online)
     (integer? lapsed-ago-ms)
     (>= lapsed-ago-ms concern-stale-ms))))

(defn concern-transition-plan!
  "Rebuild one concern transition after a global base capture. Terminal
   overlap-left warnings join the reached fact in the same canonical batch;
   active-state warnings are recovered from the current concern projection."
  [port operation snapshot]
  (let [concern (:concern-id operation)
        trigger-status (:object (peek (:facts operation)))
        reconcile-token (:operation-id operation)
        observed-rows (or (:rows snapshot) [])
        snapshot-before
        (when snapshot
          (concern-meta-from-exact-rows concern observed-rows))
        terminal? (contains? terminal-attention-statuses trigger-status)
        plan
        (fn [before]
          (cond
            (not= "concern" (:kind before))
            (if snapshot
              {:local-conflict true
               :reason "concern-missing-or-invalid"
               :observed-version (:base snapshot)
               :rows observed-rows}
              (usage-error! (str concern " is not an existing concern")))

            (if snapshot
              (contains? (:reached before) trigger-status)
              (transition-status-present? port before trigger-status))
            {:done :identical}

            (not (active-concern? before))
            {:local-conflict true
             :reason "transition-overtaken"
             :observed-version (:base snapshot)
             :rows observed-rows}

            (and (= "abandoned-stale" trigger-status)
                 (not (stale-building-concern? port before)))
            {:local-conflict true
             :reason "transition-ineligible"
             :observed-version (:base snapshot)
             :rows observed-rows}

            (not terminal?)
            {:facts [{:p attention-reconcile-pending-predicate
                      :r reconcile-token}
                     {:p "reached" :r trigger-status}]}

            :else
            (let [after (transition-state before trigger-status)
                  attention-kind (transition-kind before after)
                  overlaps
                  (if attention-kind
                    (binding [*throw-code-store-errors?* (boolean snapshot)]
                      (:overlaps (overlaps-for port concern)))
                    [])
                  events (attention-events-for-transition before after overlaps)
                  intents
                  (mapv
                   #(attention-event-intent-value
                     port concern trigger-status %)
                   events)]
              {:facts
               (vec
                (concat
                 (map (fn [intent]
                        {:p attention-event-intent-predicate :r intent})
                      intents)
                 [{:p "reached" :r trigger-status}]))})))]
    (cond
      (and snapshot (nil? snapshot-before))
      {:local-conflict true
       :reason "concern-missing-or-invalid"
       :observed-version (:base snapshot)
       :rows observed-rows}

      terminal?
      (binding [*concern-metas*
                (when snapshot-before {concern snapshot-before})]
        (plan (or snapshot-before (concern-meta port concern))))

      :else
      (plan (or snapshot-before (concern-meta port concern))))))

(defn terminal-concern-transition!
  "Commit terminal overlap-left intents and the concern terminal fact in one
   globally versioned subject-local batch. Publication follows from the durable
   outbox, so every post-commit crash point is replayable."
  [port concern trigger-status]
  (when-not (contains? terminal-attention-statuses trigger-status)
    (throw
     (ex-info "unsupported concern terminal transition"
              {:type :invalid-concern-terminal-transition
               :trigger-status trigger-status})))
  (let [concern (existing-concern! port concern)
        result
        (transact-after-read!
         port concern
         (fn []
           ;; Every read of this phase is guarded by the global base captured
           ;; just above. Candidate peers come from bounded touches indexes.
           (let [before (concern-meta port concern)]
               (cond
                 (terminal-status-present? before trigger-status)
                 {:done {:status :already :concern concern
                         :trigger-status trigger-status}}

                 (not (active-concern? before))
                 {:done {:status :ineligible :concern concern
                         :trigger-status trigger-status}}

                 (and (= "abandoned-stale" trigger-status)
                      (not (stale-building-concern? port before)))
                 {:done {:status :ineligible :concern concern
                         :trigger-status trigger-status}}

                 :else
                 (let [before-overlaps (:overlaps (overlaps-for port concern))
                       after (terminal-state before trigger-status)
                       events
                       (attention-events-for-transition
                        before after before-overlaps)
                       intents
                       (mapv
                        #(attention-event-intent-value
                          port concern trigger-status %)
                        events)]
                   {:facts
                    (vec
                     (concat
                      (map (fn [intent]
                             {:p attention-event-intent-predicate
                              :r intent})
                           intents)
                      [{:p "reached" :r trigger-status}]))})))))
        transition
        (cond
          (:done result) (:done result)
          (:ok result) {:status :committed
                        :concern concern
                        :trigger-status trigger-status}
          :else
          (throw
           (ex-info "concern terminal transition publication failed"
                    {:type :concern-terminal-transition-failed
                     :concern concern
                     :trigger-status trigger-status
                     :result result})))
        reconciliation
        (when (#{:committed :already} (:status transition))
          (reconcile-attention! port concern))]
    (cond-> transition
      reconciliation (assoc :reconciliation reconciliation))))

(defn surface [spine c statuses none-msg]
  (render-overlap-data spine (overlaps-for spine c) statuses none-msg))

;; `overlap` is the footprint view. --landing filters to likely-to-land work.
(defn overlap! [port c landing?]
  (if landing?
    (do (println "LIKELY-TO-LAND work in your footprint — build against these:")
        (surface port c #{"likely-to-land"} "no likely-to-land work is in your footprint yet"))
    (do (println (str "Concerns in the footprint of " c " (any status; likely-to-land marked):"))
        (surface port c nil "nothing else is in your footprint"))))

(def declare-transport-timeout-ms-default 1200)
(def declare-transport-timeout-ms-maximum 1500)

(defn declare-transport-timeout-ms []
  (let [raw (or (System/getenv "NORTH_CONCERN_DECLARE_TRANSPORT_TIMEOUT_MS")
                (str declare-transport-timeout-ms-default))
        value (when (re-matches #"[1-9][0-9]*" raw) (parse-long raw))]
    (when-not (and value (<= 1 value declare-transport-timeout-ms-maximum))
      (configuration-error!
       (str "NORTH_CONCERN_DECLARE_TRANSPORT_TIMEOUT_MS must be an integer from 1 through "
            declare-transport-timeout-ms-maximum)))
    value))

(defn exact-ref? [value]
  (and (string? value)
       (re-matches #"@[A-Za-z0-9][A-Za-z0-9._:-]*" value)))

(def transport-unknown-types
  #{:coordination-operation-timeout
    :rpc/ambiguous-write
    :rpc/retry-exhausted
    :concern-declare-transport-unknown
    :concern-write-transport-unknown
    :store-rpc-routing-unavailable})

(defn transport-unknown? [error]
  (loop [cause error]
    (cond
      (nil? cause) false
      (instance? java.io.IOException cause) true
      (contains? transport-unknown-types (:type (ex-data cause))) true
      :else (recur (.getCause ^Throwable cause)))))

(defn concern-declare-facts
  "The exact ordered desired projection. kind=concern is last: it is the
   visibility marker both the live batch and future reconciliation publish
   only after the complete body is present."
  [{:keys [agent repo intent about files reconcile-token]}]
  (vec
   (concat
    [{:predicate "title"
      :object (str "[" repo "] " intent)
      :cardinality "single"}
     {:predicate "agent" :object agent :cardinality "single"}
     {:predicate "driver" :object agent :cardinality "single"}
     {:predicate "repo" :object repo :cardinality "single"}
     {:predicate "intent" :object intent :cardinality "single"}]
    (when about
      [{:predicate "about" :object about :cardinality "single"}])
    (when code-port
      [{:predicate "code_port" :object (str code-port) :cardinality "single"}])
    (when code-log
      [{:predicate "code_log" :object code-log :cardinality "single"}])
    (map (fn [file]
           {:predicate "touches" :object file :cardinality "multi"})
         files)
    [{:predicate attention-reconcile-pending-predicate
      :object reconcile-token
      :cardinality "multi"}
     {:predicate "reached" :object "building" :cardinality "multi"}
     {:predicate "kind" :object "concern" :cardinality "single"}])))

(defn build-concern-operation
  [{:keys [agent repo intent about about-binding-cid files]}]
  (let [created-at (java.time.Instant/now)
        operation-id (str (java.util.UUID/randomUUID))
        concern-id
        (str "@concern-" (.toEpochMilli created-at) "-"
             (subs operation-id 0 4))]
    (north.concern-spool/build-operation
     {:operation-id operation-id
      :concern-id concern-id
      :target-log (north.coord/expected-log)
      :created-at (str created-at)
      :about about
      :about-binding-cid about-binding-cid
      :facts
      (concern-declare-facts
       {:agent agent
        :repo repo
        :intent intent
        :about about
        :reconcile-token operation-id
        :files files})})))

(defn build-concern-transition-operation [raw trigger-status]
  (let [concern (norm-cid raw)
        operation-id (str (java.util.UUID/randomUUID))]
    (when-not (and (string? concern)
                   (re-matches #"@concern-[0-9]{10,}-[0-9a-f]{4}" concern))
      (usage-error! "a canonical concern id is required"))
    (north.concern-spool/build-operation
     {:operation-type north.concern-spool/transition-operation-type
      :operation-id operation-id
      :concern-id concern
      :target-log (north.coord/expected-log)
      :created-at (str (java.time.Instant/now))
      :facts
      [{:predicate attention-reconcile-pending-predicate
        :object operation-id
        :cardinality "multi"}
       {:predicate "reached"
        :object trigger-status
        :cardinality "multi"}]})))

(defn checked-declare-batch! [port operation]
  (let [facts
        (mapv
         (fn [{:keys [predicate object]}]
           {:p predicate :r object})
         (:facts operation))
        response
        (transact-after-read!
         port
         (:concern-id operation)
         (fn []
           (when-let [about (get-in operation [:precondition :about :subject])]
             (existing-thread! port about))
           {:facts facts}))]
    (cond
      (:reject response)
      (throw
       (ex-info "Store RPC explicitly rejected concern declaration"
                {:type :concern-declare-semantic-rejection
                 :response response}))

      (transaction-ack? response (count facts))
      response

      :else
      (throw
       (ex-info "Store RPC acknowledgement for concern declaration is ambiguous"
                {:type :concern-declare-transport-unknown})))))

(defn ensure-agent-label! [port agent agent-ref]
  (when (and (nil? (resolved port agent-ref "identity_manifest_sha256"))
             (nil? (resolved port agent-ref "display_name")))
    (require-publication!
     "Store RPC explicitly rejected concern principal label"
     (north.coord/publish!
      port
      [{:op :set :subject agent-ref :predicate "display_name"
        :values [agent] :cardinality :one}]))))

(defn durable-local-declare! [operation about-raw]
  (when (and about-raw (not (exact-ref? about-raw)))
    (configuration-error!
     (str "Store RPC transport is unavailable; --about must be an exact @ref "
          "before a durable-local operation can be published (nothing was spooled)")))
  (when (and about-raw
             (nil? (get-in operation
                           [:precondition :about :binding-cid])))
    (binding [*out* *err*]
      (println
       (str "concern: Store RPC transport is unavailable before --about "
            "could be bound to a stable thread identity; nothing was spooled")))
    (System/exit 4))
  (try
    (let [receipt
          (north.concern-spool/publish-operation!
           operation
           (north.coord/request-deadline-ns 400))]
      (println
       (str "✓ concern " (:concern-id operation)
            " durable-local visibility=pending"))
      (println
       (str "  operation=" (:operation-id receipt)
            " target_log=" (:target-log receipt)))
      (println
       (str "  local_path=" (:path receipt)))
      receipt)
    (catch Exception error
      (binding [*out* *err*]
        (println
         (str "concern: durable-local publication failed: "
              (.getMessage error))))
      (System/exit 4))))

(defn durable-local-transition! [operation]
  (try
    (let [receipt
          (north.concern-spool/publish-operation!
           operation
           (north.coord/request-deadline-ns 400))]
      (println
       (str "✓ concern " (:concern-id operation)
            " transition=" (:object (peek (:facts operation)))
            " durable-local visibility=pending"))
      (println
       (str "  operation=" (:operation-id receipt)
            " target_log=" (:target-log receipt)))
      (println (str "  local_path=" (:path receipt)))
      {:status :pending
       :concern (:concern-id operation)
       :trigger-status (:object (peek (:facts operation)))
       :receipt receipt})
    (catch Exception error
      (binding [*out* *err*]
        (println
         (str "concern: durable-local transition publication failed: "
              (.getMessage error))))
      (System/exit 4))))

(defn execute-concern-transition!
  [raw trigger-status live-transition!]
  (try
    (live-transition!)
    (catch Exception error
      (if (transport-unknown? error)
        (durable-local-transition!
         (build-concern-transition-operation raw trigger-status))
        (throw error)))))

(defn ensure-candidate! [port raw revision git-dir]
  (let [concern (existing-concern! port raw)
        response
        (north.coord/publish!
         port
         [{:op :set :subject concern :predicate "candidate_git_dir"
           :values [git-dir] :cardinality :one}
          {:op :set :subject concern :predicate "candidate_rev"
           :values [revision] :cardinality :one}])]
    (when (:reject response)
      (throw
       (ex-info "Store RPC rejected the concern candidate identity"
                {:type :concern-candidate-rejected
                 :concern concern
                 :revision revision
                 :git-dir git-dir
                 :response response})))
    (require-publication!
     "Store RPC rejected the concern candidate identity"
     response)
    (when-not (and (= revision (resolved port concern "candidate_rev"))
                   (= git-dir (resolved port concern "candidate_git_dir")))
      (throw
       (ex-info "concern candidate identity read-back mismatch"
                {:type :concern-candidate-readback-mismatch
                 :concern concern
                 :revision revision
                 :git-dir git-dir})))
    concern))

(defn advance-active-maturity! [port raw maturity-level]
  (let [concern (existing-concern! port raw)
        reconcile-token (str (java.util.UUID/randomUUID))]
    (transact-after-read!
     port concern
     (fn []
       (let [before (concern-meta port concern)]
         (if (transition-status-present? port before maturity-level)
           {:done :identical}
           {:facts
            [{:p attention-reconcile-pending-predicate :r reconcile-token}
             {:p "reached" :r maturity-level}]}))))
    (reconcile-attention! port concern)
    (let [after (meta-of port concern)]
      (println
       (str "✓ " concern " reached=" maturity-level
            " (status=" (:status after) ")"
            (when-let [revision (:candidate-rev after)]
              (str " candidate=" revision)))))))

(let [[ps verb & args] *command-line-args*
      port (Integer/parseInt ps)]
  (case verb
    "declare"
    (let [{:keys [agent repo intent files about about-raw]}
          (parse-declare-args! args)
          fs (->> (str/split (or files "") #",")
                  (map str/trim)
                  (remove str/blank?)
                  distinct
                  sort
                  vec)
          agent-e (norm-cid agent)
          operation
          (build-concern-operation
           {:agent agent-e
            :repo repo
            :intent intent
            :about about
            :files fs})
          id (:concern-id operation)
          batch-acked? (atom false)
          after-data* (atom nil)]
      (let [result
            (try
              (binding [north.coord/*request-deadline-ns*
                        (north.coord/request-deadline-ns
                         (declare-transport-timeout-ms))]
                ;; A configured code corpus must have a canonical SpaceId before
                ;; the first coordination mutation; otherwise spool locally.
                (when code-port (validate-code-store! code-port code-log))
                ;; An about ref is validated inside checked-declare-batch!'s
                ;; expected-version transaction. Unmanaged principals still
                ;; receive a label before that declaration transaction.
                (ensure-agent-label! port agent agent-e)
                ;; One all-or-none spine publication carries the exact same
                ;; live facts as before, with kind=concern terminal.
                (checked-declare-batch! port operation)
                (reset! batch-acked? true)
                (let [after-data (overlaps-for port id)
                      _after-data (reset! after-data* after-data)]
                  ;; The declaration batch's pending marker makes this desired-
                  ;; state publication replayable after every crash point.
                  (reconcile-attention! port id)
                  {:durability "coordinator"
                   :after-data after-data}))
              (catch Exception error
                (if (transport-unknown? error)
                  (if @batch-acked?
                    {:durability "coordinator"
                     :after-data @after-data*
                     :attention-deferred (.getMessage error)}
                    (durable-local-declare! operation about-raw))
                  (throw error))))]
        (when (= "coordinator" (:durability result))
          (println (str "✓ concern " id))
          (println
           (str "  " agent-e "  building  [" repo "]  touches {"
                (str/join " " fs) "}"))
          (if-let [after-data (:after-data result)]
            (do
              (println "\nOverlapping concerns — coordinate, you are NOT blocked:")
              (render-overlap-data
               port
               after-data
               nil
               "no other concern is in your footprint"))
            (binding [*out* *err*]
              (println
               (str "concern: declaration is coordinator-durable; overlap attention "
                    "is deferred after a transport failure"
                    (when-let [message (:attention-deferred result)]
                      (str ": " message))))))
          (println
           (str "\n  next: `concern overlap " id
                "` — who's in your footprint, likely-to-land"
                " marked (build against those);  `concern status " id
                " likely-to-land` as you near merge.")))))

    "reconcile-local"
    (do
      (when-not (or (empty? args) (= ["--operations-only"] (vec args)))
        (usage-error! "reconcile-local accepts only optional --operations-only"))
      (let [operations-only? (= ["--operations-only"] (vec args))
            result
            (binding
             [north.concern-spool-reconcile/*transition-plan!*
              concern-transition-plan!]
              (north.concern-spool-reconcile/reconcile-pass! port))
            {:keys [status selected processed bytes settled conflicts
                    already-settled already-conflict deferred retired remaining
                    elapsed-ms]}
            result
            attention-attempt
            (when (and (not operations-only?)
                       (not= :busy status)
                       (pos? (+ settled already-settled)))
              (try
                {:result (reconcile-attention! port)}
                (catch Exception error
                  {:deferred (.getMessage error)})))
            attention (:result attention-attempt)]
        (if (= :busy status)
          (do
            (binding [*out* *err*]
              (println "concern: local reconciler is already running"))
            (System/exit 3))
          (do
            (println
             (str "✓ concern local reconciliation"
                  " selected=" selected
                  " processed=" processed
                  " bytes=" bytes
                  " settled=" settled
                  " conflicts=" conflicts
                  " already_settled=" already-settled
                  " already_conflict=" already-conflict
                  " deferred=" deferred
                  " retired=" retired
                  " remaining=" remaining
                  " elapsed_ms=" elapsed-ms
                  (when attention
                    (str " attention=" (count (:notifications attention))))))
            (when-let [message (:deferred attention-attempt)]
              (binding [*out* *err*]
                (println
                 (str "concern: local operations reconciled; attention repair "
                      "is deferred after a transport failure: " message))))))))

    "overlap"
    (let [[c & flags] args]
      (overlap! port (norm-cid c) (boolean (some #(= % "--landing") flags))))

    ;; Liveness-derived DECAY (design 019f4418): a lapsed owner's concern is NOT hidden
    ;; — hiding is what made 17 dead-agent concerns invisibly linger AND let a stale one
    ;; misroute a live lane. It is RENDERED, decayed at read time: building -> STALE (dim),
    ;; likely-to-land -> ORPHANED (prominent retained recovery candidate). The janitor's
    ;; terminal verdict `abandoned-stale` (owner dead >24h) retires the concern — hidden by
    ;; default, shown with --all. Agent-less concerns can't lapse, so render live.
    "ls"
    (let [flags   (set (filter #(str/starts-with? % "--") args))
          show-all (boolean (or (flags "--all") (flags "--stale")))
          repo    (first (remove #(str/starts-with? % "--") args))
          facts   (concern-list-facts port)
          now     (System/currentTimeMillis)
          all-ms  (->> (concerns-from-live facts)
                       (map #(meta-from-live facts % now))
                       (remove #(= (:status %) "landed"))
                       (filter #(or (nil? repo) (= (:repo %) repo)))
                       (sort-by (juxt :repo #(str (:agent %)))))
          active  (remove :abandoned all-ms)             ; abandoned-stale retired: hidden unless --all
          shown   (if show-all all-ms active)
          stale-ct    (count (filter #(and (not (:online %)) (not (:abandoned %))
                                           (not= (:status %) "likely-to-land")) active))
          orphaned-ct (count (filter #(and (not (:online %)) (= (:status %) "likely-to-land")) active))
          retired-ct (- (count all-ms) (count active))]
      (println (str "ACTIVE CONCERNS" (when repo (str " in " repo)) " — " (count shown)
                    (when (pos? stale-ct)    (str "  [" stale-ct " STALE: owner lapsed]"))
                    (when (pos? orphaned-ct) (str "  [" orphaned-ct " ORPHANED: owner gone, likely-to-land]"))
                    (when (and (pos? retired-ct) (not show-all))
                      (str "  [" retired-ct " abandoned-stale retired — `concern ls --all` to show]"))))
      (doseq [m shown]
        (println (decorate m))))

    ;; Strict versioned MACHINE projection — the same liveness decay as `ls`, emitted
    ;; as JSON so consumers (the dashboard) never scrape rendered text. Optional repo
    ;; positional filters as in `ls`. Class ∈ {live,stale,orphaned,retired}.
    "list-json"
    (let [repo (first (remove #(str/starts-with? % "--") args))]
      (println (json/generate-string (concern-projection port repo))))

    "reconcile-attention"                                ; hidden crash-healing seam
    (let [[raw] args]
      (when (> (count args) 1)
        (usage-error! "reconcile-attention accepts at most one concern id"))
      (let [{:keys [overlaps events terminal-events terminal-more active-more
                    deferred notifications]}
            (reconcile-attention! port raw)]
        (println
         (str "✓ concern attention reconciled overlaps=" overlaps
              " desired=" events
              " terminal=" terminal-events
              " terminal_more=" terminal-more
              " active_more=" active-more
              " deferred=" deferred
              " more=" (boolean (or terminal-more active-more))
              " materialized=" (count notifications)))))

    "retire-stale"                                      ; hidden janitor boundary
    (let [[raw] args]
      (when-not (= 1 (count args))
        (usage-error! "retire-stale requires exactly <concern-id>"))
      (execute-concern-transition!
       raw
       "abandoned-stale"
       #(prn
         (select-keys
          (terminal-concern-transition!
           port raw "abandoned-stale")
           [:status :concern :trigger-status]))))

    "candidate"
    (let [[raw requested & extra] args]
      (when (or (nil? raw) (seq extra))
        (usage-error! "candidate requires <concern-id> and optional <git-rev>"))
      (let [{:keys [revision git-dir]} (resolve-candidate! requested)]
        (ensure-candidate! port raw revision git-dir)
        (execute-concern-transition!
         raw
         "likely-to-land"
         #(advance-active-maturity! port raw "likely-to-land"))))

    "status"
    (let [[raw st] args]
      (when-not (= 2 (count args))
        (usage-error! "status requires exactly <concern-id> <maturity>"))
      (when-not (contains? maturity-idx st)
        (usage-error! (str "invalid maturity " (pr-str st) "; expected one of "
                           (str/join ", " maturity))))
      (when (= "likely-to-land" st)
        (let [{:keys [revision git-dir]} (resolve-candidate! nil)]
          (ensure-candidate! port raw revision git-dir)))
      (execute-concern-transition!
       raw
       st
       #(let [c (existing-concern! port raw)]
          (if (= "landed" st)
            (let [transition
                  (terminal-concern-transition! port c "landed")]
              (when (= :ineligible (:status transition))
                (throw
                 (ex-info "inactive concern cannot newly reach landed"
                          {:type :ineligible-concern-terminal-transition
                           :concern c})))
              (println
               (str "✓ " c " reached=landed (status=landed)")))
            (advance-active-maturity! port c st)))))

    "done"
    (let [[raw] args]
      (when-not (= 1 (count args))
        (usage-error! "done requires exactly <concern-id>"))
      (execute-concern-transition!
       raw
       "landed"
       #(let [c (existing-concern! port raw)
              transition
              (terminal-concern-transition! port c "landed")]
          (when (= :ineligible (:status transition))
            (throw
             (ex-info "inactive concern cannot newly land"
                      {:type :ineligible-concern-terminal-transition
                       :concern c})))
          (println (str "✓ " c " landed")))))

    (do (println usage)
        (System/exit 2))))
