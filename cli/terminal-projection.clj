(ns north.terminal-projection
  "Pure read contract for crash-safe lane terminals and run publications.

  A managed lane terminal is committed by terminal_manifest_sha256 after its
  base process/delivery facts and any status-required proof projection. A run
  is committed by kind=run, which its scoped writer publishes last. Legacy lane
  rows remain readable only when they have no process_outcome fact at all.")

(require '[cheshire.core :as json]
         '[clojure.string :as str])

(def terminal-predicates
  ["outcome" "process_outcome" "delivery_outcome" "delivery_reason"])
(def delivery-proof-predicates
  ["delivery_evidence" "delivery_evidence_sha256"
   "delivery_attestation" "delivery_attestation_sha256"])
(def terminal-projection-predicates
  (into terminal-predicates delivery-proof-predicates))
(def run-resolution-predicates
  (into ["agent" "at" "kind"] terminal-projection-predicates))
(def delivery-evidence-version "north:done-bars:v2")
(def run-bar-evidence-version "north:run-bar-evidence:v1")
(def max-delivery-bars 32)
(def max-delivery-bar-utf8-bytes 512)
(def max-delivery-observed-utf8-bytes 2048)
(def max-delivery-envelope-utf8-bytes (* 256 1024))
(def max-run-bar-evidence-record-utf8-bytes (* 16 1024))
(def max-run-reservation-baseline-utf8-bytes (* 64 1024))
(def max-delivery-writer-request-utf8-bytes (* 16 1024))
(def max-delivery-thread-id-utf8-bytes 512)
(def max-delivery-run-id-utf8-bytes 512)
(def max-delivery-agent-id-utf8-bytes 256)
(def max-delivery-attestation-utf8-bytes (* 16 1024))
(def run-reservation-version "north:run-reservation:v1")
(def run-reservation-body-predicates
  ["run_capability_sha256" "run_reservation_agent"
   "run_reservation_contract_origin" "run_reservation_done_when"
   "run_reservation_thread" "run_reservation_version" "run_reserved_at"])
(def run-reservation-predicates
  (conj run-reservation-body-predicates "run_reservation_manifest_sha256"))
(def delivery-attestation-version "north:delivery-attestation:v1")
(def delivery-attestation-authority "managed-independent-verifier")

(defn- values-of [facts predicate]
  (let [value (get facts predicate ::absent)]
    (cond
      (= ::absent value) []
      (set? value) (vec value)
      (and (sequential? value) (not (string? value))) (vec value)
      :else [value])))

(defn fact-present?
  [facts predicate]
  (boolean (seq (values-of facts predicate))))

(defn singleton-value
  "One exact nonblank string value, or nil for absent/conflicting/malformed
  facts. Maps folded to scalar values and maps folded to sets are both accepted."
  [facts predicate]
  (let [values (values-of facts predicate)]
    (when (= 1 (count values))
      (let [value (first values)]
        (when (and (string? value) (not (str/blank? value))) value)))))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        (.getBytes (str value)
                                   java.nio.charset.StandardCharsets/UTF_8))]
    (format "%064x" (java.math.BigInteger. 1 digest))))

(defn utf8-byte-count [value]
  (when (string? value)
    (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8))))

(defn valid-unicode-scalars?
  "True only when VALUE contains no unpaired UTF-16 surrogate. Java's default
  UTF-8 encoder silently replaces malformed surrogate code units, so proof
  byte limits must reject them before encoding."
  [value]
  (boolean
   (when (string? value)
     (loop [index 0]
       (if (= index (.length value))
         true
         (let [current (.charAt value index)]
           (cond
             (Character/isHighSurrogate current)
             (and (< (inc index) (.length value))
                  (Character/isLowSurrogate (.charAt value (inc index)))
                  (recur (+ index 2)))

             (Character/isLowSurrogate current) false
             :else (recur (inc index)))))))))

(defn- evidence-control-code-unit?
  [code-unit]
  (or (<= 0 code-unit 0x1f)
      (<= 0x7f code-unit 0x9f)))

