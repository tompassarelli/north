#!/usr/bin/env bb
;; Narrow managed writer for run-scoped done-bar evidence. The public adapter
;; supplies only bar + observation; run/thread/reporter come from the child
;; environment, and the per-run capability correlates the supported writer call
;; with the reservation committed before provider execution. This is an
;; application-integrity guard, not a same-UID security boundary: a process that
;; speaks Fram's loopback protocol directly can bypass this writer.
(ns north.delivery-evidence-internal
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(load-file (str (.getParent (io/file *file*)) "/coord.clj"))
(load-file (str (.getParent (io/file *file*)) "/terminal-projection.clj"))

(defn fail! [message data] (throw (ex-info message data)))

(defn checked! [result operation]
  (when (:reject result)
    (fail! (if (or (= :deadline (:reject result)) (:deadline result))
             "delivery evidence publication deadline exceeded"
             "coordinator rejected delivery evidence write")
           {:operation operation}))
  result)

(defn parse-request [raw]
  (let [raw (str raw)]
    (when-not
     (and (north.terminal-projection/valid-unicode-scalars? raw)
          (<= (north.terminal-projection/utf8-byte-count raw)
              north.terminal-projection/max-delivery-writer-request-utf8-bytes))
      (fail! "delivery evidence request exceeds its UTF-8 byte limit" {}))
    (try
      (let [parsed (json/parse-string raw)]
        (when-not (map? parsed)
          (fail! "delivery evidence request must be an object" {}))
        parsed)
      (catch clojure.lang.ExceptionInfo error (throw error))
      (catch Exception error
        (fail! "invalid delivery evidence JSON" {:cause (.getMessage error)})))))

(defn run-entity [raw]
  (let [value (str raw)
        canonical (if (str/starts-with? value "@") value (str "@" value))]
    (when-not (north.terminal-projection/valid-run-entity? canonical)
      (fail! "invalid delivery evidence run id" {:run raw}))
    canonical))

(defn agent-entity [raw]
  (let [value (str raw)
        canonical (if (str/starts-with? value "@") value (str "@" value))]
    (when-not (north.terminal-projection/valid-agent-entity? canonical)
      (fail! "invalid delivery evidence reporter" {:reporter raw}))
    canonical))

(defn thread-entity [raw]
  (let [value (str raw)
        canonical (if (str/starts-with? value "@") value (str "@" value))]
    (when-not (north.terminal-projection/valid-thread-entity? canonical)
      (fail! "invalid delivery evidence thread" {:thread raw}))
    canonical))

;; "The coordinator answered: this subject has no facts" and "the coordinator
;; did not answer" are DIFFERENT worlds, and every guard below reads them as
;; graph truth. Folding a non-answer into {} is what let one transient
;; :query-time-limit — Fram aborts a query whose cold projection rebuild outruns
;; FRAM_QUERY_TIMEOUT_MS, and every write invalidates that projection, so the
;; read right after a capture is exactly the one that pays it — reject a
;; well-formed thread as "a non-thread subject", and would equally have let a
;; non-fresh run subject pass the freshness gate. Only an :ok answer is evidence.
(def transient-query-stops
  #{:query-time-limit :query-work-limit :query-cancelled})

(defn query-answered? [response]
  (vector? (:ok response)))

;; ONE waiting budget for every read in a writer invocation, not one per read:
;; the subprocess boundary (sdk/src/delivery-evidence.ts) kills the writer at 10s
;; and the publication retry window already owns 5s of that, so per-read windows
;; would let a chain of unanswered reads outrun the boundary and lose the very
;; cause this fix exists to report. The budget is created on the FIRST retry, so
;; an answering coordinator never opens a second deadline (the reservation's one
;; shared body/digest deadline stays the only one), and every read still ATTEMPTS
;; once even after the budget is spent — exhaustion stops waiting, not asking.
(def read-retry-budget-ms 2000)
(def ^:private read-retry-deadline-ns (atom nil))

(defn- read-deadline-ns []
  (or @read-retry-deadline-ns
      (reset! read-retry-deadline-ns
              (north.coord/retry-deadline-ns read-retry-budget-ms))))

(defn query-rows!
  "Rows of one coordinator query, or a loud failure. A transient evaluation stop
   is retried inside the shared read budget; anything else fails immediately
   under its own name rather than impersonating an empty subject."
  [port subject query]
  (let [unanswered (atom nil)
        attempt!
        (fn []
          (let [response (north.coord/send-op port {:op :query :query query})]
            (if (query-answered? response)
              {:answered response}
              (do (reset! unanswered response)
                  (if (contains? transient-query-stops (:code response))
                    {:reject :conflict}
                    {:unanswered true})))))
        first-outcome (attempt!)
        outcome (if (= :conflict (:reject first-outcome))
                  (north.coord/retry-conflicts-until! (read-deadline-ns) attempt!)
                  first-outcome)]
    (if-let [answered (:answered outcome)]
      (:ok answered)
      (fail! "coordinator did not answer a delivery evidence read"
             {:subject subject
              :code (:code @unanswered)
              :response @unanswered}))))

(defn facts-of [port subject]
  (let [rows (query-rows!
              port subject
              {:find "delivery_evidence_fact"
               :rules [{:head {:rel "delivery_evidence_fact"
                               :args [{:var "p"} {:var "r"}]}
                        :body [{:rel "triple"
                                :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [acc [predicate value]]
              (update acc predicate (fnil conj #{}) value))
            {}
            rows)))

(defn exact-request! [request expected-keys]
  (when-not (= expected-keys (set (keys request)))
    (fail! "delivery evidence request has an invalid shape"
           {:expected expected-keys :actual (set (keys request))})))

(defn title-bearing-thread? [facts]
  (let [titles (get facts "title" #{})]
    (and (= 1 (count titles))
         (string? (first titles))
         (not (str/blank? (first titles))))))

;; A cap breach is a RECOVERABLE contract state, not a mystery: the caller needs
;; the offending bars verbatim (so it can see which are stale) and the one verb
;; that retires them. Reporting only a count is what left multi-bar threads with
;; no self-service path off the reserve limit.
(defn done-bar-limit-failure!
  [message thread facts extra]
  (let [bars (north.terminal-projection/done-bar-values facts)
        longest (reduce max 0
                        (keep #(north.terminal-projection/utf8-byte-count (str %))
                              bars))]
    (fail! (str message " (" (count bars) " done_when facts, limit "
                north.terminal-projection/max-delivery-bars
                "; longest bar " longest " bytes, limit "
                north.terminal-projection/max-delivery-bar-utf8-bytes
                "); active bars: "
                (north.terminal-projection/done-bar-diagnostic bars)
                "; retire stale bars in one step with: north bars prune "
                (str/replace-first (str thread) #"^@" ""))
           (assoc extra :thread thread :bars (count bars)))))

(defn ensure-reservable-contract!
  "Fail with the MOST SPECIFIC cause a caller can act on. Too many bars and one
   over-long bar are both recoverable by grooming, and both used to surface as
   the same opaque line — or, past the raw cap, as 'invalid proof text', which
   sent readers hunting for a malformed value that did not exist."
  [thread facts canonical label extra]
  (let [raw (north.terminal-projection/done-bar-values facts)]
    (when (> (count raw) north.terminal-projection/max-delivery-bars)
      (done-bar-limit-failure!
       (str label " exceeds delivery evidence limits") thread facts extra))
    (when-not (vector? canonical)
      (fail! (str label " contains invalid proof text")
             (assoc extra :thread thread)))
    (when-not (north.terminal-projection/bounded-done-bars? canonical true)
      (done-bar-limit-failure!
       (str label " exceeds delivery evidence limits") thread facts extra))))

(defn reserve! [port request]
  (exact-request! request #{"run" "thread" "reporter" "capabilitySha256"})
  (let [run (run-entity (get request "run"))
        thread (thread-entity (get request "thread"))
        reporter (agent-entity (get request "reporter"))
        capability-digest (get request "capabilitySha256")
        thread-facts (facts-of port thread)
        baseline (north.terminal-projection/canonical-done-when thread-facts)
        contract-origin (if (seq baseline) "accepted" "worker-defined")]
    (when-not (and (string? capability-digest)
                   (re-matches #"^[0-9a-f]{64}$" capability-digest))
      (fail! "invalid run capability digest" {}))
    (when-not (title-bearing-thread? thread-facts)
      (fail! "cannot reserve delivery evidence for a non-thread subject"
             {:thread thread :titles (get thread-facts "title" #{})}))
    (ensure-reservable-contract!
     thread thread-facts baseline "thread done_when contract" {:run run})
    (when (seq (facts-of port run))
      (fail! "run subject is not fresh" {:run run}))
    (let [projection
          (sorted-map
           "run_capability_sha256" capability-digest
           "run_reservation_agent" reporter
           "run_reservation_contract_origin" contract-origin
           "run_reservation_done_when" (json/generate-string baseline)
           "run_reservation_thread" thread
           "run_reservation_version"
           north.terminal-projection/run-reservation-version
           "run_reserved_at" (str (java.time.Instant/now)))
          marker
          (north.terminal-projection/run-reservation-manifest-sha256 projection)
          deadline-ns (north.coord/retry-deadline-ns)]
      (doseq [[predicate value] projection]
        (checked!
         (north.coord/retry-conflicts-until!
          deadline-ns
          #(north.coord/append! port run predicate value))
         [:append run predicate value]))
      (checked!
       (north.coord/assert-after-read!
        port run "run_reservation_manifest_sha256" marker
        (fn []
          (let [current-thread (facts-of port thread)]
            (when-not (title-bearing-thread? current-thread)
              (fail! "thread identity changed while reserving delivery evidence"
                     {:run run :thread thread}))
            (when-not (= baseline
                         (north.terminal-projection/canonical-done-when
                          current-thread))
              (fail! "thread contract changed while reserving delivery evidence"
                     {:run run :thread thread}))
            (when-not
             (north.terminal-projection/bounded-done-bars?
              (north.terminal-projection/canonical-done-when current-thread)
              true)
              (fail! "thread done_when contract exceeds delivery evidence limits"
                     {:run run :thread thread})))
          (let [stored (facts-of port run)]
            (when-not (= (into {} (map (fn [[predicate value]]
                                        [predicate #{value}])
                                      projection))
                         stored)
              (fail! "run reservation projection changed before commit"
                     {:run run :stored stored}))))
        Integer/MAX_VALUE deadline-ns)
       [:append-after-read run "run_reservation_manifest_sha256" marker])
      (let [stored (facts-of port run)]
        (when-not (and (north.terminal-projection/run-reservation-valid? stored)
                       (= (set (keys stored))
                          (conj (set (keys projection))
                                "run_reservation_manifest_sha256")))
          (fail! "run reservation lost singleton/freshness race"
                 {:run run :stored stored})))
      (println (json/generate-string
                (sorted-map "baselineDoneWhen" baseline
                            "contractOrigin" contract-origin
                            "ok" true "reporter" reporter
                            "run" run "thread" thread))))))

(defn validate-record-context!
  [port run thread reporter capability bar observed]
  (let [reservation (facts-of port run)
        evidence-state
        (north.terminal-projection/run-evidence-state
         reservation run thread reporter)]
    (when-not (north.terminal-projection/run-reservation-valid? reservation)
      (fail! "run has no valid committed reservation" {:run run}))
    (when-not (= #{reporter} (get reservation "run_reservation_agent"))
      (fail! "run reservation reporter mismatch" {:run run :reporter reporter}))
    (when-not (= #{thread} (get reservation "run_reservation_thread"))
      (fail! "run reservation thread mismatch" {:run run :thread thread}))
    (when-not (= #{(north.terminal-projection/sha256 capability)}
                 (get reservation "run_capability_sha256"))
      (fail! "run evidence capability mismatch" {:run run}))
    (when-not (:valid? evidence-state)
      (fail! "run contains malformed, cross-scoped, duplicate, or excessive evidence"
             {:run run}))
    (let [stored (:entries evidence-state)
          existing (first (filter #(= bar (get (second %) "bar")) stored))]
      ;; Exact replay remains authorized after terminal publication so the
      ;; non-authoritative human projection can be healed without mutating the
      ;; writer-scoped run evidence set.
      (if (and existing (= observed (get (second existing) "observed")))
        {:existing (first existing) :stored stored}
        (let [thread-facts (facts-of port thread)
              active-bars
              (north.terminal-projection/canonical-done-when thread-facts)
              baseline
              (north.terminal-projection/run-reservation-done-when reservation)
              origin
              (north.terminal-projection/singleton-value
               reservation "run_reservation_contract_origin")]
          ;; Every authority check below applies to a CORRECTION exactly as it
          ;; applies to a first observation: superseding is a fresh write by the
          ;; same reserved writer, not an exemption from the reservation.
          (when (contains? reservation "kind")
            (fail! "run evidence is closed after terminal publication" {:run run}))
          (ensure-reservable-contract!
           thread thread-facts active-bars
           "active done_when contract" {:run run})
          (when (and (= "accepted" origin) (not= baseline active-bars))
            (fail! "accepted done_when contract changed during the run"
                   {:run run :thread thread}))
          (when-not (contains? (set active-bars) bar)
            (fail! "evidence bar is not an active done_when on the reserved thread"
                   {:run run :thread thread :bar bar}))
          (if existing
            ;; A typo in an observation used to burn the bar's only slot for the
            ;; life of the run. The correction supersedes in place: the cap is
            ;; not re-checked because the record COUNT does not grow, and the
            ;; superseded text stays in the append-only Fram log.
            {:supersede (first existing)
             :superseded-observed (get (second existing) "observed")
             :stored stored}
            (do
              (when (>= (count stored)
                        north.terminal-projection/max-delivery-bars)
                (fail! "run evidence record cap reached" {:run run}))
              {:stored stored})))))))

(defn commit-record-once!
  "Commit one observation for RUN/BAR and report which observations it replaced.
   Supersession retracts the stale record BEFORE asserting the correction: the
   opposite order would leave a two-records-for-one-bar window, and every reader
   (run-evidence-state, terminal publication) treats a duplicate bar as a
   tampered set — a transient gap in one bar is recoverable, an invalid set is
   not. Each retract re-enters the loop so the assert always commits against a
   base captured AFTER it."
  [port run thread reporter capability bar observed raw]
  (loop [remaining 16
         superseded []]
    (let [base (north.coord/cur-ver port)
          context
          (validate-record-context!
           port run thread reporter capability bar observed)]
      (cond
        (:existing context)
        {:raw (:existing context) :superseded superseded}

        (:supersede context)
        (do
          (checked!
           (north.coord/retract! port run "run_bar_evidence" (:supersede context))
           [:retract run "run_bar_evidence" (:supersede context)])
          (if (> remaining 1)
            (recur (dec remaining)
                   (conj superseded (:superseded-observed context)))
            (fail! "run evidence supersession did not converge"
                   {:run run :bar bar})))

        :else
        (let [result
              (north.coord/send-op
               port {:op :assert-at-version
                     :te run :p "run_bar_evidence" :r raw :base base})]
          (if (and (= :conflict (:reject result)) (> remaining 1))
            (recur (dec remaining) superseded)
            (do
              (checked! result [:append-after-read run "run_bar_evidence" raw])
              {:raw raw :superseded superseded})))))))

(defn best-effort-thread-projection!
  [port thread bar observed superseded]
  ;; Human review convenience only. The writer-scoped run record is the
  ;; canonical acknowledgement; a thread projection outage must not turn that
  ;; irreversible success into a false CLI failure. Its literal is
  ;; idempotent, so a safe retry may heal it.
  ;;
  ;; A superseded observation's line is retracted so the human surface shows the
  ;; correction rather than both readings. Only the exact literal this run wrote
  ;; is retracted; the log keeps the whole history.
  (doseq [stale superseded
          :when (not= stale observed)]
    (try
      (north.coord/retract! port thread "bar_evidence" (str bar " → " stale))
      (catch Exception _ nil)))
  (try
    (north.coord/append! port thread "bar_evidence"
                         (str bar " → " observed))
    (catch Exception _ nil))
  nil)

(defn record! [port request]
  (exact-request! request
                  #{"run" "thread" "reporter" "capability" "bar" "observed"})
  (let [run (run-entity (get request "run"))
        thread (thread-entity (get request "thread"))
        reporter (agent-entity (get request "reporter"))
        capability (get request "capability")
        raw-bar (get request "bar")
        raw-observed (get request "observed")
        bar (north.terminal-projection/canonical-evidence-text raw-bar)
        observed (north.terminal-projection/canonical-evidence-text raw-observed)]
    (when-not (and (string? capability) (not (str/blank? capability)))
      (fail! "run evidence capability is missing" {}))
    (when-not
     (north.terminal-projection/bounded-nonblank-text?
      bar north.terminal-projection/max-delivery-bar-utf8-bytes)
      (fail! "done-bar must be nonblank and within its UTF-8 byte limit" {}))
    (when-not
     (north.terminal-projection/bounded-nonblank-text?
      observed north.terminal-projection/max-delivery-observed-utf8-bytes)
      (fail! "observed result must be nonblank and within its UTF-8 byte limit"
             {}))
    (let [record
          (sorted-map
           "bar" bar
           "observed" observed
           "recordedAt" (str (java.time.Instant/now))
           "reporter" reporter
           "run" run
           "thread" thread
           "version" north.terminal-projection/run-bar-evidence-version)
          raw (json/generate-string record)]
      (when-not (north.terminal-projection/run-bar-evidence-valid? record)
        (fail! "internal run evidence record failed validation" {:record record}))
      ;; One live record per run/bar makes retries idempotent and corrections
      ;; supersede in place. The scoped writer, active contract, open terminal,
      ;; and cap checks are read under the exact coordinator base used by the
      ;; append.
      (let [{:keys [raw superseded]}
            (commit-record-once!
             port run thread reporter capability bar observed raw)
            committed raw]
      (when-not (contains? (get (facts-of port run) "run_bar_evidence" #{})
                           committed)
        (fail! "run evidence was not acknowledged" {:run run}))
      (best-effort-thread-projection! port thread bar observed superseded)
      (println committed)))))

;; UNRESERVED fallback. A lane whose reservation never happened (no NORTH_RUN_ID
;; in its environment) still observed something real, and losing that entirely is
;; worse than recording it at a visibly lower tier. This writes ONE thread-scoped
;; fact under its own predicate and marker: it carries no run binding, no
;; capability, and no reporter authority, and nothing promotes it into run-bound
;; evidence. Its authority is exactly that of an ordinary `north tell` — which is
;; what it is, with canonicalization and bar matching enforced.
(defn record-unreserved! [port request]
  (exact-request! request #{"thread" "bar" "observed"})
  (let [thread (thread-entity (get request "thread"))
        bar (north.terminal-projection/canonical-evidence-text (get request "bar"))
        observed (north.terminal-projection/canonical-evidence-text
                  (get request "observed"))]
    (when-not
     (north.terminal-projection/bounded-nonblank-text?
      bar north.terminal-projection/max-unreserved-bar-utf8-bytes)
      (fail! "done-bar must be nonblank and within its UTF-8 byte limit" {}))
    (when-not
     (north.terminal-projection/bounded-nonblank-text?
      observed north.terminal-projection/max-delivery-observed-utf8-bytes)
      (fail! "observed result must be nonblank and within its UTF-8 byte limit"
             {}))
    (let [facts (facts-of port thread)
          active (north.terminal-projection/active-done-bar-texts facts)]
      (when-not (title-bearing-thread? facts)
        (fail! "cannot record unreserved evidence for a non-thread subject"
               {:thread thread :titles (get facts "title" #{})}))
      (when-not (contains? (set active) bar)
        (fail! (str "evidence bar is not an active done_when on the thread; "
                    "active bars: "
                    (north.terminal-projection/done-bar-diagnostic active))
               {:thread thread :bar bar}))
      (let [literal
            (north.terminal-projection/unreserved-bar-evidence-literal
             bar observed)
            prefix
            (north.terminal-projection/unreserved-bar-evidence-prefix bar)
            stale
            (filter #(and (string? %)
                          (str/starts-with? % prefix)
                          (not= % literal))
                    (get facts
                         north.terminal-projection/unreserved-bar-evidence-predicate
                         #{}))]
        (when-not literal
          (fail! "internal unreserved evidence literal failed validation" {}))
        ;; Predicate written as a LITERAL on purpose: cli/tests/pred-cli-test.clj
        ;; only binds a fixed predicate string to the VOCAB registry.
        (checked!
         (north.coord/append! port thread "bar_evidence_unreserved" literal)
         [:append thread "bar_evidence_unreserved" literal])
        ;; Same supersession rule as the run-scoped path, one tier down: the
        ;; corrected reading replaces the stale line and the log keeps both.
        (doseq [value stale]
          (try
            (north.coord/retract! port thread "bar_evidence_unreserved" value)
            (catch Exception _ nil)))
        (println
         (json/generate-string
          (sorted-map
           "bar" bar
           "observed" observed
           "recordedAt" (str (java.time.Instant/now))
           "scope" "unreserved"
           "superseded" (count stale)
           "thread" thread
           "version"
           north.terminal-projection/unreserved-bar-evidence-version)))))))

(defn -main []
  (let [[port-s operation raw] *command-line-args*
        port (Integer/parseInt
              (or port-s (or (System/getenv "NORTH_PORT") "7977")))
        request (parse-request (if (some? raw) raw (slurp *in*)))]
    (case operation
      "reserve" (reserve! port request)
      "record" (record! port request)
      "record-unreserved" (record-unreserved! port request)
      (fail! "unsupported delivery evidence operation"
             {:operation operation}))))

(when (= *file* (System/getProperty "babashka.file"))
  (-main))
