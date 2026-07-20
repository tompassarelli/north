#!/usr/bin/env bb
;; mine-verbosity-test.clj — the W2 verbosity advisory in north-mine.clj.
;; Daemon-free and content-free: exercises the PURE extractors/aggregator and
;; scan-file over synthetic transcripts written to a temp dir. Proves:
;;   * response-length thresholds (long / very-long boundaries, distribution)
;;   * interruption ATTRIBUTION (corrective / during-tool / other)
;;   * fast-skip window + abandoned-session detection
;;   * empty + small corpora -> insufficient-evidence (never a recommendation)
;;   * scan is DETERMINISTIC (same bytes -> same map)
;;   * the advisory emitter NEVER writes a routing/prompt/posture/model predicate
;;
;;   bb cli/tests/mine-verbosity-test.clj [port]
;;
;; We pre-load coord.clj so north-mine's guarded sibling-load is skipped and the
;; file loads as a plain library (its main-guard stays dormant under load-file).
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json])

(def here (.getParent (io/file (System/getProperty "babashka.file"))))
(load-file (str here "/../coord.clj"))
(load-file (str here "/../north-mine.clj"))

(def checks (atom []))
(defn chk [nm ok] (swap! checks conj [nm (boolean ok)]))

;; --- synthetic transcript builders ----------------------------------------
(defn chars [n] (apply str (repeat n \x)))
(defn jline [m] (json/generate-string m))
(defn asst-text [ts n]
  (jline {:type "assistant" :timestamp ts :sessionId "s"
          :message {:role "assistant" :content [{:type "text" :text (chars n)}]}}))
(defn asst-tool [ts id]
  (jline {:type "assistant" :timestamp ts :sessionId "s"
          :message {:role "assistant"
                    :content [{:type "tool_use" :id id :name "Bash" :input {:command "ls"}}]}}))
(defn usr-prompt [ts s]
  (jline {:type "user" :timestamp ts :sessionId "s"
          :message {:role "user" :content [{:type "text" :text s}]}}))
(defn usr-interrupt [ts tool?]
  (usr-prompt ts (if tool? "[Request interrupted by user for tool use]"
                            "[Request interrupted by user]")))

(defn scan-lines [lines]
  (let [f (java.io.File/createTempFile "mine-verb" ".jsonl")]
    (.deleteOnExit f)
    (spit f (str (str/join "\n" lines) "\n"))
    (scan-file f false)))

;; ==========================================================================
;; 1. pure extractors
(chk "assistant-text-len sums text blocks only (ignores thinking/tool_use)"
     (= 5 (assistant-text-len [{:type "thinking" :text "zzzzzzzz"}
                               {:type "text" :text "hello"}
                               {:type "tool_use" :name "Bash"}])))
(chk "assistant-text-len handles a bare string body" (= 3 (assistant-text-len "abc")))
(chk "assistant-text-len of non-text is 0" (= 0 (assistant-text-len [{:type "tool_use"}])))
(chk "len-bucket boundaries" (and (= "2000-5999" (len-bucket LONG-RESPONSE-CHARS))
                                  (= "800-1999" (len-bucket (dec LONG-RESPONSE-CHARS)))
                                  (= "6000+" (len-bucket VERY-LONG-RESPONSE-CHARS))
                                  (= "0-199" (len-bucket 0))))
(chk "parse-ms parses ISO, nils garbage"
     (and (= 0 (parse-ms "1970-01-01T00:00:00.000Z")) (nil? (parse-ms "not-a-time"))
          (nil? (parse-ms nil))))

;; ==========================================================================
;; 2. empty + trivial scans
(let [u (scan-lines [])]
  (chk "empty transcript: 0 responses, not verbosity-notable, abandoned 0"
       (and (= 0 (:asst-responses u)) (false? (verbosity-notable? u)) (= 0 (:abandoned u)))))

