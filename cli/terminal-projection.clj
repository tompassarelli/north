(ns north.terminal-projection
  (:require [cheshire.core :as json]
            [clojure.string :as str]))

(def terminal-predicates ["process_outcome" "delivery_outcome" "delivery_reason"])

(def delivery-proof-predicates ["delivery_evidence" "delivery_evidence_sha256"])

(def terminal-projection-predicates (into terminal-predicates delivery-proof-predicates))

(def run-resolution-predicates (into ["agent" "at" "kind"] terminal-projection-predicates))

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

(def max-unreserved-bar-utf8-bytes (* 4 1024))

(def max-done-bar-diagnostic-utf8-bytes 8192)

(def unreserved-bar-evidence-predicate "bar_evidence_unreserved")

(def unreserved-bar-evidence-version "north:unreserved-bar-evidence:v1")

(def unreserved-bar-evidence-marker "unreserved ·")

(def run-reservation-version "north:run-reservation:v1")

(def run-reservation-body-predicates ["run_capability_sha256" "run_reservation_agent" "run_reservation_contract_origin" "run_reservation_done_when" "run_reservation_thread" "run_reservation_version" "run_reserved_at"])

(def run-reservation-predicates (conj run-reservation-body-predicates "run_reservation_manifest_sha256"))

(defn- values-of [facts predicate]
  (let [value (get facts predicate ::absent)]
  (cond
  (= ::absent value) []
  (set? value) (vec value)
  (and (sequential? value) (not (string? value))) (vec value)
  :else [value])))

(defn fact-present? [facts predicate]
  (boolean (seq (values-of facts predicate))))

(defn singleton-value
  "One exact nonblank string value, or nil for absent/conflicting/malformed\n  facts. Maps folded to scalar values and maps folded to sets are both accepted." [facts predicate]
  (let [values (values-of facts predicate)]
  (if (= 1 (count values)) (do
  (let [value (first values)]
  (if (and (string? value) (not (str/blank? value))) (do
  value)))))))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8))]
  (format "%064x" (java.math.BigInteger. 1 digest))))

(defn utf8-byte-count [^String value]
  (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8)))

(defn valid-unicode-scalars?
  "True only when VALUE contains no unpaired UTF-16 surrogate. Java's default\n  UTF-8 encoder silently replaces malformed surrogate code units, so proof\n  byte limits must reject them before encoding." [value]
  (boolean (if (string? value) (do
  (loop [index 0]
  (if (= index (.length value)) true (let [current (.charAt value index)]
  (cond
  (Character/isHighSurrogate current) (and (< (inc index) (.length value)) (Character/isLowSurrogate (.charAt value (inc index))) (recur (+ index 2)))
  (Character/isLowSurrogate current) false
  :else (recur (inc index))))))))))

(defn- ^Boolean evidence-control-code-unit? [code-unit]
  (or (and (<= 0 code-unit) (<= code-unit 0x1f)) (and (<= 0x7f code-unit) (<= code-unit 0x9f))))