(defn canonical-evidence-text
  "Canonical proof text shared with the SDK: valid Unicode scalar sequence,
  no C0/C1 controls, and ASCII SPACE trimmed at the two edges. Unicode spaces
  such as NBSP and EM SPACE remain content rather than acquiring runtime-
  specific trim semantics."
  [value]
  (when (and (string? value)
             (valid-unicode-scalars? value)
             (not-any? #(evidence-control-code-unit? (int %)) value))
    (let [canonical (str/replace value #"^ +| +$" "")]
      (when (seq canonical) canonical))))

(defn bounded-nonblank-text? [value max-bytes]
  (and (string? value)
       (= value (canonical-evidence-text value))
       (<= (utf8-byte-count value) max-bytes)))

(def entity-whitespace-code-units
  ;; Unicode White_Space plus BOM. Hard-coded for identical Java/ECMAScript
  ;; semantics; their native whitespace/trim tables do not agree.
  (into #{0x20 0xa0 0x1680 0x2028 0x2029 0x202f 0x205f 0x3000 0xfeff}
        (range 0x2000 0x200b)))

(defn- forbidden-entity-code-unit?
  [code-unit]
  (or (= code-unit (int \@))
      (evidence-control-code-unit? code-unit)
      (contains? entity-whitespace-code-units code-unit)))

(defn valid-thread-entity?
  [value]
  (boolean
   (and (string? value)
        (valid-unicode-scalars? value)
        (<= (utf8-byte-count value) max-delivery-thread-id-utf8-bytes)
        (str/starts-with? value "@")
        (> (.length value) 1)
        (not-any? #(forbidden-entity-code-unit? (int %))
                  (subs value 1)))))

(defn valid-run-entity?
  [value]
  (boolean
   (and (string? value)
        (<= (utf8-byte-count value) max-delivery-run-id-utf8-bytes)
        ;; accept both the legacy `@run-` (pre-2026-07-20 logs) and the
        ;; telemetry-routable `@run:` id; see newRunId in sdk telemetry.ts.
        (re-matches #"^@run[-:][A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn valid-agent-entity?
  [value]
  (boolean
   (and (string? value)
        (<= (utf8-byte-count value) max-delivery-agent-id-utf8-bytes)
        (re-matches #"^@agent:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn bounded-done-bars? [bars allow-empty?]
  (and (vector? bars)
       (or allow-empty? (seq bars))
       (<= (count bars) max-delivery-bars)
       (every? #(bounded-nonblank-text?
                 % max-delivery-bar-utf8-bytes)
               bars)
       (= bars (vec (sort (distinct bars))))))

(declare instant?)

(defn canonical-done-when
  "Canonical semantic done-bar set used at reservation, assessment, and commit:
  ASCII-space trimmed, nonblank, Unicode-safe, unique, and lexical. Nil means
  at least one stored done_when value is malformed; invalid facts are never
  silently dropped from an authority boundary."
  [facts]
  (let [raw (values-of facts "done_when")
        canonical (mapv canonical-evidence-text raw)]
    (when (and (<= (count raw) max-delivery-bars)
               (every? some? canonical))
      (->> canonical distinct sort vec))))

(defn run-reservation-done-when
  "Parse the manifest-bound canonical reservation baseline, including an empty
  vector for an explicitly worker-defined contract. Nil means malformed."
  [facts]
  (when-let [raw (singleton-value facts "run_reservation_done_when")]
    (when (<= (utf8-byte-count raw)
              max-run-reservation-baseline-utf8-bytes)
      (try
        (let [decoded (json/parse-string raw)
              parsed (when (and (sequential? decoded) (not (string? decoded)))
                       (vec decoded))]
          (when (and parsed (bounded-done-bars? parsed true))
            parsed))
        (catch Exception _ nil)))))

(defn run-reservation-manifest-sha256
  [projection]
  (let [body (into (sorted-map) (select-keys projection run-reservation-body-predicates))]
    (when (= (count body) (count run-reservation-body-predicates))
      (sha256
       (apply str
              (map (fn [[predicate value]]
                     (str predicate "\u0000" value "\n"))
                   body))))))

(defn run-reservation-valid?
  "A reservation is committed only when every authority field is singleton and
  its marker digests the exact projection. Additional run_bar_evidence values
  are allowed; conflicting reservation publishers invalidate the subject."
  [facts]
  (let [body (into (sorted-map)
                   (keep (fn [predicate]
                           (when-let [value (singleton-value facts predicate)]
                             [predicate value])))
                   run-reservation-body-predicates)
        marker (singleton-value facts "run_reservation_manifest_sha256")
        expected (run-reservation-manifest-sha256 body)
        contract-origin (get body "run_reservation_contract_origin")
        baseline (run-reservation-done-when facts)]
    (boolean
     (and (= (count body) (count run-reservation-body-predicates))
          (every? #(= 1 (count (values-of facts %))) run-reservation-predicates)
          (= run-reservation-version (get body "run_reservation_version"))
          (valid-agent-entity? (get body "run_reservation_agent"))
          (valid-thread-entity? (get body "run_reservation_thread"))
          (re-matches #"^[0-9a-f]{64}$" (get body "run_capability_sha256"))
          (instant? (get body "run_reserved_at"))
          (#{"accepted" "worker-defined"} contract-origin)
          (some? baseline)
          (if (= "accepted" contract-origin) (seq baseline) (empty? baseline))
          expected (= marker expected)))))

(defn terminal-manifest-sha256
  "Digest the exact base terminal plus any delivery-proof projection using the
  writer's canonical encoding. Old four-field unverified/blocked terminals keep
  their original digest; proof fields are committed when present."
  [facts]
  (let [required (into {}
                       (keep (fn [predicate]
                               (when-let [value (singleton-value facts predicate)]
                                 [predicate value])))
                       terminal-predicates)
        conflicts? (some #(> (count (values-of facts %)) 1)
                         terminal-projection-predicates)
        projection (into (sorted-map)
                         (keep (fn [predicate]
                                 (when-let [value (singleton-value facts predicate)]
                                   [predicate value])))
                         terminal-projection-predicates)]
    (when (and (= (count terminal-predicates) (count required))
               (not conflicts?))
      (sha256
       (apply str
              (map (fn [[predicate value]]
                     (str predicate "\u0000" value "\n"))
                   projection))))))

(defn- parse-json-map [raw]
  (try
    (let [parsed (json/parse-string raw)]
      (when (map? parsed) parsed))
    (catch Exception _ nil)))

(defn instant? [value]
  (boolean
   (when (string? value)
     (when-let [[_ year month day hour minute second]
                (re-matches
                 #"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$"
                 value)]
       (try
         ;; Instant.parse accepts 24:00 and normalizes it to the next day.
         ;; Proof syntax must reject normalization just like the TS validator.
         (let [hour (parse-long hour)
               minute (parse-long minute)
               second (parse-long second)]
           (and (< hour 24) (< minute 60) (< second 60)
                (do (java.time.LocalDate/of (parse-long year)
                                            (parse-long month)
                                            (parse-long day))
                    (java.time.Instant/parse value)
                    true)))
         (catch Exception _ false))))))

(defn run-bar-evidence-valid?
  [record]
  (boolean
   (and (map? record)
        (= #{"bar" "observed" "recordedAt" "reporter" "run" "thread" "version"}
           (set (keys record)))
        (= run-bar-evidence-version (get record "version"))
        (valid-run-entity? (get record "run"))
        (valid-thread-entity? (get record "thread"))
        (valid-agent-entity? (get record "reporter"))
        (bounded-nonblank-text?
         (get record "bar") max-delivery-bar-utf8-bytes)
        (bounded-nonblank-text?
         (get record "observed") max-delivery-observed-utf8-bytes)
        (instant? (get record "recordedAt")))))

(defn parse-run-bar-evidence
  "Parse one stored record only after its raw UTF-8 envelope is bounded."
  [raw]
  (when (and (string? raw)
             (<= (utf8-byte-count raw)
                 max-run-bar-evidence-record-utf8-bytes))
    (try
      (let [record (json/parse-string raw)]
        (when (run-bar-evidence-valid? record) record))
      (catch Exception _ nil))))

(defn run-evidence-state
  "Validate the entire evidence set on one reserved run. A malformed,
  cross-scoped, duplicate-bar, or over-cap record invalidates the set; callers
  must not cherry-pick the valid subset."
  [facts run thread reporter]
  (let [raws (values-of facts "run_bar_evidence")
        parsed (mapv parse-run-bar-evidence raws)
        scoped?
        (every? (fn [record]
                  (and record
                       (= run (get record "run"))
                       (= thread (get record "thread"))
                       (= reporter (get record "reporter"))))
                parsed)
        bars (when scoped? (mapv #(get % "bar") parsed))
        valid? (and (<= (count raws) max-delivery-bars)
                    scoped?
                    (= (count bars) (count (distinct bars))))]
    {:valid? (boolean valid?)
     :entries (if valid? (mapv vector raws parsed) [])
     :raws (if valid? (set raws) #{})
     :records (if valid? parsed [])}))

(defn evidence-reports-bar?
  "Human thread-review projection only. Delivery qualification uses structured
  run_bar_evidence; this parser keeps needs-review/routing context compatible."
  [bar evidence]
  (let [bar (some-> bar str str/trim)
        evidence (some-> evidence str str/trim)]
    (boolean
     (and (seq bar) (seq evidence)
          (re-matches
           (re-pattern
            (str "^" (java.util.regex.Pattern/quote bar) "\\s*→\\s*\\S.*$"))
           evidence)))))

(defn- valid-evidence-envelope?
  [raw digest]
  (boolean
   (and (string? raw)
        (valid-unicode-scalars? raw)
        (<= (utf8-byte-count raw) max-delivery-envelope-utf8-bytes)
        (let [parsed (parse-json-map raw)
              baseline-bars (get parsed "baselineDoneWhen")
              bars (get parsed "doneWhen")
              matches (get parsed "matches")
              thread (get parsed "thread")
              reporter (get parsed "reporter")
              run (get parsed "run")
              contract-origin (get parsed "contractOrigin")]
          (and
          parsed
          (= (set (keys parsed))
             #{"version" "run" "thread" "reporter" "contractOrigin"
               "baselineDoneWhen" "doneWhen" "matches"})
          (= digest (sha256 raw))
          (= delivery-evidence-version (get parsed "version"))
          (valid-run-entity? run)
          (valid-thread-entity? thread)
          (valid-agent-entity? reporter)
          (#{"accepted" "worker-defined"} contract-origin)
          (bounded-done-bars? baseline-bars true)
          (bounded-done-bars? bars false)
          (if (= "accepted" contract-origin)
            (and (seq baseline-bars) (= baseline-bars bars))
            (empty? baseline-bars))
          (vector? matches) (= (count bars) (count matches))
          (every?
           true?
           (map (fn [bar match]
                  (let [evidence (get match "evidence")]
                    (and (map? match)
                         (= #{"bar" "evidence"} (set (keys match)))
                         (= bar (get match "bar"))
                         (vector? evidence) (= 1 (count evidence))
                         (every?
                          #(and (run-bar-evidence-valid? %)
                                (= bar (get % "bar"))
                                (= run (get % "run"))
                                (= thread (get % "thread"))
                                (= reporter (get % "reporter")))
                          evidence))))
                bars matches)))))))

(defn- valid-attestation-envelope?
  [raw digest evidence-raw evidence-digest]
  (boolean
   (and (string? raw)
        (valid-unicode-scalars? raw)
        (<= (utf8-byte-count raw) max-delivery-attestation-utf8-bytes)
        (let [attestation (parse-json-map raw)
              evidence (parse-json-map evidence-raw)]
          (and attestation evidence
          (= #{"version" "target" "run" "thread" "evidenceSha256"
               "actor" "role" "authority" "attestedAt"}
             (set (keys attestation)))
          (= digest (sha256 raw))
          (= delivery-attestation-version (get attestation "version"))
          (= delivery-attestation-authority (get attestation "authority"))
          (= (get evidence "run") (get attestation "run"))
          (= (get evidence "thread") (get attestation "thread"))
          (= evidence-digest (get attestation "evidenceSha256"))
          (= (get evidence "reporter") (get attestation "target"))
          (valid-agent-entity? (get attestation "target"))
          (valid-agent-entity? (get attestation "actor"))
          (valid-run-entity? (get attestation "run"))
          (valid-thread-entity? (get attestation "thread"))
          (not= (get attestation "actor") (get evidence "reporter"))
          (not= (get attestation "actor") (get attestation "target"))
          (#{"verifier" "judge"} (get attestation "role"))
          (instant? (get attestation "attestedAt")))))))

(defn delivery-proof
  [facts]
  (let [evidence (singleton-value facts "delivery_evidence")
        evidence-digest (singleton-value facts "delivery_evidence_sha256")
        attestation (singleton-value facts "delivery_attestation")
        attestation-digest (singleton-value facts "delivery_attestation_sha256")]
    (when (or evidence evidence-digest attestation attestation-digest)
      {:evidence evidence
       :evidence-digest evidence-digest
       :attestation attestation
       :attestation-digest attestation-digest})))

(defn delivery-projection-valid?
  "Reported requires a complete run-scoped self-reported done-bar snapshot.
  Verified is a reserved legacy value and is rejected until North has an
  isolated verifier boundary. Unverified/blocked forbid proof residue."
  [facts]
  (let [outcome (singleton-value facts "delivery_outcome")
        reason (singleton-value facts "delivery_reason")
        process (singleton-value facts "process_outcome")
        proof (delivery-proof facts)
        proof-fact-count (count (filter #(fact-present? facts %)
                                       delivery-proof-predicates))
        {:keys [evidence evidence-digest attestation attestation-digest]} proof
        evidence-valid? (and evidence evidence-digest
                             (valid-evidence-envelope? evidence evidence-digest))]
    (boolean
     (and process
          (if (= "ran" process) (not= "blocked" outcome) (= "blocked" outcome))
          (case outcome
       "blocked" (and (zero? proof-fact-count) (some? reason))
       "unverified" (and (zero? proof-fact-count) (some? reason))
       "reported" (and (= "complete_run_scoped_done_bar_evidence_self_reported" reason)
                       (= 2 proof-fact-count)
                       evidence-valid?
                       (nil? attestation) (nil? attestation-digest))
       ;; Shared-UID AGENT_ID is provenance, not an unforgeable independent
       ;; verifier capability. Historical envelopes remain parseable for display,
       ;; but no modern writer/reader grants the verified state.
       "verified" false
       false)))))

(defn terminal-manifest-valid?
  [facts]
  (let [marker (singleton-value facts "terminal_manifest_sha256")
        process (singleton-value facts "process_outcome")
        legacy-alias (singleton-value facts "outcome")
        expected (terminal-manifest-sha256 facts)]
    (boolean (and marker process legacy-alias expected
                  (= process legacy-alias)
                  (= marker expected)
                  (delivery-projection-valid? facts)))))

(defn terminal-delivery-outcome
  "Delivery state only from a complete, digest-committed, proof-valid terminal."
  [facts]
  (when (terminal-manifest-valid? facts)
    (some-> (singleton-value facts "delivery_outcome") str/trim)))

(defn terminal-process-outcome
  "Resolve a lane terminal. The presence of process_outcome selects the modern
  protocol and therefore requires a valid terminal manifest; it never falls
  back to the concurrently published legacy alias. A true legacy row is
  accepted only when process_outcome is absent."
  [facts]
  (if (fact-present? facts "process_outcome")
    (when (terminal-manifest-valid? facts)
      (some-> (singleton-value facts "process_outcome") str/trim))
    (some-> (singleton-value facts "outcome") str/trim)))

(defn committed-run?
  "kind=run is the run writer's last-write commit marker."
  [facts]
  (= "run" (singleton-value facts "kind")))

(defn committed-run-process-outcome
  "Resolve a run terminal only after kind=run committed the row. Modern run
  rows require a complete proof-valid process/delivery projection; committed
  historical rows may carry only outcome."
  [facts]
  (when (committed-run? facts)
    (if (fact-present? facts "process_outcome")
      (let [process (singleton-value facts "process_outcome")
            legacy-alias (singleton-value facts "outcome")]
        (when (and process legacy-alias
                   (= process legacy-alias)
                   (terminal-manifest-sha256 facts)
                   (delivery-projection-valid? facts))
          (str/trim process)))
      ;; A historical run is legacy only when no modern terminal residue is
      ;; present. Partial modern publication must never downgrade to the old
      ;; outcome-only protocol.
      (when-not (some #(fact-present? facts %)
                      (concat (remove #{"outcome"} terminal-predicates)
                              delivery-proof-predicates))
        (some-> (singleton-value facts "outcome") str/trim)))))

(defn- terminal-body-present?
  [facts]
  (boolean
   (some #(fact-present? facts %)
         (conj terminal-projection-predicates "terminal_manifest_sha256"))))

(defn- strict-run-instant
  [facts]
  (let [at (singleton-value facts "at")]
    (when (instant? at)
      (java.time.Instant/parse at))))

(defn lane-resolution
  "Canonical execution resolution for one managed lane.

  RUN-ENTRIES are maps with :subject and :facts. The valid digest-committed lane
  terminal is primary. With no lane terminal body, the latest exact run by its
  singleton ISO timestamp may resolve the lane only when its last-write kind
  marker, agent identity, and terminal projection are all valid. Ambiguous,
  torn, conflicting, uncommitted, or nonterminal evidence is indeterminate:
  callers must neither call it active nor finished."
  [control lane-facts run-entries]
  (let [lane-outcome (terminal-process-outcome lane-facts)]
    (cond
      lane-outcome
      {:status :resolved
       :source :agent
       :outcome lane-outcome
       :delivery-outcome (terminal-delivery-outcome lane-facts)
       :facts lane-facts}

      (terminal-body-present? lane-facts)
      {:status :indeterminate :reason :invalid-lane-terminal}

      (empty? run-entries)
      {:status :unresolved :reason :no-run}

      :else
      (let [dated
            (mapv (fn [{:keys [subject facts] :as entry}]
                    (assoc entry :instant (strict-run-instant facts)))
                  run-entries)]
        (cond
          (some #(or (not (valid-run-entity? (:subject %)))
                     (not (map? (:facts %)))
                     (nil? (:instant %)))
                dated)
          {:status :indeterminate :reason :invalid-run-ordering}

          :else
          (let [latest-instant
                (reduce (fn [latest candidate]
                          (if (pos? (.compareTo ^java.time.Instant candidate
                                               ^java.time.Instant latest))
                            candidate
                            latest))
                        (map :instant dated))
                latest (filterv #(= latest-instant (:instant %)) dated)]
            (if (not= 1 (count latest))
              {:status :indeterminate :reason :ambiguous-latest-run}
              (let [{:keys [subject facts]} (first latest)
                    agent (singleton-value facts "agent")
                    outcome (committed-run-process-outcome facts)]
                (cond
                  (not= control agent)
                  {:status :indeterminate :reason :invalid-run-agent
                   :run-subject subject}

                  (not (committed-run? facts))
                  {:status :indeterminate :reason :uncommitted-latest-run
                   :run-subject subject}

                  (nil? outcome)
                  {:status :indeterminate :reason :invalid-latest-run-terminal
                   :run-subject subject}

                  :else
                  {:status :resolved
                   :source :run
                   :run-subject subject
                   :outcome outcome
                   :delivery-outcome
                   (singleton-value facts "delivery_outcome")
                   :facts facts})))))))))

(defn lane-resolved?
  [control lane-facts run-entries]
  (= :resolved (:status (lane-resolution control lane-facts run-entries))))