;; ==========================================================================
;; 3. response-length thresholds + distribution
(let [u (scan-lines [(usr-prompt "2026-07-20T10:00:00.000Z" "hi")
                     (asst-text "2026-07-20T10:00:01.000Z" 100)                       ; short
                     (asst-text "2026-07-20T10:00:02.000Z" LONG-RESPONSE-CHARS)        ; long (boundary)
                     (asst-text "2026-07-20T10:00:03.000Z" (dec LONG-RESPONSE-CHARS))  ; NOT long (boundary-1)
                     (asst-text "2026-07-20T10:00:04.000Z" VERY-LONG-RESPONSE-CHARS)])] ; very-long
  (chk "counts every text turn" (= 4 (:asst-responses u)))
  (chk "long threshold is inclusive; boundary-1 excluded" (= 2 (:asst-long u)))
  (chk "very-long threshold inclusive" (= 1 (:asst-very-long u)))
  (chk "chars summed" (= (+ 100 LONG-RESPONSE-CHARS (dec LONG-RESPONSE-CHARS) VERY-LONG-RESPONSE-CHARS)
                         (:asst-chars u)))
  (chk "length histogram populated" (= 1 (get-in u [:len-hist "0-199"]))))

;; ==========================================================================
;; 4. interruption attribution
(let [u (scan-lines [(asst-text "2026-07-20T10:00:00.000Z" 3000)          ; long output
                     (usr-interrupt "2026-07-20T10:00:30.000Z" false)])]  ; plain interrupt, +30s
  (chk "plain interrupt after a long text turn -> corrective"
       (and (= 1 (:interruptions u)) (= 1 (get-in u [:interrupt-attr :corrective]))))
  (chk "corrective interrupt outside fast-skip window is not a fast-skip" (= 0 (:fast-skips u))))

(let [u (scan-lines [(asst-tool "2026-07-20T10:00:00.000Z" "t1")
                     (usr-interrupt "2026-07-20T10:00:01.000Z" true)])]
  (chk "interrupt marked 'for tool use' -> during-tool"
       (= 1 (get-in u [:interrupt-attr :during-tool]))))

(let [u (scan-lines [(asst-tool "2026-07-20T10:00:00.000Z" "t1")
                     (usr-interrupt "2026-07-20T10:00:01.000Z" false)])] ; plain, but last output was a tool call
  (chk "plain interrupt after a tool call -> during-tool (attributed by anchor)"
       (= 1 (get-in u [:interrupt-attr :during-tool]))))

(let [u (scan-lines [(asst-text "2026-07-20T10:00:00.000Z" 100)          ; SHORT output
                     (usr-interrupt "2026-07-20T10:00:30.000Z" false)])]
  (chk "plain interrupt after a short turn -> other (not blamed on verbosity)"
       (= 1 (get-in u [:interrupt-attr :other]))))

(let [u (scan-lines [(asst-text "2026-07-20T10:00:00.000Z" 3000)          ; long
                     (usr-prompt "2026-07-20T10:00:10.000Z" "new topic")  ; fresh human turn resets anchor
                     (usr-interrupt "2026-07-20T10:00:12.000Z" false)])]  ; interrupt w/ no output since
  (chk "a fresh prompt clears the anchor: later interrupt is 'other', not corrective"
       (and (= 0 (get-in u [:interrupt-attr :corrective] 0))
            (= 1 (get-in u [:interrupt-attr :other])))))

;; ==========================================================================
;; 5. fast-skip window + abandonment
(let [u (scan-lines [(asst-text "2026-07-20T10:00:00.000Z" 3000)
                     (usr-interrupt "2026-07-20T10:00:03.000Z" false)])] ; +3s <= FAST-SKIP-MS
  (chk "interrupt within the fast-skip window counts as a fast-skip" (= 1 (:fast-skips u)))
  (chk "session left on an interrupt is abandoned" (= 1 (:abandoned u))))

(let [u (scan-lines [(asst-text "2026-07-20T10:00:00.000Z" 3000)
                     (usr-interrupt "2026-07-20T10:00:03.000Z" false)
                     (usr-prompt "2026-07-20T10:00:20.000Z" "carry on")])]
  (chk "a prompt after the interrupt clears abandonment" (= 0 (:abandoned u))))