(defn canonical-evidence-text
  "Canonical proof text shared with the SDK: valid Unicode scalar sequence,\n  no C0/C1 controls, and ASCII SPACE trimmed at the two edges. Unicode spaces\n  such as NBSP and EM SPACE remain content rather than acquiring runtime-\n  specific trim semantics." [value]
  (if (and (string? value) (valid-unicode-scalars? value) (not-any? (fn [__north_anon_1] (evidence-control-code-unit? (int __north_anon_1))) value)) (do
  (let [canonical (str/replace value #"^ +| +$" "")]
  (if (seq canonical) (do
  canonical))))))

(defn bounded-nonblank-text? [value max-bytes]
  (and (string? value) (= value (canonical-evidence-text value)) (<= (utf8-byte-count value) max-bytes)))

(def entity-whitespace-code-units (into #{0x20 0xa0 0x1680 0x2028 0x2029 0x202f 0x205f 0x3000 0xfeff} (range 0x2000 0x200b)))

(defn- forbidden-entity-code-unit? [code-unit]
  (or (= code-unit (int \@)) (evidence-control-code-unit? code-unit) (contains? entity-whitespace-code-units code-unit)))

(defn valid-thread-entity? [value]
  (boolean (and (string? value) (valid-unicode-scalars? value) (<= (utf8-byte-count value) max-delivery-thread-id-utf8-bytes) (str/starts-with? value "@") (> (.length value) 1) (not-any? (fn [__north_anon_1] (forbidden-entity-code-unit? (int __north_anon_1))) (subs value 1)))))

(defn valid-run-entity? [value]
  (boolean (and (string? value) (<= (utf8-byte-count value) max-delivery-run-id-utf8-bytes) (re-matches #"^@run[-:][A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn valid-agent-entity? [value]
  (boolean (and (string? value) (<= (utf8-byte-count value) max-delivery-agent-id-utf8-bytes) (re-matches #"^@agent:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn bounded-done-bars? [bars allow-empty?]
  (and (vector? bars) (or allow-empty? (seq bars)) (<= (count bars) max-delivery-bars) (every? (fn [__north_anon_1] (bounded-nonblank-text? __north_anon_1 max-delivery-bar-utf8-bytes)) bars) (= bars (vec (sort (distinct bars))))))

(declare instant?)

(defn canonical-done-when
  "Canonical semantic done-bar set used at reservation, assessment, and commit:\n  ASCII-space trimmed, nonblank, Unicode-safe, unique, and lexical. Nil means\n  at least one stored done_when value is malformed; invalid facts are never\n  silently dropped from an authority boundary." [facts]
  (let [raw (values-of facts "done_when")
   canonical (mapv canonical-evidence-text raw)]
  (if (and (<= (count raw) max-delivery-bars) (every? some? canonical)) (do
  (->> canonical distinct sort vec)))))

(defn done-bar-values
  "Every raw stored done_when value. DIAGNOSTICS ONLY — it keeps malformed and\n  over-cap values so an error can quote what is actually on the thread; it is\n  never an authority set (canonical-done-when is)." [facts]
  (values-of facts "done_when"))

(defn active-done-bar-texts
  "Canonical bar texts a caller may cite right now, tolerating a contract that\n  canonical-done-when rejects as a whole. Malformed values are dropped HERE\n  because this answers 'may this exact text be cited', never 'is this contract\n  admissible' — every authority boundary keeps using canonical-done-when." [facts]
  (->> (done-bar-values facts) (map canonical-evidence-text) (filter some?) distinct sort vec))

(defn- single-line-diagnostic-text [value]
  (str/replace (str value) #"[\p{Cntrl}\u0080-\u009f]" " "))

(defn done-bar-diagnostic
  "One-line verbatim rendering of BARS for a writer error. Writer failures reach\n  a coordinator as a single `Message:` line, so a done-bar error that only\n  reports a COUNT leaves no way to see which bars to retire. Bounded by\n  max-done-bar-diagnostic-utf8-bytes; the remainder is counted, never dropped\n  in silence." [bars]
  (let [items (mapv (fn [__north_anon_1] (str "\"" (single-line-diagnostic-text __north_anon_1) "\"")) bars)]
  (if (empty? items) "(none)" (loop [remaining items
   shown []
   budget max-done-bar-diagnostic-utf8-bytes]
  (let [bind__0 (first remaining)]
  (if bind__0 (let [^String item bind__0]
  (let [cost (+ (utf8-byte-count item) (if (seq shown) 3 0))]
  (if (<= cost budget) (recur (rest remaining) (conj shown item) (- budget cost)) (str (if (seq shown) (do
  (str (str/join " | " shown) " | "))) "(+" (count remaining) " more omitted)")))) (str/join " | " shown)))))))

(defn unreserved-bar-evidence-literal
  "Thread-scoped observation recorded with NO run reservation. Self-labelling by\n  construction so no reader — human or projection — can mistake it for\n  run-bound verification, and no path upgrades it into one." [bar observed]
  (if (and (bounded-nonblank-text? bar max-unreserved-bar-utf8-bytes) (bounded-nonblank-text? observed max-delivery-observed-utf8-bytes)) (do
  (str unreserved-bar-evidence-marker " " bar " → " observed))))

(defn unreserved-bar-evidence-prefix
  "Literal prefix of every unreserved observation for one exact bar; used to\n  find the stale lines one re-record supersedes." [bar]
  (str unreserved-bar-evidence-marker " " bar " → "))

(defn run-reservation-done-when
  "Parse the manifest-bound canonical reservation baseline, including an empty\n  vector for an explicitly worker-defined contract. Nil means malformed." [facts]
  (let [bind__1 (singleton-value facts "run_reservation_done_when")]
  (if bind__1 (let [raw bind__1]
  (do
  (if (<= (utf8-byte-count raw) max-run-reservation-baseline-utf8-bytes) (do
  (try
  (let [decoded (json/parse-string raw)
   parsed (if (and (sequential? decoded) (not (string? decoded))) (do
  (vec decoded)))]
  (if (and parsed (bounded-done-bars? parsed true)) (do
  parsed)))
  (catch Exception _
    nil)))))))))

(defn run-reservation-manifest-sha256 [projection]
  (let [body (into (sorted-map) (select-keys projection run-reservation-body-predicates))]
  (if (= (count body) (count run-reservation-body-predicates)) (do
  (sha256 (apply str (map (fn [[predicate value]] (str predicate "\u0000" value "\n")) body)))))))

(defn run-reservation-valid?
  "A reservation is committed only when every authority field is singleton and\n  its marker digests the exact projection. Additional run_bar_evidence values\n  are allowed; conflicting reservation publishers invalidate the subject." [facts]
  (let [body (into (sorted-map) (keep (fn [predicate] (let [bind__2 (singleton-value facts predicate)]
  (if bind__2 (let [value bind__2]
  (do
  [predicate value])))))) run-reservation-body-predicates)
   marker (singleton-value facts "run_reservation_manifest_sha256")
   expected (run-reservation-manifest-sha256 body)
   contract-origin (get body "run_reservation_contract_origin")
   baseline (run-reservation-done-when facts)]
  (boolean (and (= (count body) (count run-reservation-body-predicates)) (every? (fn [__north_anon_1] (= 1 (count (values-of facts __north_anon_1)))) run-reservation-predicates) (= run-reservation-version (get body "run_reservation_version")) (valid-agent-entity? (get body "run_reservation_agent")) (valid-thread-entity? (get body "run_reservation_thread")) (re-matches #"^[0-9a-f]{64}$" (get body "run_capability_sha256")) (instant? (get body "run_reserved_at")) (#{"accepted" "worker-defined"} contract-origin) (some? baseline) (if (= "accepted" contract-origin) (seq baseline) (empty? baseline)) expected (= marker expected)))))

(defn terminal-manifest-sha256
  "Digest the exact base terminal plus any delivery-evidence projection using\n  the writer's canonical encoding." [facts]
  (let [required (into {} (keep (fn [predicate] (let [bind__3 (singleton-value facts predicate)]
  (if bind__3 (let [value bind__3]
  (do
  [predicate value])))))) terminal-predicates)
   conflicts? (some (fn [__north_anon_1] (> (count (values-of facts __north_anon_1)) 1)) terminal-projection-predicates)
   projection (into (sorted-map) (keep (fn [predicate] (let [bind__4 (singleton-value facts predicate)]
  (if bind__4 (let [value bind__4]
  (do
  [predicate value])))))) terminal-projection-predicates)]
  (if (and (= (count terminal-predicates) (count required)) (not conflicts?)) (do
  (sha256 (apply str (map (fn [[predicate value]] (str predicate "\u0000" value "\n")) projection)))))))

(defn- parse-json-map [raw]
  (try
  (let [parsed (json/parse-string raw)]
  (if (map? parsed) (do
  parsed)))
  (catch Exception _
    nil)))

(defn instant? [value]
  (boolean (if (string? value) (do
  (let [bind__5 (re-matches #"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$" value)]
  (if bind__5 (let [[_ year month day hour minute second] bind__5]
  (do
  (try
  (let [year-int (or (parse-long year) 0)
   month-int (or (parse-long month) 0)
   day-int (or (parse-long day) 0)
   hour (or (parse-long hour) 0)
   minute (or (parse-long minute) 0)
   second (or (parse-long second) 0)]
  (and (< hour 24) (< minute 60) (< second 60) (do
  (java.time.LocalDate/of year-int month-int day-int)
  (java.time.Instant/parse value)
  true)))
  (catch Exception _
    false))))))))))

(defn run-bar-evidence-valid? [record]
  (boolean (and (map? record) (= #{"bar" "observed" "recordedAt" "reporter" "run" "thread" "version"} (set (keys record))) (= run-bar-evidence-version (get record "version")) (valid-run-entity? (get record "run")) (valid-thread-entity? (get record "thread")) (valid-agent-entity? (get record "reporter")) (bounded-nonblank-text? (get record "bar") max-delivery-bar-utf8-bytes) (bounded-nonblank-text? (get record "observed") max-delivery-observed-utf8-bytes) (instant? (get record "recordedAt")))))

(defn parse-run-bar-evidence
  "Parse one stored record only after its raw UTF-8 envelope is bounded." [raw]
  (if (and (string? raw) (<= (utf8-byte-count raw) max-run-bar-evidence-record-utf8-bytes)) (do
  (try
  (let [record (json/parse-string raw)]
  (if (run-bar-evidence-valid? record) (do
  record)))
  (catch Exception _
    nil)))))

(defn run-evidence-state
  "Validate the entire evidence set on one reserved run. A malformed,\n  cross-scoped, duplicate-bar, or over-cap record invalidates the set; callers\n  must not cherry-pick the valid subset." [facts run thread reporter]
  (let [raws (values-of facts "run_bar_evidence")
   parsed (mapv parse-run-bar-evidence raws)
   scoped? (every? (fn [record] (and record (= run (get record "run")) (= thread (get record "thread")) (= reporter (get record "reporter")))) parsed)
   bars (if scoped? (do
  (mapv (fn [__north_anon_1] (get __north_anon_1 "bar")) parsed)))
   valid? (and (<= (count raws) max-delivery-bars) scoped? (= (count bars) (count (distinct bars))))]
  {:valid? (boolean valid?) :entries (if valid? (loop [raw-items raws
   parsed-items parsed
   entries []]
  (if (seq raw-items) (recur (rest raw-items) (rest parsed-items) (conj entries [(first raw-items) (first parsed-items)])) entries)) []) :raws (if valid? (set raws) #{}) :records (if valid? parsed [])}))

(defn ^Boolean evidence-reports-bar?
  "Human thread-review projection only. Delivery qualification uses structured\n  run_bar_evidence; this parser keeps needs-review/routing context compatible." [bar evidence]
  (let [^String bar (str/trim (str bar))
   ^String evidence (str/trim (str evidence))]
  (boolean (and (seq bar) (seq evidence) (str/starts-with? evidence bar) (re-matches #"^\s*→\s*\S.*$" (subs evidence (count bar)))))))

(defn- valid-evidence-envelope? [raw digest]
  (boolean (and (string? raw) (valid-unicode-scalars? raw) (<= (utf8-byte-count raw) max-delivery-envelope-utf8-bytes) (let [parsed (parse-json-map raw)
   baseline-bars (get parsed "baselineDoneWhen")
   bars (get parsed "doneWhen")
   matches (get parsed "matches")
   thread (get parsed "thread")
   reporter (get parsed "reporter")
   run (get parsed "run")
   contract-origin (get parsed "contractOrigin")]
  (and parsed (= (set (keys parsed)) #{"version" "run" "thread" "reporter" "contractOrigin" "baselineDoneWhen" "doneWhen" "matches"}) (= digest (sha256 raw)) (= delivery-evidence-version (get parsed "version")) (valid-run-entity? run) (valid-thread-entity? thread) (valid-agent-entity? reporter) (#{"accepted" "worker-defined"} contract-origin) (bounded-done-bars? baseline-bars true) (bounded-done-bars? bars false) (if (= "accepted" contract-origin) (and (seq baseline-bars) (= baseline-bars bars)) (empty? baseline-bars)) (vector? matches) (= (count bars) (count matches)) (every? (fn [[bar match]] (let [evidence (get match "evidence")]
  (and (map? match) (= #{"bar" "evidence"} (set (keys match))) (= bar (get match "bar")) (vector? evidence) (= 1 (count evidence)) (every? (fn [__north_anon_1] (and (run-bar-evidence-valid? __north_anon_1) (= bar (get __north_anon_1 "bar")) (= run (get __north_anon_1 "run")) (= thread (get __north_anon_1 "thread")) (= reporter (get __north_anon_1 "reporter")))) evidence)))) (loop [bar-items bars
   match-items matches
   pairs []]
  (if (seq bar-items) (recur (rest bar-items) (rest match-items) (conj pairs [(first bar-items) (first match-items)])) pairs))))))))

(defn delivery-proof [facts]
  (let [evidence (singleton-value facts "delivery_evidence")
   evidence-digest (singleton-value facts "delivery_evidence_sha256")]
  (if (or evidence evidence-digest) (do
  {:evidence evidence :evidence-digest evidence-digest}))))

(defn delivery-projection-valid?
  "Reported requires a complete run-scoped self-reported done-bar snapshot.\n  Unverified and blocked forbid evidence residue." [facts]
  (let [outcome (singleton-value facts "delivery_outcome")
   reason (singleton-value facts "delivery_reason")
   process (singleton-value facts "process_outcome")
   proof (delivery-proof facts)
   proof-fact-count (count (filter (fn [__north_anon_1] (fact-present? facts __north_anon_1)) delivery-proof-predicates))
   {:keys [evidence evidence-digest]} proof
   evidence-valid? (and evidence evidence-digest (valid-evidence-envelope? evidence evidence-digest))]
  (boolean (and process (if (= "ran" process) (not= "blocked" outcome) (= "blocked" outcome)) (case outcome
    "blocked" (and (zero? proof-fact-count) (some? reason))
    "unverified" (and (zero? proof-fact-count) (some? reason))
    "reported" (and (= "complete_run_scoped_done_bar_evidence_self_reported" reason) (= 2 proof-fact-count) evidence-valid?)
    false)))))

(defn terminal-manifest-valid? [facts]
  (let [marker (singleton-value facts "terminal_manifest_sha256")
   process (singleton-value facts "process_outcome")
   expected (terminal-manifest-sha256 facts)]
  (boolean (and marker process expected (= marker expected) (delivery-projection-valid? facts)))))

(defn terminal-delivery-outcome
  "Delivery state only from a complete, digest-committed, proof-valid terminal." [facts]
  (if (terminal-manifest-valid? facts) (do
  (some-> (singleton-value facts "delivery_outcome") str/trim))))

(defn terminal-process-outcome
  "Resolve a lane terminal from a valid process_outcome manifest." [facts]
  (if (and (fact-present? facts "process_outcome") (terminal-manifest-valid? facts)) (do
  (some-> (singleton-value facts "process_outcome") str/trim))))

(defn committed-run?
  "kind=run is the run writer's last-write commit marker." [facts]
  (= "run" (singleton-value facts "kind")))

(defn committed-run-process-outcome
  "Resolve a run terminal only after kind=run committed the row." [facts]
  (if (committed-run? facts) (do
  (if (and (fact-present? facts "process_outcome") (terminal-manifest-sha256 facts) (delivery-projection-valid? facts)) (do
  (some-> (singleton-value facts "process_outcome") str/trim))))))

(defn- terminal-body-present? [facts]
  (boolean (some (fn [__north_anon_1] (fact-present? facts __north_anon_1)) (conj terminal-projection-predicates "terminal_manifest_sha256"))))

(defn- strict-run-instant [facts]
  (let [at (singleton-value facts "at")]
  (if (instant? at) (do
  (java.time.Instant/parse at)))))

(defn lane-resolution
  "Canonical execution resolution for one managed lane.\n\n  RUN-ENTRIES are maps with :subject and :facts. The valid digest-committed lane\n  terminal is primary. With no lane terminal body, the latest exact run by its\n  singleton ISO timestamp may resolve the lane only when its last-write kind\n  marker, agent identity, and terminal projection are all valid. Ambiguous,\n  torn, conflicting, uncommitted, or nonterminal evidence is indeterminate:\n  callers must neither call it active nor finished." [control lane-facts run-entries]
  (let [lane-outcome (terminal-process-outcome lane-facts)]
  (cond
  lane-outcome {:status :resolved :source :agent :outcome lane-outcome :delivery-outcome (terminal-delivery-outcome lane-facts) :facts lane-facts}
  (terminal-body-present? lane-facts) {:status :indeterminate :reason :invalid-lane-terminal}
  (empty? run-entries) {:status :unresolved :reason :no-run}
  :else (let [dated (mapv (fn [{:keys [subject facts] :as entry}] (assoc entry :instant (strict-run-instant facts))) run-entries)]
  (cond
  (some (fn [__north_anon_1] (or (not (valid-run-entity? (:subject __north_anon_1))) (not (map? (:facts __north_anon_1))) (nil? (:instant __north_anon_1)))) dated) {:status :indeterminate :reason :invalid-run-ordering}
  :else (let [latest-instant (reduce (fn [latest candidate] (if (pos? (.compareTo ^java.time.Instant candidate ^java.time.Instant latest)) candidate latest)) (map (fn [entry] (:instant entry)) dated))
   latest (filterv (fn [__north_anon_1] (= latest-instant (:instant __north_anon_1))) dated)]
  (if (not= 1 (count latest)) {:status :indeterminate :reason :ambiguous-latest-run} (let [{:keys [subject facts]} (first latest)
   agent (singleton-value facts "agent")
   outcome (committed-run-process-outcome facts)]
  (cond
  (not= control agent) {:status :indeterminate :reason :invalid-run-agent :run-subject subject}
  (not (committed-run? facts)) {:status :indeterminate :reason :uncommitted-latest-run :run-subject subject}
  (nil? outcome) {:status :indeterminate :reason :invalid-latest-run-terminal :run-subject subject}
  :else {:status :resolved :source :run :run-subject subject :outcome outcome :delivery-outcome (singleton-value facts "delivery_outcome") :facts facts})))))))))

(defn lane-resolved? [control lane-facts run-entries]
  (= :resolved (:status (lane-resolution control lane-facts run-entries))))
