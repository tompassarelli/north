;; north-mine.clj <port> [--dry-run] [--full] [--root DIR] [--report FILE] [--limit N] [--verbose]
;;
;; Cognitive telemetry miner (thread 019f2036-3ca4): the stack learns from its own
;; operating record. Streams Claude Code session transcripts (~/.claude/projects/**/*.jsonl)
;; and extracts per-session STRUGGLE SIGNALS, then appends them AS FACTS through the
;; coordinator — the mining dual of sdk/src/telemetry.ts (which records run tuples) and
;; sdk/src/struggle.ts (which scores a LIVE run; same fingerprint scheme, applied post-hoc).
;;
;; The key reframe (program thread 019f200f-46f6): an agent's WRONG GUESSES are
;; VOCABULARY VOTES — when it calls a tool that doesn't exist, the name it reached for
;; is empirical evidence of the right surface. Signals mined per transcript unit:
;;   verb votes    — "No such tool available: X" tool_result errors -> attempted name
;;   input errors  — InputValidationError per tool (deferred-tool + param friction)
;;   retry loops   — same tool+input fingerprint failing >=3 times w/o a success
;;   guard denials — graph-upstream / racket-build / firn guard + hook blocks
;;   engine rejects— coordinator "reject:<reason>" replies (e.g. reserved predicate)
;;   doc re-reads  — the same .md Read from the top >=3 times in one session
;;
;; VERBOSITY ADVISORY (thread 019f7d16 W2). A second, disjoint signal family reads
;; the human/assistant DIALOGUE shape — never content — to weigh whether responses
;; run long enough that humans push back:
;;   response length      — per assistant text turn: chars, and counts over the
;;                          long / very-long thresholds (a length DISTRIBUTION, not text)
;;   interruption/correct — "[Request interrupted by user]" turns, ATTRIBUTED to the
;;                          preceding output: corrective (cut off a long text turn),
;;                          during-tool (cut off a tool call), or other
;;   fast-skip / abandon  — an interrupt arriving within FAST-SKIP-MS of the output
;;                          (bailed before reading), or a session left on an interrupt
;; These per-session counts feed `advisory-of` — a PURE aggregate over the scanned
;; corpus that emits an emit-only verdict on "@advisory:verbosity"
;; (insufficient-evidence | within-norms | verbose-tendency). Below the minimum
;; sample (MIN-ADVISORY-RESPONSES / MIN-ADVISORY-SESSIONS) the verdict is always
;; insufficient-evidence — a small corpus never yields a recommendation. This is
;; ADVICE ONLY: it writes facts on @advisory:*/@mine:* subjects and NEVER touches a
;; prompt, posture, model, provider, or any Gaffer routing predicate.
;;
;; Facts land on a titleless "@mine:<transcript-stem>" subject (the @run:* pattern —
;; queryable via fram, invisible to the work views). Predicate vocabulary is kept SMALL,
;; reusing kind/session_id/repo/at/error_count/note; the one minted predicate is
;; `verb_vote` (multi, one fact per attempted name per session — a session votes once).
;;
;; IDEMPOTENT by construction: subjects are deterministic, objects are deterministic
;; (counts inside notes are BUCKETED so a still-growing session doesn't mint rivals),
;; and append! collapses identical (te,p,r) — re-running never duplicates. Incremental
;; state (path -> mtime/size) lives in ~/.local/state/north/north-mine/state.edn so the
;; steady-state run only touches changed files; --full rescans everything.
;;
;; PRIVACY: signals, not surveillance — only short verb/tool/doc names and truncated
;; error reasons are recorded, never message content.
;;
;; All writes go through the coordinator socket (cli/coord.clj append!/put!) — never
;; the facts.log directly. Streaming line-reader; snapshot lines and >8MB lines are
;; skipped unparsed; per-file line cap keeps a pathological transcript bounded.
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[clojure.edn :as edn]
         '[cheshire.core :as json])

;; shared coord substrate (Foundation Part B): send-op/append!/put! live once in cli/coord.clj.
;; Guard the sibling load so a test can pre-load coord.clj (from its own dir) and then
;; load THIS file as a library without the babashka.file parent resolving to the wrong dir.
(when-not (find-ns 'north.coord)
  (load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj")))

(def ^:const MAX-LINE-BYTES (* 8 1024 1024))
(def ^:const MAX-LINES-PER-FILE 400000)
(def ^:const MAX-VOTES-PER-UNIT 12)
(def ^:const MAX-NOTES-PER-UNIT 30)
(def ^:const RETRY-THRESHOLD 3)
(def ^:const REREAD-THRESHOLD 3)

;; --- verbosity advisory tunables (thread 019f7d16 W2) ----------------------
;; Char thresholds over an assistant turn's USER-FACING text (text blocks only;
;; thinking/tool_use excluded). ~4 chars/token, so 2000c ≈ 500 tokens is a "long"
;; turn, 6000c ≈ 1500 tokens a "very long" one.
(def ^:const LONG-RESPONSE-CHARS 2000)
(def ^:const VERY-LONG-RESPONSE-CHARS 6000)
;; An interrupt arriving within this window of the output it cut off = the human
;; bailed before reading (a fast-skip), not a considered correction.
(def ^:const FAST-SKIP-MS 4000)
;; Below either floor the advisory refuses to recommend — insufficient-evidence.
(def ^:const MIN-ADVISORY-RESPONSES 30)
(def ^:const MIN-ADVISORY-SESSIONS 3)
;; Advisory trip points: a verbose-tendency verdict needs BOTH long outputs AND
;; human push-back (corrective interrupts or fast-skips) — one alone is within-norms.
(def ^:const LONG-RATE-ADVISORY 0.25)
(def ^:const PUSHBACK-RATE-ADVISORY 0.05)

(def home (System/getProperty "user.home"))
(defn tilde [p] (if (and p (str/starts-with? p home)) (str "~" (subs p (count home))) p))

;; ---------------------------------------------------------------------------
;; content flattening — tool_result content is a string OR [{:type "text" :text ..}]
(defn content-text [c]
  (cond (string? c) c
        (sequential? c) (str/join "\n" (keep #(when (map? %) (:text %)) c))
        :else ""))

;; ---------------------------------------------------------------------------
;; signal matchers (structural: applied only to tool_result / hook-error fields,
;; never raw transcript lines — a grep dump QUOTING these strings can't false-positive
;; because quoted dumps ride non-error results and fail the shape guards below)
(defn unknown-tool [s]
  (when-let [[_ n] (re-find #"No such tool available: ([A-Za-z0-9_.:-]+)" s)]
    (str/replace n #"\.+$" "")))

(defn engine-reject [s]
  ;; a real coordinator reject reply is SHORT; long contents are code/doc reads.
  (let [t (str/trim s)]
    (when (and (<= (count t) 500) (str/includes? t "reject:"))
      (some-> (re-find #"reject:([^\n\"]{1,60})" t) second str/trim))))

(defn guard-label [s]
  (let [t (str/triml s)]
    (cond
      (str/includes? t "This file is GRAPH-OWNED")               "graph-upstream-guard"
      (str/includes? t "Racket version mismatch")                 "racket-build-guard"
      (str/includes? t "Stale bytecode")                          "racket-build-guard"
      (str/starts-with? t "BLOCKED:")                             "firn-guard"
      (str/includes? t "operation blocked by hook")               "hook-block"
      (str/starts-with? t "Permission for this action was denied by the Claude Code auto mode classifier")
      "auto-classifier"
      :else nil)))

;; same shape as struggle.ts fingerprint(): name + head of the canonical input
(defn fingerprint [nm input]
  (str nm "|" (let [s (try (json/generate-string input) (catch Exception _ (str input)))]
                (subs s 0 (min 200 (count s))))))

;; short human label for a retry-loop episode — verb + hint, no content
(defn tool-hint [nm input]
  (case nm
    "Bash" (let [c (str (:command input))]
             (subs c 0 (min 60 (count (first (str/split-lines c))))))
    ("Read" "Edit" "Write") (some-> (:file_path input) io/file .getName)
    (some->> (vals (select-keys input [:skill :query :pattern :prompt]))
             (remove nil?) first str (#(subs % 0 (min 40 (count %)))))))

(defn bucket [n] (cond (>= n 20) "20+" (>= n 10) "10+" (>= n 5) "5+" :else (str n)))

;; ---------------------------------------------------------------------------
;; verbosity signals (thread 019f7d16 W2) — dialogue SHAPE, never content.
;; user-facing text length of one assistant turn: sum of `text` blocks only.
;; thinking + tool_use carry no user-visible verbosity and are excluded.
(defn assistant-text-len [content]
  (cond
    (string? content) (count content)
    (sequential? content)
    (reduce + 0 (keep #(when (and (map? %) (= "text" (:type %)) (string? (:text %)))
                         (count (:text %)))
                      content))
    :else 0))

;; coarse length histogram bucket — a privacy-safe distribution, no text retained.
(defn len-bucket [n]
  (cond (>= n VERY-LONG-RESPONSE-CHARS) "6000+"
        (>= n LONG-RESPONSE-CHARS)      "2000-5999"
        (>= n 800)                      "800-1999"
        (>= n 200)                      "200-799"
        :else                           "0-199"))

;; ISO-8601 transcript timestamp -> epoch millis (nil on any parse failure).
(defn parse-ms [ts]
  (when (string? ts)
    (try (.toEpochMilli (java.time.Instant/parse ts)) (catch Exception _ nil))))

;; ---------------------------------------------------------------------------
;; per-file scan — one pass, bounded, pure (returns the unit's findings map)
(defn scan-file [^java.io.File f verbose?]
  (let [stem (str/replace (.getName f) #"\.jsonl$" "")
        st (volatile! {:stem stem :session-id nil :cwd nil :last-ts nil
                       :error-count 0 :lines 0
                       :pending {}          ; tool_use_id -> {:name :input}
                       :streak {}           ; fingerprint -> consecutive failures
                       :retries {}          ; fingerprint -> {:label :max}
                       :votes {}            ; attempted name -> count
                       :input-val {}        ; tool name -> count
                       :rejects {}          ; reason -> count
                       :guards {}           ; "label tool" -> count
                       :rereads {}          ; ~path -> top-read count
                       ;; --- verbosity advisory accumulators (W2) ---
                       :asst-responses 0    ; assistant turns bearing user-facing text
                       :asst-chars 0        ; Σ text chars (for a corpus mean; no text kept)
                       :asst-long 0         ; turns >= LONG-RESPONSE-CHARS
                       :asst-very-long 0    ; turns >= VERY-LONG-RESPONSE-CHARS
                       :len-hist {}         ; len-bucket -> count (distribution)
                       :interruptions 0     ; "[Request interrupted by user]" turns
                       :interrupt-attr {}   ; :corrective | :during-tool | :other -> count
                       :fast-skips 0        ; interrupts within FAST-SKIP-MS of the output
                       :last-asst nil       ; transient: {:textlen :tool? :ts} for attribution
                       :pending-abandon false})] ; transient: last interrupt got no reply
    (with-open [rdr (io/reader f)]
      (loop [lines (line-seq rdr) n 0]
        (when (and (seq lines) (< n MAX-LINES-PER-FILE))
          (let [^String l (first lines)]
            ;; keep every dialogue line (assistant/user carry text-length + interrupt
            ;; signals now, not just tool_use/tool_result) plus attachment hook errors;
            ;; skip only snapshots, summaries/system lines, and oversized blobs.
            (when-not (or (> (.length l) MAX-LINE-BYTES)
                          (str/starts-with? l "{\"type\":\"file-history-snapshot\"")
                          (not (or (str/includes? l "\"type\":\"assistant\"")
                                   (str/includes? l "\"type\":\"user\"")
                                   (str/includes? l "\"attachment\""))))
              ;; a malformed/odd-shaped line skips itself, never the file
              (when-let [o (try (json/parse-string l true) (catch Exception _ nil))]
                (try
                  (vswap! st #(cond-> %
                              (:sessionId o) (assoc :session-id (:sessionId o))
                              (and (:cwd o) (nil? (:cwd %))) (assoc :cwd (:cwd o))
                              (:timestamp o) (assoc :last-ts (:timestamp o))))
                (case (:type o)
                  "assistant"
                  (let [content (get-in o [:message :content])
                        tlen (assistant-text-len content)
                        has-tool? (boolean (some #(and (map? %) (= "tool_use" (:type %))) content))]
                    ;; response-length accounting — text-bearing turns only
                    (when (pos? tlen)
                      (vswap! st (fn [s] (-> s
                                            (update :asst-responses inc)
                                            (update :asst-chars + tlen)
                                            (cond-> (>= tlen LONG-RESPONSE-CHARS) (update :asst-long inc))
                                            (cond-> (>= tlen VERY-LONG-RESPONSE-CHARS) (update :asst-very-long inc))
                                            (update-in [:len-hist (len-bucket tlen)] (fnil inc 0))))))
                    ;; a produced assistant turn clears any pending-abandon and, when it
                    ;; carries text or a tool call, becomes the attribution anchor for a
                    ;; following interrupt
                    (vswap! st assoc :pending-abandon false)
                    (when (or (pos? tlen) has-tool?)
                      (vswap! st assoc :last-asst {:textlen tlen :tool? has-tool?
                                                   :ts (parse-ms (:last-ts @st))}))
                  (doseq [b content
                          :when (= "tool_use" (:type b))]
                    (vswap! st assoc-in [:pending (:id b)] {:name (:name b) :input (:input b)})
                    ;; doc re-read: a Read of a .md STARTED FROM THE TOP (offset-paging
                    ;; a big file is one logical read, not a re-read)
                    (when (and (= "Read" (:name b))
                               (some-> (get-in b [:input :file_path]) (str/ends-with? ".md"))
                               ;; offset is usually an int but appears as other shapes in the
                               ;; wild (e.g. a [from to] vector) — only a top-read counts
                               (let [off (get-in b [:input :offset])]
                                 (or (nil? off) (and (number? off) (<= off 1)))))
                      (vswap! st update-in [:rereads (tilde (get-in b [:input :file_path]))] (fnil inc 0)))))

                  "user"
                  (let [content (get-in o [:message :content])
                        utext (content-text content)
                        interrupt? (str/includes? utext "Request interrupted")]
                    (cond
                      ;; a human interrupt — attribute it to the output it cut off
                      interrupt?
                      (let [{atxt :textlen atool? :tool? ats :ts} (:last-asst @st)
                            tool-marker? (str/includes? utext "for tool use")
                            its (parse-ms (:last-ts @st))
                            gap (when (and ats its) (- its ats))
                            attr (cond (or tool-marker? atool?)                    :during-tool
                                       (and atxt (>= atxt LONG-RESPONSE-CHARS))     :corrective
                                       :else                                        :other)]
                        (vswap! st #(-> %
                                        (update :interruptions inc)
                                        (update-in [:interrupt-attr attr] (fnil inc 0))
                                        (assoc :last-asst nil :pending-abandon true)))
                        (when (and gap (<= 0 gap FAST-SKIP-MS) atxt (pos? atxt))
                          (vswap! st update :fast-skips inc)))
                      ;; a genuine human prompt (has text, not an interrupt) opens a new
                      ;; turn — the prior output is no longer the attribution anchor, and
                      ;; a continued conversation means the last interrupt was NOT abandoned
                      (seq (str/trim utext))
                      (vswap! st assoc :last-asst nil :pending-abandon false))
                  (doseq [b content
                          :when (= "tool_result" (:type b))]
                    (let [{nm :name input :input} (get (:pending @st) (:tool_use_id b))
                          _  (vswap! st update :pending dissoc (:tool_use_id b))
                          s  (content-text (:content b))
                          fp (when nm (fingerprint nm input))]
                      (if (:is_error b)
                        (do
                          (vswap! st update :error-count inc)
                          (when-let [v (unknown-tool s)] (vswap! st update-in [:votes v] (fnil inc 0)))
                          (when (and nm (str/includes? s "InputValidationError"))
                            (vswap! st update-in [:input-val nm] (fnil inc 0)))
                          (when-let [g (guard-label s)]
                            (vswap! st update-in [:guards (str g " [" (or nm "?") "]")] (fnil inc 0)))
                          (when fp
                            (let [k (inc (get-in @st [:streak fp] 0))]
                              (vswap! st assoc-in [:streak fp] k)
                              (when (>= k RETRY-THRESHOLD)
                                (vswap! st update-in [:retries fp]
                                        (fn [r] {:label (or (:label r)
                                                            (str nm (when-let [h (tool-hint nm input)] (str "(" h ")"))))
                                                 :max (max k (:max r 0))}))))))
                        ;; success of the same fingerprint ends its failure run
                        (when fp (vswap! st update :streak dissoc fp)))
                      ;; a coordinator reject is a REPLY, error-flag or not
                      (when-let [rej (engine-reject s)]
                        (vswap! st update-in [:rejects rej] (fnil inc 0))))))

                  "attachment"
                  (let [a (:attachment o)]
                    (when (contains? #{"hook_blocking_error" "hook_non_blocking_error"} (:type a))
                      (when-let [g (guard-label (str (content-text (:content a)) "\n" (:stderr a)))]
                        (vswap! st update-in [:guards (str g " [" (or (:hookName a) "hook") "]")] (fnil inc 0)))))
                  nil)
                  (catch Exception _ nil))))
            (vswap! st update :lines inc)
            (recur (rest lines) (inc n))))))
    (let [{:keys [rereads pending-abandon] :as u} @st
          u (assoc u :rereads (into {} (filter #(>= (val %) REREAD-THRESHOLD) rereads))
                     ;; the session was left on an unanswered interrupt -> abandoned (1|0)
                     :abandoned (if pending-abandon 1 0))]
      (when verbose? (binding [*out* *err*] (println "  scanned" (tilde (.getPath f)) (:lines u) "lines")))
      (dissoc u :pending :streak :lines :last-asst :pending-abandon))))

(defn findings? [{:keys [votes input-val rejects retries guards rereads]}]
  (boolean (seq (concat votes input-val rejects retries guards rereads))))

;; ---------------------------------------------------------------------------
;; fact emission — @mine:<stem>, existing predicates + the one minted `verb_vote`
(defn emit-facts! [port {:keys [stem session-id cwd last-ts error-count
                                 votes input-val rejects retries guards rereads]}]
  (let [te (str "@mine:" stem)
        note! (fn [s] (north.coord/append! port te "note" s))]
    (north.coord/put! port te "kind" "mine")
    (north.coord/put! port te "session_id" (or session-id stem))
    (when cwd (north.coord/put! port te "repo" (tilde cwd)))
    (when last-ts (north.coord/put! port te "at" last-ts))
    (when (pos? error-count) (north.coord/put! port te "error_count" (str error-count)))
    (doseq [v (take MAX-VOTES-PER-UNIT (keys votes))]
      (north.coord/append! port te "verb_vote" v))
    (doseq [s (take MAX-NOTES-PER-UNIT
                    (concat (for [[nm c] input-val] (str "input_validation: " nm " x" (bucket c)))
                            (for [[_ {:keys [label max]}] retries] (str "retry_loop: " label " x" (bucket max)))
                            (for [[doc c] rereads] (str "doc_reread: " doc " x" (bucket c)))
                            (for [[g c] guards] (str "guard_denial: " g " x" (bucket c)))
                            (for [[r c] rejects] (str "engine_reject: " r " x" (bucket c)))))]
      (note! s))))

;; ---------------------------------------------------------------------------
;; verbosity advisory (thread 019f7d16 W2)
;;
;; per-session verbosity notes ride the SAME @mine:<stem> subject as the struggle
;; signals, using the already-registered `note` predicate — no new per-session
;; vocabulary. Emitted only for a session that actually shows a signal.
(defn verbosity-notable? [{:keys [asst-long interruptions fast-skips abandoned]}]
  (boolean (or (pos? (or asst-long 0)) (pos? (or interruptions 0))
               (pos? (or fast-skips 0)) (pos? (or abandoned 0)))))

(defn emit-verbosity! [port {:keys [stem session-id cwd last-ts asst-responses asst-long
                                    asst-very-long interruptions interrupt-attr
                                    fast-skips abandoned]}]
  (let [te (str "@mine:" stem)
        note! (fn [s] (north.coord/append! port te "note" s))]
    ;; ensure the header exists even for a verbosity-only session (idempotent puts)
    (north.coord/put! port te "kind" "mine")
    (north.coord/put! port te "session_id" (or session-id stem))
    (when cwd (north.coord/put! port te "repo" (tilde cwd)))
    (when last-ts (north.coord/put! port te "at" last-ts))
    (when (pos? (or asst-long 0))
      (note! (str "response_length: " asst-responses " text-turns, " asst-long
                  " long(>=" LONG-RESPONSE-CHARS "c), " asst-very-long
                  " very-long(>=" VERY-LONG-RESPONSE-CHARS "c)")))
    (when (pos? (or interruptions 0))
      (note! (str "interruption: " interruptions " total, "
                  (get interrupt-attr :corrective 0) " corrective(after-long-text), "
                  (get interrupt-attr :during-tool 0) " during-tool, "
                  (get interrupt-attr :other 0) " other")))
    (when (pos? (or fast-skips 0)) (note! (str "fast_skip: " fast-skips)))
    (when (pos? (or abandoned 0)) (note! "abandoned_output: session left on an interrupt"))))

;; PURE aggregate over the scanned units — deterministic (all folds are sums; no
;; map-order dependence). Never performs I/O and never reads routing state. The
;; verdict is advice, not action: below MIN-ADVISORY-* it is insufficient-evidence,
;; so a small corpus can never produce a recommendation.
(defn advisory-of [units]
  (let [Σ  (fn [k] (reduce + 0 (map #(or (get % k) 0) units)))
        Σa (fn [a] (reduce + 0 (map #(get-in % [:interrupt-attr a] 0) units)))
        resp (Σ :asst-responses)
        long (Σ :asst-long)
        vlong (Σ :asst-very-long)
        chars (Σ :asst-chars)
        ints  (Σ :interruptions)
        corr  (Σa :corrective)
        tool  (Σa :during-tool)
        other (Σa :other)
        skips (Σ :fast-skips)
        aband (Σ :abandoned)
        sessions (count (filter #(pos? (or (:asst-responses %) 0)) units))
        rate (fn [a b] (if (pos? b) (/ (double a) b) 0.0))
        long-rate (rate long resp)
        corr-rate (rate corr resp)
        skip-rate (rate skips resp)
        insufficient? (or (< resp MIN-ADVISORY-RESPONSES) (< sessions MIN-ADVISORY-SESSIONS))
        verdict (cond
                  insufficient? "insufficient-evidence"
                  (and (>= long-rate LONG-RATE-ADVISORY)
                       (or (>= corr-rate PUSHBACK-RATE-ADVISORY)
                           (>= skip-rate PUSHBACK-RATE-ADVISORY))) "verbose-tendency"
                  :else "within-norms")]
    {:responses resp :sessions sessions :long long :very-long vlong
     :mean-chars (if (pos? resp) (quot chars resp) 0)
     :interruptions ints :corrective corr :during-tool tool :other-interrupt other
     :fast-skips skips :abandoned aband
     :long-rate long-rate :corrective-rate corr-rate :fast-skip-rate skip-rate
     :verdict verdict}))

;; PURE projection of an advisory to the fact tuples it would write, so a test can
;; PROVE the emitter never touches a routing/prompt/posture/model/Gaffer predicate:
;; every tuple is a fact write ([:put|:append]) on an @advisory:*/@mine:* subject
;; drawn from ADVISORY-PREDS alone. This is the no-mutation contract in code.
(def ^:const ADVISORY-SUBJECT "@advisory:verbosity")
(def ADVISORY-PREDS
  #{"kind" "advises" "at" "sample_responses" "sample_sessions"
    "long_response_rate" "corrective_interruption_rate" "fast_skip_rate"
    "verdict" "note"})
(defn- f3 [x] (format "%.3f" (double x)))
(defn advisory-note [adv]
  (str "verbosity advisory (" (:verdict adv) "): "
       (:responses adv) " text-turns / " (:sessions adv) " sessions, "
       "mean " (:mean-chars adv) "c, " (:long adv) " long / " (:very-long adv) " very-long; "
       "interrupts " (:interruptions adv) " (" (:corrective adv) " corrective, "
       (:during-tool adv) " during-tool), fast-skips " (:fast-skips adv)
       ", abandoned " (:abandoned adv)))
(defn advisory-facts [adv date]
  (let [te ADVISORY-SUBJECT]
    [[:put te "kind" "advisory"]
     [:put te "advises" "response_verbosity"]
     [:put te "at" (str date)]
     [:put te "sample_responses" (str (:responses adv))]
     [:put te "sample_sessions" (str (:sessions adv))]
     [:put te "long_response_rate" (f3 (:long-rate adv))]
     [:put te "corrective_interruption_rate" (f3 (:corrective-rate adv))]
     [:put te "fast_skip_rate" (f3 (:fast-skip-rate adv))]
     [:put te "verdict" (:verdict adv)]
     [:append te "note" (advisory-note adv)]]))

(defn emit-advisory! [port adv date]
  (doseq [[op te pred val] (advisory-facts adv date)]
    (case op
      :put    (north.coord/put! port te pred val)
      :append (north.coord/append! port te pred val))))

;; ---------------------------------------------------------------------------
;; incremental state
(def state-file (io/file home ".local/state/north/north-mine/state.edn"))
(defn load-state [] (try (edn/read-string (slurp state-file)) (catch Exception _ {})))
(defn save-state! [m]
  (io/make-parents state-file)
  (spit state-file (pr-str m)))

;; ---------------------------------------------------------------------------
;; report
(defn- rank [m n] (take n (sort-by (comp - val) m)))

(defn report-md [units meta]
  (let [agg (fn [k] (apply merge-with + (map k units)))          ; name -> total hits
        sess (fn [k] (frequencies (mapcat (comp keys k) units))) ; name -> #sessions
        votes (agg :votes) vsess (sess :votes)
        ivals (agg :input-val)
        rejects (agg :rejects)
        guards (agg :guards)
        rereads (agg :rereads) rsess (sess :rereads)
        retries (->> units (mapcat (comp vals :retries)) (map (juxt :label :max))
                     (reduce (fn [m [l x]] (update m l (fnil max 0) x)) {}))
        retry-n (->> units (mapcat (comp vals :retries)) (map :label) frequencies)
        errors (reduce + 0 (map :error-count units))
        sec (fn [title rows fmt]
              (str "\n## " title "\n\n"
                   (if (seq rows) (str/join "\n" (map fmt rows)) "(none found)") "\n"))]
    (str "# Telemetry baseline — " (:date meta) "\n\n"
         "Mined by `north-mine` over " (:files meta) " transcript files ("
         (:units-with-findings meta) " with findings) under `~/.claude/projects/`. "
         "Signals only — no message content.\n\n"
         "Total tool_result errors seen: " errors "\n"
         (sec "Verb votes — hallucinated tool names (the vocabulary signal)"
              (rank votes 25)
              (fn [[v c]] (str "- `" v "` — " c " call(s) across " (get vsess v 1) " session(s)")))
         (sec "InputValidationError by tool (deferred-tool + param friction)"
              (rank ivals 20)
              (fn [[t c]] (str "- `" t "` — " c)))
         (sec "Retry loops (same tool+input failing >=3x without a success)"
              (sort-by (comp - val) retry-n)
              (fn [[l c]] (str "- `" l "` — " c " episode(s), worst streak x" (get retries l))))
         (sec "Doc re-reads (same .md read from the top >=3x in one session)"
              (rank rereads 25)
              (fn [[d c]] (str "- `" d "` — " c " top-reads across " (get rsess d 1) " session(s)")))
         (sec "Guard denials" (rank guards 20) (fn [[g c]] (str "- " g " — " c)))
         (sec "Engine rejections (coordinator reject:*)"
              (rank rejects 20) (fn [[r c]] (str "- `reject:" r "` — " c)))
         (let [adv (advisory-of units)
               hist (agg :len-hist)
               order ["0-199" "200-799" "800-1999" "2000-5999" "6000+"]]
           (str "\n## Verbosity advisory (response length / interruption / fast-skip)\n\n"
                "**Verdict: " (:verdict adv) "**"
                (when (= "insufficient-evidence" (:verdict adv))
                  (str " (need >=" MIN-ADVISORY-RESPONSES " text-turns and >="
                       MIN-ADVISORY-SESSIONS " sessions; have "
                       (:responses adv) " / " (:sessions adv) ")"))
                " — advisory only; no prompt/routing change.\n\n"
                "- Text-turns: " (:responses adv) " across " (:sessions adv)
                " session(s), mean " (:mean-chars adv) " chars\n"
                "- Long (>=" LONG-RESPONSE-CHARS "c): " (:long adv)
                " (" (f3 (:long-rate adv)) "), very-long (>=" VERY-LONG-RESPONSE-CHARS "c): "
                (:very-long adv) "\n"
                "- Interruptions: " (:interruptions adv) " total — "
                (:corrective adv) " corrective (" (f3 (:corrective-rate adv))
                "), " (:during-tool adv) " during-tool, " (:other-interrupt adv) " other\n"
                "- Fast-skips: " (:fast-skips adv) " (" (f3 (:fast-skip-rate adv))
                "), abandoned sessions: " (:abandoned adv) "\n"
                "- Length distribution: "
                (str/join ", " (for [b order :when (get hist b)] (str b "=" (get hist b))))
                "\n")))))

;; ---------------------------------------------------------------------------
(defn -main [& args]
  (let [port (Integer/parseInt (or (first args) north.coord/PORT))
        opts (set (rest args))
        opt-val (fn [flag] (second (drop-while #(not= % flag) (rest args))))
        root (io/file (or (opt-val "--root") (str home "/.claude/projects")))
        dry? (contains? opts "--dry-run")
        full? (contains? opts "--full")
        verbose? (contains? opts "--verbose")
        limit (some-> (opt-val "--limit") Integer/parseInt)
        report-file (opt-val "--report")
        state (if full? {} (load-state))
        all (->> (file-seq root)
                 (filter #(and (.isFile ^java.io.File %) (str/ends-with? (.getName ^java.io.File %) ".jsonl")
                               (pos? (.length ^java.io.File %)))))
        todo (->> all
                  (remove (fn [^java.io.File f]
                            (when-let [{:keys [mtime size]} (get state (.getPath f))]
                              (and (= mtime (.lastModified f)) (= size (.length f))))))
                  (sort-by #(.lastModified ^java.io.File %))
                  (#(if limit (take limit %) %)))
        _ (binding [*out* *err*]
            (println (format "north-mine: %d transcript files, %d to scan%s"
                             (count all) (count todo) (if dry? " (dry-run)" ""))))
        units (volatile! [])
        state' (volatile! state)]
    (doseq [^java.io.File f todo]
      (let [u (try (scan-file f verbose?)
                   (catch Exception e
                     (binding [*out* *err*] (println "  ERROR" (.getPath f) (ex-message e)))
                     nil))]
        (when u
          (vswap! units conj u)
          (when-not dry?
            (when (findings? u) (emit-facts! port u))
            (when (verbosity-notable? u) (emit-verbosity! port u)))
          (vswap! state' assoc (.getPath f) {:mtime (.lastModified f) :size (.length f)}))))
    (when-not dry? (save-state! @state'))
    (let [us @units
          with-findings (filter findings? us)
          adv (advisory-of us)]
      (binding [*out* *err*]
        (println (format "north-mine: scanned %d, findings in %d, facts %s"
                         (count us) (count with-findings)
                         (if dry? "SKIPPED (dry-run)" "written via coordinator")))
        (println (format "north-mine: verbosity advisory = %s (%d text-turns / %d sessions)"
                         (:verdict adv) (:responses adv) (:sessions adv))))
      ;; the aggregate advisory reflects the WHOLE corpus, so it is only PUT on a
      ;; --full scan — an incremental run would clobber it with a partial sample.
      ;; Never on --dry-run.
      (when (and full? (not dry?))
        (emit-advisory! port adv (java.time.LocalDate/now)))
      (when report-file
        (io/make-parents (io/file report-file))
        (spit report-file
              (report-md us {:date (str (java.time.LocalDate/now))
                             :files (count us)
                             :units-with-findings (count with-findings)}))
        (binding [*out* *err*] (println "report ->" (tilde report-file)))))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
