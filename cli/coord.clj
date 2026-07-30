;; coord.clj — the ONE shared coordination substrate for the north *-cli.clj
;; scripts (Foundation thread 019f100f Part B). Every CLI spoke the :7977 daemon
;; wire (:assert / :version / :retract / :resolved / :query) through a VERBATIM
;; copy of these helpers — 10 copies of send-op, 5 of assert!, 2 of retract!, and
;; ~11 single/multi resolved variants. One drift in any copy and the swarm's
;; coordination silently diverges. This is the single definition they all load.
;;
;; WRITE VERBS — cardinality-typed (move-C). The one global-version CAS ritual that
;; every assert! cargo-culted (read GLOBAL :version, pass it as the per-fact base,
;; retry) is GONE. It is replaced by three verbs whose choice is the predicate's
;; cardinality, NOT a base dance:
;;   append!  MULTI            one op, NO base, NO retry  — rival/disjoint writes
;;                             coexist (engine appends; identical is idempotent).
;;   put!     SINGLE  LWW      one op, NO base            — engine supersedes a
;;                             declared-single pred (last writer wins).
;;   swap!    SINGLE  CAS      base + retry  — the ONLY base+retry verb; opt-IN
;;                             conflict-detection for a genuine read-modify-write.
;; append!/put! pass NO :base, so the (now base-OPTIONAL) engine never staleness-
;; rejects them; only swap! threads a base. assert! survives as a thin alias to swap!
;; (byte-for-byte the old CAS behavior) for any un-migrated straggler.
;;
;; DUAL MODE (the schema-validate.clj precedent): load-file'd by a sibling CLI as a
;; library, OR run directly as a connectivity smoke. The main-guard keeps the CLI
;; dormant when another script loads us:
;;   bb cli/coord.clj <port>            -> prints the daemon's :version (a ping)
;; Load it sibling-relative so cwd never matters:
;;   (load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
;; then call north.coord/send-op (or rebind the local names you use).
(ns north.coord
  (:require [clojure.edn :as edn] [clojure.java.io :as io] [clojure.string :as str]))

;; The canonical coordinator port. The CLIs take <port> as argv[0]; PORT is the
;; default/canonical reference (Part C's pred-cli + future callers read it).
(def PORT (or (System/getenv "NORTH_PORT") "7977"))

;; Stage-A telemetry partition. The flag is deliberately explicit for the first
;; cut: NORTH_TELEMETRY_PARTITION=1 routes telemetry-owned subjects to the
;; independently fenced writer named by NORTH_TELEMETRY_PORT and
;; FRAM_TELEMETRY_LOG. Setting the one flag to 0 restores the prior unified
;; coordinator path without moving or rewriting either origin log.
(def telemetry-subject-tokens #{"run" "session" "mine" "guard_denial"})
(def ^:dynamic *operation-domain* nil)

(defn telemetry-partition-enabled? []
  (= "1" (System/getenv "NORTH_TELEMETRY_PARTITION")))

(defn- configured-telemetry-port []
  (when (telemetry-partition-enabled?)
    (let [raw (System/getenv "NORTH_TELEMETRY_PORT")
          value (some-> raw parse-long)]
      (when-not (and value (<= 1 value 65535))
        (throw
         (ex-info
          "NORTH_TELEMETRY_PORT must be an integer from 1 through 65535 when NORTH_TELEMETRY_PARTITION=1"
          {:type :invalid-telemetry-port :value raw})))
      (int value))))

(defn telemetry-log-path []
  (when (telemetry-partition-enabled?)
    (let [path (System/getenv "FRAM_TELEMETRY_LOG")]
      (when (str/blank? path)
        (throw
         (ex-info
          "FRAM_TELEMETRY_LOG is required when NORTH_TELEMETRY_PARTITION=1"
          {:type :missing-telemetry-log})))
      (.getCanonicalPath (io/file path)))))

(defn telemetry-subject? [subject]
  (boolean
   (when (and (string? subject) (str/starts-with? subject "@"))
     (let [colon (str/index-of subject ":")
           token (when (and colon (> colon 1)) (subs subject 1 colon))]
       (contains? telemetry-subject-tokens token)))))

(defn- query-literal-subjects [query]
  (->> (concat (:rules query) (mapcat identity (:strata query)))
       (mapcat :body)
       (keep (fn [literal]
               (let [subject (first (:args literal))]
                 (when (and (= "triple" (:rel literal))
                            (string? subject)
                            (str/starts-with? subject "@"))
                   subject))))
       set))

(defn- operation-subject [operation]
  (or (:te operation)
      (let [subjects (when (#{:query :query-page} (:op operation))
                       (query-literal-subjects (:query operation)))]
        (when (= 1 (count subjects)) (first subjects)))))

(declare expected-log)

(defn route-for-operation [requested-port operation]
  (let [subject (operation-subject operation)]
    (if (and (telemetry-partition-enabled?)
             (or (= :telemetry *operation-domain*)
                 (and subject (telemetry-subject? subject))))
      {:port (configured-telemetry-port)
       :log (telemetry-log-path)
       :domain :telemetry}
      {:port requested-port
       :log (expected-log)
       :domain :coordination})))

(defn- timeout-ms [name default]
  (let [raw (or (System/getenv name) (str default))]
    (when-not (re-matches #"[1-9][0-9]{0,5}" raw)
      (throw (ex-info (str name " must be an integer from 1 through 999999 milliseconds")
                      {:type :invalid-coordinator-timeout :name name :value raw})))
    (Integer/parseInt raw)))

(def ^:dynamic *response-byte-limit-override* nil)

;; 64 MiB, not 8. The cap bounds how much one response may consume, but 8 MiB sat
;; BELOW what North's own warm path needs: the whole-corpus `:facts` view is
;; ~345k triples, and both the coordination and telemetry domains blew the limit
;; on every call. The failure was invisible and expensive — `live-triples-at`
;; marked the domain unavailable, north fell back to a COLD FOLD of the 36 MB log
;; on disk, and the answer was still correct, just far slower.
;;
;; Measured 2026-07-29, `north validate`, same corpus and load:
;;   8 MiB cap   44,326 ms   (cap exceeded -> cold fold)
;;   64 MiB cap  21,047 ms   (warm path)
;; 64 MiB is already the maximum this function permits, so this raises the
;; default to the ceiling the policy had always allowed rather than inventing a
;; new bound.
;;
;; This does NOT make the whole-corpus fetch cheap — that is a separate refactor
;; (predicate-scoped reads). It stops a silent 2x penalty on top of it.
(def ^:private default-response-byte-limit "67108864")

(defn- response-byte-limit []
  (if *response-byte-limit-override*
    *response-byte-limit-override*
    (let [raw (or (System/getenv "NORTH_COORD_MAX_RESPONSE_BYTES")
                  default-response-byte-limit)
          value (when (re-matches #"[1-9][0-9]{0,7}" raw)
                  (parse-long raw))]
      (when-not (and value (<= value 67108864))
        (throw
         (ex-info
          "NORTH_COORD_MAX_RESPONSE_BYTES must be an integer from 1 through 67108864"
          {:type :invalid-coordinator-response-limit :value raw})))
      (int value))))

(def query-page-response-byte-limit 1048576)
(def query-page-row-limit 4096)
(def query-page-cursor-byte-limit 4096)
(def query-page-cursor-prefix "fram-query-page-v1.")

(defn- base64url-encode-utf8 [value]
  (.encodeToString
   (.withoutPadding (java.util.Base64/getUrlEncoder))
   (.getBytes ^String value java.nio.charset.StandardCharsets/UTF_8)))

(defn- base64url-decode-utf8 [value]
  (let [bytes (.decode (java.util.Base64/getUrlDecoder) ^String value)
        decoder
        (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
          (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
          (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
    (str (.decode decoder (java.nio.ByteBuffer/wrap bytes)))))

(defn valid-query-page-cursor? [value]
  (and
   (string? value)
   (<= (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8))
       query-page-cursor-byte-limit)
   (.startsWith ^String value query-page-cursor-prefix)
   (> (count value) (count query-page-cursor-prefix))
   (try
     (let [payload (subs value (count query-page-cursor-prefix))
           decoded (base64url-decode-utf8 payload)]
       (= value
          (str query-page-cursor-prefix
               (base64url-encode-utf8 decoded))))
     (catch Exception _ false))))

(defn connect-socket [port]
  (let [s (java.net.Socket.)]
    (try
      (.connect s
                (java.net.InetSocketAddress. "127.0.0.1" (int port))
                (timeout-ms "NORTH_COORD_CONNECT_TIMEOUT_MS" 1000))
      (.setSoTimeout s (timeout-ms "NORTH_COORD_READ_TIMEOUT_MS" 30000))
      s
      (catch Throwable t
        (.close s)
        (throw t)))))

(defn- decode-utf8! [bytes]
  (try
    (let [decoder
          (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
            (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
            (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
      (str (.decode decoder (java.nio.ByteBuffer/wrap bytes))))
    (catch java.nio.charset.CharacterCodingException error
      (throw
       (ex-info "coordinator response line is not valid UTF-8"
                {:type :malformed-coordinator-utf8}
                error)))))

(defn- response-timeout! [timeout cause]
  (throw
   (ex-info "coordinator response deadline exceeded"
            {:type :coordinator-response-timeout
             :timeout-ms timeout}
            cause)))

;; North keeps this small stdlib-only client instead of loading fram.rt: hooks
;; and sibling CLIs load coord.clj directly, without Fram's kernel/fold/Cheshire
;; classpath. The wire invariants still match Fram's client: bounded UTF-8,
;; absolute deadlines, exactly one parsed form, and exactly one terminal frame.
(defrecord CoordinatorReader [socket input buffer bounds])

(defn coordinator-reader [socket]
  (->CoordinatorReader
   socket
   (.getInputStream socket)
   (byte-array 65536)
   (int-array 2)))

(defn- as-reader [source]
  (if (instance? CoordinatorReader source)
    source
    (coordinator-reader source)))

(defn- finish-line! [output]
  (let [line (decode-utf8! (.toByteArray output))]
    (if (str/ends-with? line "\r")
      (subs line 0 (dec (count line)))
      line)))

(defn- arm-deadline! [socket deadline timeout]
  (let [remaining-ns (- deadline (System/nanoTime))]
    (when-not (pos? remaining-ns)
      (response-timeout! timeout nil))
    (.setSoTimeout
     socket
     (int (max 1 (quot (+ remaining-ns 999999) 1000000))))))

(defn- read-line-limited! [source deadline timeout eof-ok?]
  (let [{:keys [socket input buffer bounds]} (as-reader source)
        buffer-size (alength buffer)
        latin1 java.nio.charset.StandardCharsets/ISO_8859_1
        newline "\n"
        limit (response-byte-limit)
        output (java.io.ByteArrayOutputStream.)]
    (loop []
      (when (and deadline (not (pos? (- deadline (System/nanoTime)))))
        (response-timeout! timeout nil))
      (let [start (aget bounds 0)
            end (aget bounds 1)]
        (if (< start end)
          (let [available (- end start)
                segment (String. buffer start available latin1)
                newline-offset (.indexOf segment newline)
                take-bytes (if (neg? newline-offset)
                             available
                             newline-offset)
                total (+ (.size output) take-bytes)]
            (when (> total limit)
              (throw
               (ex-info
                (str "coordinator response line exceeds " limit " bytes")
                {:type :coordinator-response-too-large
                 :max-bytes limit})))
            (.write output buffer start take-bytes)
            (if (neg? newline-offset)
              (do
                (aset-int bounds 0 end)
                (recur))
              (do
                (aset-int bounds 0 (+ start newline-offset 1))
                (finish-line! output))))
          (do
            (when deadline
              (arm-deadline! socket deadline timeout))
            (let [read-count
                  (try
                    (.read input buffer 0 buffer-size)
                    (catch java.net.SocketTimeoutException error
                      (response-timeout! timeout error)))]
              (cond
                (= -1 read-count)
                (if (and eof-ok? (zero? (.size output)))
                  nil
                  (throw
                   (ex-info
                    (if (zero? (.size output))
                      "coordinator closed before sending a response line"
                      "coordinator closed during a response line")
                    {:type (if (zero? (.size output))
                             :coordinator-response-closed
                             :coordinator-response-truncated)
                     :bytes (.size output)})))

                (zero? read-count)
                (recur)

                :else
                (do
                  (aset-int bounds 0 0)
                  (aset-int bounds 1 read-count)
                  (recur))))))))))

(defn read-line-bounded!
  "Read exactly one UTF-8 line through a persistent chunked reader.
   The deadline is absolute, so a peer cannot stay alive by dripping bytes just
   under SO_TIMEOUT. The byte cap excludes the line terminator."
  [source]
  (let [timeout (timeout-ms "NORTH_COORD_READ_TIMEOUT_MS" 30000)]
    (read-line-limited!
     source
     (+ (System/nanoTime) (* 1000000 (long timeout)))
     timeout
     false)))

(defn read-stream-line-bounded!
  "Read one event-stream line with no idle deadline but the same byte and UTF-8
   bounds as request responses. The persistent reader retains bytes following
   the newline for the next event. Clean EOF returns nil; partial EOF is invalid."
  [source]
  (let [reader (as-reader source)]
    (.setSoTimeout (:socket reader) 0)
    (read-line-limited! reader nil nil true)))

(defn- ensure-terminal-eof! [reader deadline timeout]
  (let [{:keys [socket input buffer bounds]} reader]
    (loop []
      (let [start (aget bounds 0)
            end (aget bounds 1)]
        (when (< start end)
          (throw
           (ex-info "coordinator sent more than one terminal response frame"
                    {:type :multiple-coordinator-response-frames
                     :surplus-bytes (- end start)})))
        (arm-deadline! socket deadline timeout)
        (let [read-count
              (try
                (.read input buffer 0 (alength buffer))
                (catch java.net.SocketTimeoutException error
                  (response-timeout! timeout error)))]
          (cond
            (= -1 read-count) nil
            (zero? read-count) (recur)
            :else
            (throw
             (ex-info "coordinator sent more than one terminal response frame"
                      {:type :multiple-coordinator-response-frames
                       :surplus-bytes read-count}))))))))

(defn- read-terminal-line! [reader]
  (let [timeout (timeout-ms "NORTH_COORD_READ_TIMEOUT_MS" 30000)
        deadline (+ (System/nanoTime) (* 1000000 (long timeout)))
        line (read-line-limited! reader deadline timeout false)]
    (ensure-terminal-eof! reader deadline timeout)
    line))

(defn- malformed-edn! [line error]
  (throw
   (ex-info "coordinator response line is not exactly one valid EDN form"
            {:type :malformed-coordinator-response
             :line-bytes (count (.getBytes
                                 (str line)
                                 java.nio.charset.StandardCharsets/UTF_8))}
            error)))

(defn parse-edn-line! [line]
  (try
    (with-open [reader
                (java.io.PushbackReader. (java.io.StringReader. line))]
      (let [eof (Object.)
            value (edn/read {:eof eof} reader)
            trailing (edn/read {:eof eof} reader)]
        (when (or (identical? eof value)
                  (not (identical? eof trailing)))
          (throw (ex-info "not exactly one EDN form" {})))
        value))
    ;; Hostile bounded input can still overflow a recursive parser. Normalize
    ;; that one Error, but let VM-fatal Errors propagate.
    (catch StackOverflowError error
      (malformed-edn! line error))
    (catch Exception error
      (malformed-edn! line error))))

(defn read-edn-response! [reader]
  (parse-edn-line! (read-terminal-line! reader)))

;; ---- optional JSON response decoding ---------------------------------------
;; EDN is this client's default and stays that way: coord.clj is deliberately
;; stdlib-only so hooks and sibling CLIs can load it without Fram's Cheshire
;; classpath, and that guarantee is load-bearing.
;;
;; But EDN parsing dominates whole-corpus reads. Measured 2026-07-29 on the same
;; ~345k-triple corpus: EDN 9,509-11,129 ms, JSON 2,978-4,013 ms — roughly 3x,
;; which is why fram.rt/coord-live-state already asks for `:fmt :json`.
;;
;; So: ASK for Cheshire rather than require it. Babashka bundles it, so in
;; practice this probe answers yes and the ns declaration stays free of it —
;; which is the point: coord.clj gains a fast path without gaining a dependency
;; that a non-bb loader would have to satisfy. The capability is resolved once;
;; requiring-resolve is not free and this sits on a hot path.
;;
;; The degradation that actually fires in the field is not a missing decoder but
;; a daemon that predates :fmt — see live-triples-at.
(def ^:private json-decoder
  (delay
    (try
      (when-let [parse (requiring-resolve 'cheshire.core/parse-string)]
        (fn [line] (parse line)))
      (catch Throwable _ nil))))

(defn json-response-available?
  "Whether this classpath can decode a JSON coordinator response."
  []
  (some? @json-decoder))

(defn read-json-response!
  "Read one terminal frame and decode it as JSON, reusing the same bounded
   reader as the EDN path so byte limits and deadlines are identical.

   Returns STRING-keyed data, matching the coordinator's JSON wire shape
   (`\"version\"`, `\"log\"`, `\"facts\"`) — callers normalise."
  [reader]
  (let [decode @json-decoder]
    (when-not decode
      (throw (ex-info "JSON coordinator decoding is unavailable on this classpath"
                      {:type :coordinator-json-unavailable})))
    (decode (read-terminal-line! reader))))

;; Every North request carries the exact corpus identity. The distinct :for-log
;; envelope is a protocol boundary, not optional metadata: a pre-fence daemon
;; rejects the unknown op, so a new North client can never silently fall back to
;; an unfenced read or write.
(defn canonical-log-path [log]
  (when-not (and (string? log) (not (str/blank? log)))
    (throw (ex-info "coordinator log identity must be a nonblank path"
                    {:type :invalid-log-identity :log log})))
  (.getCanonicalPath (io/file log)))

;; FRAM_LOG selects WHICH CORPUS; FRAM_TELEMETRY_LOG only says where telemetry
;; goes and must never veto that selection (bin/north 94643cc).
(defn expected-log []
  (let [explicit (System/getenv "FRAM_LOG")
        home (or (System/getenv "HOME") (System/getProperty "user.home"))
        requested (io/file
                   (or explicit
                       (str home "/.local/state/north/facts.log")))
        split (io/file (.getParentFile requested) "coordination.log")
        selected (if (and (nil? explicit) (.isFile split))
                   split
                   requested)]
    (.getCanonicalPath selected)))

(defn log-envelope-for [log op]
  (when (= :for-log (:op op))
    (throw (ex-info "nested coordinator log fences are not supported"
                    {:type :invalid-log-fence})))
  (cond-> {:op :for-log
           :expected-log (canonical-log-path log)
           :request op}
    (contains? op :fmt) (assoc :fmt (:fmt op))))

(defn log-envelope [op]
  (log-envelope-for (expected-log) op))

(defn validate-subscription! [line]
  (let [reply (when (string? line) (parse-edn-line! line))
        served (:log reply)
        valid-log? (and (string? served)
                        (= (expected-log)
                           (.getCanonicalPath (io/file served))))]
    (when-not (and (map? reply) (integer? (:subscribed reply)) valid-log?)
      (throw (ex-info
              (str "coordinator refused the fenced subscription: "
                   (if (nil? line) "connection closed before handshake" (pr-str reply)))
              {:type :invalid-subscription-handshake
               :expected-log (expected-log)
               :reply reply})))
    reply))

;; one fenced request/response over the daemon socket: write one EDN op +
;; newline, read one EDN reply line. The atom every other helper is built from.
(def ^:private max-request-line-bytes (* 1024 1024))

(defn- send-envelope
  "Send one request envelope and read exactly one terminal frame.

  `read-response!` is parameterised only so a caller can opt into the JSON
  decoder; it defaults to EDN, so every existing call site is byte-for-byte
  unchanged."
  ([port envelope] (send-envelope port envelope read-edn-response!))
  ([port envelope read-response!]
  (with-open [s (connect-socket port)]
    (let [payload (pr-str envelope)
          payload-bytes (.getBytes payload java.nio.charset.StandardCharsets/UTF_8)
          _ (when (> (alength payload-bytes) max-request-line-bytes)
              (throw
               (ex-info
                (str "coordinator request line exceeds "
                     max-request-line-bytes " bytes")
                {:type :coordinator-request-too-large
                 :max-bytes max-request-line-bytes})))
          ;; One write preserves the line-frame boundary for peers that answer
          ;; and close as soon as the complete request arrives.
          wire (.getBytes (str payload "\n")
                          java.nio.charset.StandardCharsets/UTF_8)
          w (.getOutputStream s)
          reader (coordinator-reader s)]
      (.write w wire)
      (.flush w)
      (read-response! reader)))))

(defn send-op [port op]
  (let [{:keys [port log]} (route-for-operation port op)]
    (send-envelope port (log-envelope-for log op))))

(defn send-op-for-log
  "Send OP fenced to LOG. The optional reader exists so the JSON fast path stays
   BEHIND this seam: callers and tests stub send-op-for-log to inject
   coordinator failures, and a format that reached send-envelope directly would
   slip past every one of them — which is exactly what it did, silently turning
   live-facts-view-detail-test from 11/11 into 5/11 by letting its injected
   failures through to a real socket."
  ([port log op] (send-envelope port (log-envelope-for log op)))
  ([port log op read-response!]
   (send-envelope port (log-envelope-for log op) read-response!)))

(defn- normalize-facts-response
  "Both wire formats carry the same value; only key TYPE differs — EDN answers
   with `:version`/`:facts`, JSON with `\"version\"`/`\"facts\"` (verified against
   the live daemon, which also renders `:code` keywords as strings). Normalising
   first keeps ONE validation below, so the two formats can never drift into
   accepting different things."
  [response]
  (when (map? response)
    {:version (or (:version response) (get response "version"))
     :facts (or (:facts response) (get response "facts"))}))

(defn- valid-triples
  "The triples, as vectors, or nil if the response is not a well-formed corpus.
   Coerces sequentials to vectors because a JSON decoder is free to hand back
   any sequential; the per-element contract (3 strings) stays exact."
  [triples]
  (when (sequential? triples)
    (let [rows (mapv #(if (vector? %) % (vec %)) triples)]
      (when (every? #(and (= 3 (count %)) (every? string? %)) rows)
        rows))))

(defn- fetch-triples
  "One :facts round trip in the requested wire format."
  [port log json?]
  (if json?
    (send-op-for-log port log {:op :facts :fmt :json} read-json-response!)
    (send-op-for-log port log {:op :facts})))

(defn- live-triples-at [port log]
  ;; Ask for JSON when this classpath can decode it. The whole-corpus :facts
  ;; response is the single largest thing North reads, and DECODING it — not
  ;; producing or transferring it — is the dominant cost. Fram's own
  ;; coord-live-state already opts in for exactly this reason
  ;; (coord_daemon.clj:5629, "~12x faster as JSON than as EDN" at ~2MB).
  ;;
  ;; Falls back to EDN on any JSON failure rather than reporting the domain
  ;; unavailable. A daemon predating :fmt support answers a REJECT, not a
  ;; corpus, and the correct response to "this coordinator is older than I
  ;; assumed" is to speak the older dialect — not to make north's whole live
  ;; view vanish. Version skew between an installed north and a running fram is
  ;; the normal state during a cutover, not an exceptional one.
  (let [attempt
        (fn [json?]
          (let [response (normalize-facts-response (fetch-triples port log json?))
                triples (valid-triples (:facts response))]
            (if (and triples (integer? (:version response)))
              {:available true :version (:version response) :facts triples}
              {:available false :error "malformed :facts response"})))]
    (try
      (if (json-response-available?)
        (let [json-result (try (attempt true)
                               (catch Exception error
                                 {:available false :error (.getMessage error)}))]
          (if (:available json-result)
            json-result
            ;; Keep the EDN answer's error if it also fails: it is the format
            ;; every coordinator speaks, so its complaint is the truer one.
            (attempt false)))
        (attempt false))
      (catch Exception error
        {:available false :error (.getMessage error)}))))

(defn live-facts-view
  "Compose materialized live facts from the independently fenced coordination
   and telemetry writers. This is a view union, never an event-log union: each
   origin resolves its own local transaction order before facts cross the seam.
   Exact duplicate triples collapse set-wise. Unavailable domains are named so
   callers never have to infer absence from an empty result."
  [coordination-port]
  ;; The two fetches are INDEPENDENT — different ports, different logs, different
  ;; writers — so running them in sequence just adds their latencies. Measured
  ;; 2026-07-29: coordination 4,132ms (345,679 facts) + telemetry 2,730ms
  ;; (237,328 facts) = 6,862ms sequential, against max() = 4,132ms in parallel.
  ;; ~2.7s off every verb in partitioned mode, with no semantic change: the
  ;; result is still a view union, each origin still resolves its own order
  ;; before crossing the seam, and both errors are still surfaced per-domain.
  ;;
  ;; The telemetry fetch is started FIRST so it overlaps the larger coordination
  ;; read rather than trailing it.
  (let [telemetry-future
        (when (telemetry-partition-enabled?)
          (future (live-triples-at (configured-telemetry-port) (telemetry-log-path))))
        coordination
        (live-triples-at coordination-port (expected-log))
        telemetry
        (when telemetry-future
          ;; Bounded: a hung domain must not hang the caller forever. On timeout
          ;; it reports unavailable with a reason, exactly like any other failure.
          (deref telemetry-future 120000
                 {:available false :error "telemetry domain fetch timed out"}))
        domains
        (cond-> {:coordination coordination}
          telemetry (assoc :telemetry telemetry))
        available (filter (comp :available val) domains)
        unavailable (->> domains
                         (remove (comp :available val))
                         (map (comp name key))
                         sort
                         vec)
        ;; The dedupe is LOAD-BEARING and cannot be dropped: fram's
        ;; kernel/build-index accumulates multi-valued predicates with
        ;; `(conj (get m kk []) r)` (kernel.bclj:321), so a duplicate triple
        ;; would surface as a repeated value in every by-predicate read — a
        ;; thread listing the same `touches` twice. It is, however, the single
        ;; most expensive step in composing the view: measured 2026-07-29 at
        ;; 1,662 ms over 590,496 rows.
        ;;
        ;; What it does NOT need to do is re-establish distinctness WITHIN a
        ;; domain. A coordinator's materialized live view is already a set: a
        ;; fact is identified by (subject predicate value), so asserting one
        ;; twice is idempotent and the fold emits it once. Verified on the live
        ;; corpus — `distinct` removed 0 of 350,150 coordination rows.
        ;;
        ;; So a single domain skips the pass entirely (1,741 ms -> 12 ms), and
        ;; the multi-domain union uses a transducer rather than the lazy seq
        ;; `distinct` returns, which alone was 1,662 ms -> 1,449 ms.
        domain-facts (mapv (comp :facts val) available)
        facts (if (< (count domain-facts) 2)
                (vec (first domain-facts))
                (into [] (distinct) (apply concat domain-facts)))]
    {:facts facts
     :domains domains
     :unavailable unavailable
     ;; WHY each domain is unavailable, not merely THAT it is. live-triples-at
     ;; already captures the exception into :error and this dropped it, so
     ;; callers could only report a domain NAME. On 2026-07-29 that discarded
     ;; string was "coordinator response line exceeds 8388608 bytes" — the whole
     ;; corpus outgrowing the response cap — and its absence turned a one-line
     ;; diagnosis into an hour of bisecting a write path that was never broken.
     ;; Additive: :unavailable keeps its shape for existing callers.
     :unavailable-detail (->> domains
                              (remove (comp :available val))
                              (map (fn [[domain result]]
                                     [(name domain)
                                      (or (:error result) "no reason recorded")]))
                              (sort-by first)
                              vec)
     :complete (empty? unavailable)}))

(defn indexed-query
  "Run one simple Fram indexed query with a bounded result set and an exclusive
   success envelope. A contradictory or malformed response never becomes an
   empty result: callers may treat the typed row-limit error as over-broad, but
   every other invalid envelope fails closed."
  [port query max-rows]
  (when-not (and (integer? max-rows)
                 (<= 1 max-rows query-page-row-limit))
    (throw
     (ex-info "indexed query row limit is outside the North/Fram contract"
              {:type :invalid-indexed-query-limit
               :limit max-rows
               :max query-page-row-limit})))
  (let [response
        (send-op port {:op :query
                       :query query
                       :query-max-rows max-rows
                       :query-max-response-bytes query-page-response-byte-limit})
        keys* (when (map? response) (set (keys response)))
        success?
        (and (= keys* #{:ok :version :engine})
             (vector? (:ok response))
             (<= (count (:ok response)) max-rows)
             (every? #(and (vector? %) (every? string? %)) (:ok response))
             (integer? (:version response))
             (not (neg? (:version response)))
             (= "index" (:engine response)))
        error-envelope?
        (and (map? response)
             (not (contains? response :ok))
             (vector? (:error response))
             (seq (:error response))
             (every? string? (:error response))
             (keyword? (:code response))
             (integer? (:version response))
             (not (neg? (:version response)))
             (= "index" (:engine response)))]
    (cond
      success? response

      (and error-envelope? (= :query-row-limit (:code response)))
      (throw
       (ex-info "indexed query exceeded its row bound"
                {:type :indexed-query-row-limit
                 :code (:code response)
                 :max-rows max-rows
                 :response response}))

      error-envelope?
      (throw
       (ex-info (str "coordinator returned indexed-query error: "
                     (name (:code response)))
                {:type :indexed-query-error
                 :code (:code response)
                 :response response}))

      :else
      (throw
       (ex-info "coordinator returned a malformed indexed-query response"
                {:type :malformed-indexed-query-response})))))

(defn indexed-query-in-domain
  "Run an indexed query against one named origin. Variable-subject telemetry
   queries cannot be routed from a subject token, so callers must state the
   telemetry domain. With Stage A disabled this is the prior coordination
   query, which keeps rollback flag-only."
  [port domain query max-rows]
  (when-not (#{:coordination :telemetry} domain)
    (throw (ex-info "unknown coordinator query domain"
                    {:type :invalid-query-domain :domain domain})))
  (binding [*operation-domain*
            (when (and (= :telemetry domain)
                       (telemetry-partition-enabled?))
              :telemetry)]
    (indexed-query port query max-rows)))

(defn query-page
  "Run Fram's internal deterministic query-page verb under its tighter 1 MiB
   client cap. There is no compatibility fallback: managed replay depends on
   pagination and fails closed against an older or malformed coordinator."
  [port query limit after]
  (when-not (and (integer? limit)
                 (<= 1 limit query-page-row-limit))
    (throw
     (ex-info "query page limit is outside the North/Fram contract"
              {:type :invalid-query-page-limit
               :limit limit
               :max query-page-row-limit})))
  (when-not (or (nil? after) (valid-query-page-cursor? after))
    (throw
     (ex-info "query page cursor is outside the North/Fram contract"
              {:type :invalid-query-page-cursor})))
  (binding [*response-byte-limit-override*
            query-page-response-byte-limit]
    (let [response
          (send-op port {:op :query-page
                         :query query
                         :limit limit
                         :after after})
          error? (boolean (and (map? response) (vector? (:error response))))
          page?
          (boolean
           (and (map? response)
               (vector? (:ok response))
               (<= (count (:ok response)) limit)
               (every?
                #(and (vector? %) (every? string? %))
                (:ok response))
               (boolean? (:more response))
               (if (:more response)
                 (valid-query-page-cursor? (:next response))
                 (nil? (:next response)))))]
      (when (= "unknown op" (:error response))
        (throw
         (ex-info "coordinator lacks required query-page protocol"
                  {:type :query-page-unsupported})))
      (when-not (and (integer? (:version response))
                     (not (neg? (:version response)))
                     (= "scan" (:engine response))
                     (not= error? page?))
        (throw
         (ex-info "coordinator returned a malformed query page"
                  {:type :malformed-query-page-response})))
      response)))

(defn query-page-in-domain
  "Run a deterministic query page against one named origin. See
   indexed-query-in-domain for why variable-subject telemetry reads declare
   their domain explicitly."
  [port domain query limit after]
  (when-not (#{:coordination :telemetry} domain)
    (throw (ex-info "unknown coordinator query domain"
                    {:type :invalid-query-domain :domain domain})))
  (binding [*operation-domain*
            (when (and (= :telemetry domain)
                       (telemetry-partition-enabled?))
              :telemetry)]
    (query-page port query limit after)))

(defn send-raw-op
  "Low-level compatibility/policy probe. Managed North operations must use
   send-op/send-op-for-log; this exists only to prove that a daemon rejects an
   unfenced request before north-coord-up declares it strict-ready."
  [port op]
  (send-envelope port op))

(defn strict-coordinator-status [port log]
  (let [expected (canonical-log-path log)]
    (try
      (let [fenced (send-op-for-log port expected {:op :version})
            raw (send-raw-op port {:op :version})
            served (:served-log raw)
            served-canonical
            (when (and (string? served) (not (str/blank? served)))
              (canonical-log-path served))]
        (cond
          (not (integer? (:version fenced)))
          {:ready false :reason :fenced-version-invalid}

          (not= :log-fence-required (:code raw))
          {:ready false :reason :raw-request-not-rejected}

          (not= expected served-canonical)
          {:ready false :reason :strict-probe-served-wrong-log
           :expected-log expected :served-log served-canonical}

          :else
          {:ready true :version (:version fenced) :log expected}))
      ;; read-edn-response! already normalizes parser StackOverflowError into an
      ;; Exception. Preserve ordinary probe diagnostics without swallowing
      ;; unrelated VM-fatal Errors.
      (catch Exception error
        {:ready false
         :reason :probe-failed
         :error (.getMessage error)}))))

;; the daemon's current global version (only swap!/retract! read it now — the base).
(defn cur-ver [port] (:version (send-op port {:op :version})))

(defn cur-ver-for-subject [port subject]
  (let [{target-port :port target-log :log}
        (route-for-operation port {:op :resolved :te subject :p "kind"})]
    (:version (send-op-for-log target-port target-log {:op :version}))))

;; A Fact is a string subject/predicate/object triple. Blank literal objects are
;; intentional in a few contracts (empty message bodies and DONE payloads), so
;; preserve explicit ""; nil is different — `(str nil)` used to turn an omitted
;; CLI argument into a blank fact. Reject malformed shapes before any socket write.
(defn- write-value! [te p r]
  (when-not (and (string? te) (not (str/blank? te)))
    (throw (ex-info "coord write requires a nonblank string subject"
                    {:type :invalid-write :field :subject})))
  (when-not (and (string? p) (not (str/blank? p)))
    (throw (ex-info "coord write requires a nonblank string predicate"
                    {:type :invalid-write :field :predicate})))
  (when (nil? r)
    (throw (ex-info "coord write requires a non-nil object; pass \"\" explicitly when blank is intended"
                    {:type :invalid-write :field :object})))
  (str r))

;; append! — MULTI cardinality: one wire op, NO base, NO retry. The engine appends
;; (rival/disjoint values coexist; an identical (te,p,r) is idempotent). The safe
;; coexist default. (str r) coerces defensively (callers already pass strings).
(defn append! [port te p r]
  (send-op port {:op :assert :te te :p p :r (write-value! te p r)}))

;; put! — SINGLE last-writer-wins: one wire op, NO base. For a pred the engine has
;; declared single this SUPERSEDES the prior live value (LWW). Wire-identical to
;; append!; the cardinality FACT (engine-side) — not this verb — decides append-vs-
;; supersede, so the verb names the call site's INTENT. A no-base write is never
;; staleness-rejected (base-optional engine), which IS the LWW contract.
(defn put! [port te p r]
  (send-op port {:op :assert :te te :p p :r (write-value! te p r)}))

;; Lease-fenced variants — the coordinator validates RES/HOLDER/EPOCH and
;; performs the fact mutation under its one writer lock. A separate `fence-ok`
;; preflight is not an authority boundary: expiry/takeover could land between
;; two socket turns. These verbs close that window.
(defn put-with-fence! [port {:keys [resource holder epoch]} te p r]
  (send-op port {:op :assert-with-fence
                 :res resource :holder holder :epoch epoch
                 :te te :p p :r (write-value! te p r)}))

(defn retract-with-fence! [port {:keys [resource holder epoch]} te p r]
  (send-op port {:op :retract-with-fence
                 :res resource :holder holder :epoch epoch
                 :te te :p p :r (write-value! te p r)}))

;; swap! — SINGLE compare-and-swap: the ONLY base+retry verb. Reads the base, writes
;; under it, retries on :reject (a concurrent write moved the base). Reserve for a
;; genuine read-modify-write race; near-zero production callers after move-C. 4 tries.
(defn swap! [port te p r]
  (let [rv (write-value! te p r)]
    (loop [tries 4]
      (let [res (send-op port {:op :assert :te te :p p :r rv
                               :base (cur-ver-for-subject port te)})]
        (if (and (:reject res) (pos? tries)) (recur (dec tries)) res)))))

;; assert-after-read! — commit one marker against the exact GLOBAL graph version
;; a caller validated. The callback MUST perform every load-bearing read after
;; BASE is captured. :assert-at-version performs its comparison + assert in one
;; serialized Fram coordinator turn; ordinary :assert's :base is only
;; cardinality-local OCC and MUST NOT be substituted here. A concurrent graph
;; write makes the marker assert reject; retry therefore re-runs the callback
;; over a fresh graph instead of blessing a stale read. This is intentionally
;; global-version conservative: unrelated traffic may cause a retry, but can
;; never create a false successful commit. Contention retries use a monotonic
;; deadline plus equal-jitter exponential backoff: a fixed tight attempt count
;; can starve behind unrelated telemetry, while an unbounded loop can hide a
;; permanently moving graph. The explicit ATTEMPTS arity remains a smaller
;; caller-selected cap when needed. Callers publishing prerequisite facts may
;; create one deadline and pass it through the seven-argument arity so body +
;; marker share the same finite window.
;; 30s: the 5s default starved reservation publication behind telemetry
;; write bursts at ~300k-version store scale (2026-07-28 preflight failures:
;; "delivery evidence publication deadline exceeded"), same growth arithmetic
;; as the coordinator query budget (30s) and SDK read timeout (45s).
(def assert-after-read-deadline-ms 30000)
(def ^:private assert-after-read-initial-backoff-ms 1)
(def ^:private assert-after-read-max-backoff-ms 64)

(def ^:dynamic *retry-monotonic-now-ns*
  "Injectable monotonic clock for deterministic deadline tests."
  (fn [] (System/nanoTime)))

(def ^:dynamic *retry-sleep-ms!*
  "Injectable sleeper for deterministic deadline tests."
  (fn [milliseconds] (Thread/sleep (long milliseconds))))

(def ^:dynamic *retry-jitter-ms*
  "Injectable inclusive random selection inside the equal-jitter bounds."
  (fn [floor-ms cap-ms]
    (.nextLong (java.util.concurrent.ThreadLocalRandom/current)
               (long floor-ms) (inc (long cap-ms)))))

(defn retry-deadline-ns
  "Create an absolute monotonic deadline. TIMEOUT-MS exists for bounded tests;
   production callers use the zero-argument reservation window."
  ([] (retry-deadline-ns assert-after-read-deadline-ms))
  ([timeout-ms]
   (when-not (and (integer? timeout-ms) (pos? timeout-ms))
     (throw (ex-info "retry deadline requires a positive integer timeout"
                     {:timeout-ms timeout-ms})))
   (+ (long (*retry-monotonic-now-ns*))
      (* (long timeout-ms) 1000000))))

(defn- retry-delay-ms [backoff-ms remaining-ms]
  (let [cap-ms (long (min backoff-ms remaining-ms))
        floor-ms (long (max 1 (quot (inc cap-ms) 2)))
        delay-ms (*retry-jitter-ms* floor-ms cap-ms)]
    (when-not (and (integer? delay-ms)
                   (<= floor-ms delay-ms cap-ms))
      (throw
       (ex-info "retry jitter must stay inside the requested inclusive bounds"
                {:floor-ms floor-ms :cap-ms cap-ms :delay-ms delay-ms})))
    (long delay-ms)))

(defn retry-conflicts-until!
  "Run OPERATION! while it returns {:reject :conflict}. Retries stop at the
   absolute monotonic DEADLINE-NS or the optional ATTEMPTS cap. A deadline hit
   preserves :reject :conflict when one was observed and adds :deadline true."
  ([deadline-ns operation!]
   (retry-conflicts-until! deadline-ns Integer/MAX_VALUE operation!))
  ([deadline-ns attempts operation!]
   (when-not (integer? deadline-ns)
     (throw (ex-info "retry deadline must be an absolute integer nanosecond value"
                     {:deadline-ns deadline-ns})))
   (when-not (pos? attempts)
     (throw (ex-info "conflict retry requires at least one attempt"
                     {:attempts attempts})))
   (loop [remaining attempts
          backoff-ms assert-after-read-initial-backoff-ms]
     (if-not (< (long (*retry-monotonic-now-ns*)) deadline-ns)
       {:reject :deadline}
       (let [result (operation!)]
         (if (and (= :conflict (:reject result)) (> remaining 1))
           (let [remaining-ns (- deadline-ns
                                 (long (*retry-monotonic-now-ns*)))
                 remaining-ms (quot remaining-ns 1000000)]
             (if (pos? remaining-ms)
               (do
                 (*retry-sleep-ms!*
                  (retry-delay-ms backoff-ms remaining-ms))
                 (if (< (long (*retry-monotonic-now-ns*)) deadline-ns)
                   (recur (dec remaining)
                          (min assert-after-read-max-backoff-ms
                               (* 2 backoff-ms)))
                   (assoc result :deadline true)))
               (assoc result :deadline true)))
           result))))))

(defn assert-after-read!
  ([port te p r validate!]
   (assert-after-read! port te p r validate! Integer/MAX_VALUE))
  ([port te p r validate! attempts]
   (assert-after-read! port te p r validate! attempts (retry-deadline-ns)))
  ([port te p r validate! attempts deadline-ns]
   (when-not (pos? attempts)
     (throw (ex-info "assert-after-read! requires at least one attempt"
                     {:attempts attempts})))
   (let [rv (write-value! te p r)]
     (retry-conflicts-until!
     deadline-ns attempts
     (fn []
        (let [base (cur-ver-for-subject port te)
              _ (validate!)]
          (send-op port {:op :assert-at-version
                         :te te :p p :r rv :base base})))))))

;; assert-batch-after-read! — the ALL-OR-NOTHING form of assert-after-read!.
;; One :assert-batch-at-version turn commits every planned fact or none of them
;; against the exact global version PLAN! validated, so a multi-fact publication
;; has no observable partial state. The shape it replaces — append the body
;; facts, then CAS a marker over them — cannot offer that: the body lands under
;; no global base at all, so a losing marker race, a changed read set, or an
;; exhausted deadline leaves a bodied-but-unmarked subject that every reader
;; must then treat as tampered.
;;
;; PLAN! runs AFTER the base is captured, performs every load-bearing read
;; itself, and returns either {:facts [{:p _ :r _} ...]} to publish or
;; {:done value} to finish this turn without writing (the idempotent-replay
;; case); throwing refuses outright. Note that ordinary :assert-batch is NOT a
;; substitute: its :base is per-(subject,predicate) OCC, while this op compares
;; the coordinator's global head inside the same serialized writer turn as the
;; commit. A fact-local :base is rejected by the daemon on purpose — the one
;; top-level base is the whole read set's guard.
(defn assert-batch-after-read!
  ([port te plan!]
   (assert-batch-after-read! port te plan! Integer/MAX_VALUE (retry-deadline-ns)))
  ([port te plan! attempts deadline-ns]
   (when-not (pos? attempts)
     (throw (ex-info "assert-batch-after-read! requires at least one attempt"
                     {:attempts attempts})))
   (retry-conflicts-until!
    deadline-ns attempts
    (fn []
      (let [base (cur-ver-for-subject port te)
            planned (plan!)]
        (if (contains? planned :done)
          planned
          (let [facts (:facts planned)]
            (when-not (and (sequential? facts) (seq facts))
              (throw (ex-info "assert-batch-after-read! requires a non-empty planned batch"
                              {:te te})))
            (send-op port {:op :assert-batch-at-version
                           :te te
                           :facts (mapv (fn [{:keys [p r]}]
                                          {:p p :r (write-value! te p r)})
                                        facts)
                           :base base}))))))))

(defn assert-after-read-with-fence!
  "Global read-set CAS plus an atomic lease fence. Every load-bearing read in
  VALIDATE! follows BASE capture; the daemon checks both BASE and the current
  lease epoch in the same writer turn as the marker assertion."
  ([port lease te p r validate!]
   (assert-after-read-with-fence! port lease te p r validate! 16))
  ([port {:keys [resource holder epoch]} te p r validate! attempts]
   (when-not (pos? attempts)
     (throw
      (ex-info "assert-after-read-with-fence! requires at least one attempt"
               {:attempts attempts})))
   (let [rv (write-value! te p r)]
     (loop [remaining attempts]
       (let [base (cur-ver-for-subject port te)
             _ (validate!)
             result
             (send-op port {:op :assert-at-version-with-fence
                            :res resource :holder holder :epoch epoch
                            :te te :p p :r rv :base base})]
         (if (and (= :conflict (:reject result)) (> remaining 1))
           (recur (dec remaining))
           result))))))

;; thin migration alias — old assert! WAS the swap! CAS ritual; keep it pointing
;; there so any un-migrated caller is byte-for-byte unchanged.
(def assert! swap!)

(defn retract! [port te p r]
  (let [rv (write-value! te p r)]
    (loop [tries 4]
      (let [res (send-op port {:op :retract :te te :p p :r rv
                               :base (cur-ver-for-subject port te)})]
        (if (and (:reject res) (pos? tries)) (recur (dec tries)) res)))))

;; The :resolved op's exclusive-success envelope. Same discipline indexed-query
;; (line ~386) documents for :query and reserve-link's values-of enforces for
;; this exact op: a coordinator ERROR MAP, a malformed envelope, or a
;; self-contradictory response NEVER degrades to nil/empty — it fails closed with
;; a typed cause so a caller can tell an absent value from an unreadable one.
;; This is the false-empty generator the read layer was built on; validating it
;; here is what makes resolved/many honest for every downstream read surface.
(defn resolved-envelope
  "Send one :resolved op and return the validated success envelope
   {:value :members :ambiguous? :values :version}, or THROW
   :malformed-resolved-response. Never returns an error map."
  [port te p]
  (let [response (send-op port {:op :resolved :te te :p p})
        success?
        (and (map? response)
             (= #{:value :members :ambiguous? :values :version}
                (set (keys response)))
             (integer? (:version response))
             (not (neg? (:version response)))
             (integer? (:members response))
             (not (neg? (:members response)))
             (boolean? (:ambiguous? response))
             (vector? (:values response))
             (every? string? (:values response))
             (= (:members response) (count (:values response)))
             (= (:members response) (count (set (:values response))))
             (= (:ambiguous? response) (> (:members response) 1))
             (or (nil? (:value response)) (string? (:value response)))
             (if (zero? (:members response))
               (nil? (:value response))
               (boolean (some #{(:value response)} (:values response)))))]
    (if success?
      response
      (throw
       (ex-info "coordinator returned a malformed resolved response"
                {:type :malformed-resolved-response :te te :p p})))))

;; single live value of (te,p)  (the resolved/one/rf variants collapse here).
(defn resolved [port te p] (:value (resolved-envelope port te p)))
;; all live values of (te,p) — multi-valued  (the many/rmany variants).
(defn many     [port te p] (:values (resolved-envelope port te p)))

;; The :show op is the exact-subject projection used by the daemon-first CLI.
;; Validate its complete envelope here so existence gates can reuse the same
;; authoritative read without turning a wire failure into an absent thread.
(defn show-envelope
  [port te]
  (let [response (send-op port {:op :show :te te})
        success?
        (and (map? response)
             (= #{:version :rows} (set (keys response)))
             (integer? (:version response))
             (not (neg? (:version response)))
             (vector? (:rows response))
             (every?
              (fn [row]
                (and (vector? row)
                     (= 2 (count row))
                     (string? (nth row 0))
                     (string? (nth row 1))))
              (:rows response)))]
    (if success?
      response
      (throw
       (ex-info "coordinator returned a malformed show response"
                {:type :malformed-show-response :te te})))))

(defn show-rows [port te] (:rows (show-envelope port te)))

;; --- presence liveness: the renewable-LEASE rule (presence-cli #30 is the origin) ---
;; A session's liveness is a lease fact @lease:session:<h> = "holder|exp|epoch"; the
;; agent is ONLINE iff that lease's exp is still in the FUTURE by the coordinator's clock
;; (never a self-stamped heartbeat — a crashed agent's lease simply lapses). Factored here
;; so the presence roster (presence-cli) and any consumer that must judge liveness — e.g.
;; concern-cli hiding a lapsed agent's stale concerns — share ONE definition and cannot
;; drift on what "online" means. That single-definition guarantee is this file's whole job.
(def lease-max-safe-integer 9007199254740991)

(defn decode-lease
  "Decode exactly one canonical holder|expiry|epoch lease. Invalid or
   non-interoperable wire integers are not liveness authority."
  [v]
  (when (string? v)
    (let [parts (str/split v #"\|" -1)
          [holder expiry-text epoch-text] parts
          expiry (when (and (= 3 (count parts))
                            (re-matches #"[0-9]+" expiry-text))
                   (parse-long expiry-text))
          epoch (when (and (= 3 (count parts))
                           (re-matches #"[0-9]+" epoch-text))
                  (parse-long epoch-text))]
      (when (and (= 3 (count parts))
                 (not (str/blank? holder))
                 (some? expiry)
                 (some? epoch)
                 (<= expiry lease-max-safe-integer)
                 (<= epoch lease-max-safe-integer))
        {:holder holder :exp expiry :epoch epoch}))))

(defn authoritative-lease?
  "True when a decoded lease can represent an acquired Fram lease. Epoch zero
   remains parseable for historical diagnostics but never confers authority."
  [lease]
  (boolean
   (and (map? lease)
        (string? (:holder lease))
        (not (str/blank? (:holder lease)))
        (integer? (:exp lease))
        (<= 0 (:exp lease) lease-max-safe-integer)
        (integer? (:epoch lease))
        (<= 1 (:epoch lease) lease-max-safe-integer))))

(defn lease-of [port res] (decode-lease (resolved port (str "@lease:" res) "lease")))

(defn online?
  "True iff session <handle> holds an unexpired lease. `now` defaults to the system clock
   (agent runs on the coordinator's machine, so agent-now ~ coord-now)."
  ([port handle] (online? port handle (System/currentTimeMillis)))
  ([port handle now]
   (let [l (lease-of port (str "session:" handle))]
     (boolean
      (and (authoritative-lease? l)
           (= handle (:holder l))
           (> (:exp l) now))))))

;; ============================================================================
;; INCREMENTAL AGGREGATE — the completion DUAL of mutual exclusion.
;;
;; Roadmap tier F, decision 6: "EVERYTHING COUNTABLE IS A
;; FOLD OVER AN APPEND-ONLY LOG, NEVER A MUTATED CELL." Where mutual exclusion
;; REJECTS the second writer, completion ACCEPTS every writer and DERIVES the
;; answer by folding the log at READ time — so the completion half of
;; coordination needs no write-time convergence at all.
;;
;; ONE primitive, two reducers for common aggregation shapes:
;;   quorum = count-distinct(worker) >= K   — north-map's K-of-N barrier
;;   usage  = Σ(measurement)               — telemetry and experiment totals
;; Both fold a reducer over the rows a Datalog BODY binds against the
;; scan engine. Both are commutative and idempotent (set semantics collapse a
;; double-reported worker; Σ rides write-once @charge/@run subjects), so retry,
;; double-report, and racing writers all converge with ZERO coordination. Each
;; fold is a pure, recomputable function of the log prefix — never a cached cell
;; that can silently diverge from its own source. The total order earliest-cid
;; that makes other derivations agree is not even needed here: + and set-union
;; are order-independent.

;; A REDUCER is {:init :step :final}: fold :step from :init over the rows, finalize.
;; The two production reducers — the only two coordination has ever needed:
(def distinct-reducer
  "Quorum reducer: union each row's first binding into a SET (a key seen twice
   counts once). Returns the set itself — callers count it or diff it for the
   missing members."
  {:init #{} :step (fn [s row] (conj s (first row))) :final identity})

(def sum-reducer
  "Sum reducer: Σ the numeric SECOND projection of each row (non-numeric -> 0).
   Rows MUST carry a distinct key in the FIRST position (the @run/@charge subject):
   the engine's derived head is a SET of tuples, so a value-only projection would
   collapse two equal-valued addends and UNDER-count. The key
   keeps equal values distinct — the exact dual of count-distinct, which WANTS the
   collapse. This asymmetry is why Σ projects [key val] and count-distinct [key]."
  {:init 0 :step (fn [n row] (+ n (or (parse-double (str (second row))) 0))) :final identity})

;; A general :query row read that fails closed the way indexed-query does: an
;; error map or a response whose :ok is not a row vector THROWS rather than
;; plucking nil and passing an empty fold downstream. This is the same
;; false-empty class as resolved/many — a coordinator error map must never look
;; like "zero rows" to a projection (the `concern ls` blank, the roster gaps).
;; A genuinely empty result stays an honest [] (:ok is still a vector).
(defn query-rows [port query]
  (let [response (send-op port {:op :query :query query})]
    (if (and (map? response)
             (not (contains? response :error))
             (vector? (:ok response)))
      (:ok response)
      (throw
       (ex-info "coordinator returned a malformed query response"
                {:type :malformed-query-response})))))

;; The rows a Datalog BODY binds, projected onto PROJECT (the head vars). One
;; scan-engine query; a 1- or 2-literal body routes to the join engine (q/run).
(defn agg-rows [port project body]
  (query-rows port {:find "agg"
                    :rules [{:head {:rel "agg" :args (mapv (fn [v] {:var v}) project)}
                             :body body}]}))

;; Apply a REDUCER to a row-seq you already hold. The seam for callers that must
;; scope rows with a predicate the scan body can't express (e.g. an entity-id
;; PREFIX like "@run:") — they fold the pre-filtered rows through the SAME reducer,
;; so every caller uses the same numeric reducer.
(defn reduce-rows [{:keys [init step final]} rows] (final (reduce step init rows)))
(defn sum-rows      [rows] (reduce-rows sum-reducer rows))       ; Σ the [key val] rows
(defn distinct-rows [rows] (reduce-rows distinct-reducer rows))  ; SET of the [key] rows

;; THE primitive. Pure read; recomputable from the log.
(defn aggregate [port project body reducer]
  (reduce-rows reducer (agg-rows port project body)))

;; --- named folds (each reducer, applied) ------------------------------------
(defn distinct-of
  "The SET of distinct PROJECT values BODY binds (count-distinct, set form)."
  [port project body] (aggregate port project body distinct-reducer))
(defn count-distinct
  "K-of-N quorum's left side: how many DISTINCT keys BODY binds."
  [port project body] (count (distinct-of port project body)))
(defn sum-of
  "Σ of a numeric projection over BODY. PROJECT must be [key-var val-var]: the key
   (the @run/@charge subject) keeps equal values distinct so they are not deduped
   away; the val is summed."
  [port project body] (aggregate port project body sum-reducer))

;; --- gates (a gate is just a threshold predicate over a fold) ---------------
(defn quorum-met?
  "True once ≥ K distinct keys have appeared — the barrier has FIRED. Monotone:
   never un-fires while completion predicates are irretractable (see roadmap F↔H)."
  [port k project body] (>= (count-distinct port project body) k))

;; ============================================================================
;; COMMAND-AS-FACTS — the pending-command rule (single source).
;;
;; Roadmap tier I: a command is NOT an opaque {:op :args} body blob with a
;; parse-envelope parser duplicated across msg-cli + north-listen. It is FACTS
;; on @cmd:<id> — `op` + `target` (the routing handle) + one fact per arg. PENDING
;; = has op+target, NO acked_by and NO failed_by. Success and failure are distinct
;; terminal states; an explicit `msg-cli retry` retracts failure and emits a
;; retry_requested edge rather than pretending a failed execution was acknowledged.
;; match BOTH the sender's `cmds` listing and the reactor drive off; it lives ONCE
;; here so the duplication this redesign deletes can never reappear as a copied query.
(defn pending-cmds
  "[[cmd op target] …] for every command carrying op+target and no terminal result.
   Stratum 0 binds `settled` from either success ack or failed_by; stratum 1 selects
   op+target where the command is NOT settled (the negated var is bound by the positive op literal — the engine's stratified-
   negation safety rule)."
  [port]
  (:ok (send-op port {:op :query
                      :query {:find "pending"
                              :strata [[{:head {:rel "settled" :args [{:var "c"}]}
                                         :body [{:rel "triple" :args [{:var "c"} "acked_by" {:var "a"}]}]}
                                        {:head {:rel "settled" :args [{:var "c"}]}
                                         :body [{:rel "triple" :args [{:var "c"} "failed_by" {:var "a"}]}]}]
                                       [{:head {:rel "pending" :args [{:var "c"} {:var "op"} {:var "t"}]}
                                         :body [{:rel "triple" :args [{:var "c"} "op" {:var "op"}]}
                                                {:rel "triple" :args [{:var "c"} "target" {:var "t"}]}
                                                {:rel "settled" :args [{:var "c"}] :neg true}]}]]}})))

(defn -main [& args]
  (if (= "strict-probe" (first args))
    (let [port (Integer/parseInt (or (second args) PORT))
          log (nth args 2 nil)
          status (strict-coordinator-status port log)]
      (prn status)
      (when-not (:ready status) (System/exit 1)))
    (let [port (Integer/parseInt (or (first args) PORT))]
      (prn (send-op port {:op :version})))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