;; ==========================================================================
;; 6. determinism — identical bytes -> identical scan map
(let [lines [(asst-text "2026-07-20T10:00:00.000Z" 2500)
             (usr-interrupt "2026-07-20T10:00:02.000Z" false)
             (asst-text "2026-07-20T10:00:05.000Z" 500)]]
  ;; dissoc :stem — it is the (unique) temp filename, not a scan result
  (chk "scan-file is deterministic"
       (= (dissoc (scan-lines lines) :stem) (dissoc (scan-lines lines) :stem))))

;; ==========================================================================
;; 7. advisory verdict gating (pure aggregate over synthetic units)
(defn unit [resp long chars corrective skips aband]
  {:asst-responses resp :asst-long long :asst-very-long 0 :asst-chars chars
   :interruptions corrective :interrupt-attr {:corrective corrective :during-tool 0 :other 0}
   :fast-skips skips :abandoned aband})
(let [big-verbose (repeat 3 (unit 20 10 40000 2 0 0))    ; 60 resp, long .5, corr .1
      big-longonly (repeat 3 (unit 20 10 40000 0 0 0))   ; long .5, zero pushback
      big-terse   (repeat 3 (unit 20 1 8000 2 0 0))      ; long .05, has pushback
      small       [(unit 5 5 20000 3 3 1)]]              ; only 5 resp / 1 session
  (chk "empty corpus -> insufficient-evidence" (= "insufficient-evidence" (:verdict (advisory-of []))))
  (chk "sub-threshold sample -> insufficient-evidence (even if all-long)"
       (= "insufficient-evidence" (:verdict (advisory-of small))))
  (chk "long outputs + human push-back -> verbose-tendency"
       (= "verbose-tendency" (:verdict (advisory-of big-verbose))))
  (chk "long outputs but NO push-back -> within-norms"
       (= "within-norms" (:verdict (advisory-of big-longonly))))
  (chk "push-back but outputs not long -> within-norms"
       (= "within-norms" (:verdict (advisory-of big-terse))))
  (chk "advisory-of is deterministic" (= (advisory-of big-verbose) (advisory-of big-verbose)))
  (chk "rates are computed correctly"
       (let [a (advisory-of big-verbose)]
         (and (== 0.5 (:long-rate a)) (== 0.1 (:corrective-rate a)) (= 60 (:responses a))))))

;; ==========================================================================
;; 8. NO ROUTING MUTATION — the emitter's fact projection is provably confined
(def FORBIDDEN
  #{"posture" "model" "provider" "reasoning" "composition" "role" "tier" "taskGrade"
    "task_grade" "gaffer_role" "prompt" "driver" "committed" "depends_on" "part_of"
    "abandoned" "canceled" "outcome" "title"})
(let [adv (advisory-of (repeat 3 (unit 20 10 40000 2 0 0)))
      facts (advisory-facts adv "2026-07-20")]
  (chk "every advisory fact is a put/append write (no lifecycle/routing op)"
       (every? #{:put :append} (map first facts)))
  (chk "every subject is an @advisory:/@mine: mining subject"
       (every? #(let [s (second %)] (or (str/starts-with? s "@advisory:") (str/starts-with? s "@mine:")))
               facts))
  (chk "every predicate is drawn from the advisory vocabulary alone"
       (every? #(contains? ADVISORY-PREDS (nth % 2)) facts))
  (chk "no advisory fact touches a routing/prompt/posture/model predicate"
       (not-any? #(contains? FORBIDDEN (nth % 2)) facts))
  (chk "the advisory vocabulary is itself disjoint from routing predicates"
       (empty? (clojure.set/intersection ADVISORY-PREDS FORBIDDEN)))
  (chk "the emitted verdict is one of the three sanctioned strings"
       (contains? #{"insufficient-evidence" "within-norms" "verbose-tendency"}
                  (some (fn [[_ _ p v]] (when (= p "verdict") v)) facts))))

;; --- report ----------------------------------------------------------------
(let [results @checks pass (count (filter second results))]
  (doseq [[nm ok] results] (println (format "  [%s]  %s" (if ok "PASS" "FAIL") nm)))
  (println (format "\nmine verbosity advisory (north-mine.clj): %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
