#!/usr/bin/env bb
;; spend-cli.clj — the spend-guard budget LEDGER primitive (build-order step 2).
;;
;; Owns every ledger side effect: schema declaration, budget creation, the
;; cap-checked CAS RESERVATION at admission, terminal SETTLEMENT, human
;; overrides, and read-only status/headroom. Lives in clj — not TS — because the
;; coordinator write wire (`:assert-at-version` global-version CAS) is exposed
;; ONLY through cli/coord.clj; the TS SDK client is read-only (north-client.ts)
;; and its coordination.ts is the real-time ping channel, not a write surface.
;; The TS admission seam shells out to `reserve`/`settle` here (same shape the
;; step-1 read path already shells `north json show`, and the Linear adapter
;; shells reserve-link.clj). Correctness over elegance — per the CAS verdict
;; (fram-cas-verification-2026-07-19.md), the reservation is a read-check-commit
;; loop, NEVER a bare read-then-tell (the documented lost-update path).
;;
;; DUAL MODE: `north spend <verb>` dispatches init/status/override here (the
;; cockpit surface — every verb prints the primitive it ran). reserve/settle are
;; machine verbs the harness invokes; they print one JSON line for the caller.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

;; Resolve coord.clj relative to THIS file (*file* is bound both when run
;; directly and when load-file'd as a library by a test), so the shared write
;; substrate loads regardless of the entry script's directory.
(load-file (str (.getParent (io/file *file*)) "/coord.clj"))

(def port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

;; --- micro-USD money (integers only; no float drift in the fact log) ---------
;; A USD string like "1.50" -> 1500000 micro-USD. At most 6 fractional digits.
(defn usd->micro [label s]
  (let [m (re-matches #"(\d+)(?:\.(\d{1,6}))?" (str s))]
    (when-not m
      (throw (ex-info (str label " must be a non-negative USD amount with at most 6 decimals: got " (pr-str s)) {})))
    (let [whole (parse-long (nth m 1))
          frac  (nth m 2)
          frac6 (when frac (parse-long (str/join (take 6 (concat frac (repeat \0))))))]
      (+ (* whole 1000000) (or frac6 0)))))

(defn require-pos-micro [label s]
  (let [v (usd->micro label s)]
    (when-not (pos? v) (throw (ex-info (str label " must be greater than zero") {})))
    v))

(defn micro->usd [micro] (format "%.6f" (/ (double micro) 1e6)))

;; --- entity ids + period -----------------------------------------------------
(defn budget-id [target] (str "spend-budget:" target))
(defn utc-month [] (.format (java.time.format.DateTimeFormatter/ofPattern "yyyy-MM")
                            (java.time.ZonedDateTime/now java.time.ZoneOffset/UTC)))
(defn period-id [target month] (str "spend-period:" target ":" month))
(defn at [id] (str "@" id))

;; --- ledger reads (fail-closed) ----------------------------------------------
(defn counter
  "Current live value of a single-valued micro-USD counter, 0 when absent.
   nil (absent) => 0; a non-integer live value is a corrupt ledger => throw."
  [port subject pred]
  (let [v (north.coord/resolved port (at subject) pred)]
    (cond (nil? v) 0
          (re-matches #"\d+" (str v)) (parse-long (str v))
          :else (throw (ex-info (str "corrupt counter " subject "/" pred " = " (pr-str v)) {})))))

(defn budget-single [port target pred]
  (north.coord/resolved port (at (budget-id target)) pred))

;; The single AUTHORITY on whether a counter predicate is safely single-valued.
;; A multi-valued counter silently never advances (resolved returns the earliest
;; coexist-elect, not the latest write) — the analyst's trap. Reservation MUST
;; fail closed if this declaration is absent. The engine stores cardinality as a
;; schema-writable fact on the raw @-prefixed predicate-name subject; it is NOT
;; reachable through :resolved (which resolves only NAMED entities via
;; resolve-name) but IS visible to the :query engine's reified-with-schema view.
;; Query values wire back as EDN symbols, so compare through str.
(defn declared-single? [port pred]
  (let [res (north.coord/send-op
             port {:op :query
                   :query {:find "v"
                           :rules [{:head {:rel "v" :args [{:var "v"}]}
                                    :body [{:rel "triple" :args [(at pred) "cardinality" {:var "v"}]}]}]}})]
    (boolean (some #(= "single" (str (first %))) (:ok res)))))

;; --- overrides ---------------------------------------------------------------
;; A spend_override is a multi-valued fact on the budget entity carrying a
;; JSON object {amount_microusd, until_ms, reason, created_by}. Only unexpired
;; overrides add headroom.
(defn active-override-micro [port target now-ms]
  (->> (north.coord/many port (at (budget-id target)) "spend_override")
       (keep (fn [raw]
               (try (let [{:strs [amount_microusd until_ms]} (json/parse-string (str raw))]
                      (when (and (integer? amount_microusd) (integer? until_ms) (> until_ms now-ms))
                        amount_microusd))
                    (catch Exception _ nil))))
       (reduce + 0)))

;; --- budget completeness (step-2 adds PRICES to the step-1 config set) --------
(def REQUIRED-MICRO ["budget_cap_microusd" "lane_envelope_default_microusd"
                     "lane_envelope_max_microusd" "burn_limit_microusd_per_hour"
                     "price_in_per_mtok" "price_out_per_mtok"])

(defn budget-defect
  "nil when the budget entity is complete + well-formed for RESERVATION; else a
   machine reason. Prices are required here (design §1: missing price refuses at
   admission, fail-closed) even though the step-1 tripwire did not require them."
  [port target]
  (let [id (budget-id target)]
    (or (some (fn [p]
                (let [v (budget-single port target p)]
                  (cond (nil? v) (str id " missing " p)
                        (not (re-matches #"\d+" (str v))) (str id " " p " not a micro-USD integer")
                        (not (pos? (parse-long (str v)))) (str id " " p " must be > 0"))))
              REQUIRED-MICRO)
        (when (str/blank? (str (budget-single port target "layer1_confirmed")))
          (str id " missing layer1_confirmed")))))

;; --- reservation: the read-check-commit CAS loop (fram-cas §4) ---------------
;; base = coordinator global version captured BEFORE reading the counter; cap
;; checked INSIDE the loop; commit via :assert-at-version; bounded retry on
;; :reject :conflict. Never a bare read-then-tell.
(def CAS-TRIES 16)

(defn reserve!
  "Reserve ENVELOPE-MICRO against TARGET's current period. Returns a result map."
  [port target envelope-micro]
  (or
   ;; fail-closed preconditions
   (when-not (and (integer? envelope-micro) (pos? envelope-micro))
     {:ok false :reason "bad-envelope"})
   (when-let [defect (budget-defect port target)]
     {:ok false :reason "budget-incomplete" :detail defect})
   (when-not (declared-single? port "reserved_microusd")
     {:ok false :reason "missing-schema" :detail "reserved_microusd not declared cardinality single — run `north spend init`"})
   (when-not (declared-single? port "settled_microusd")
     {:ok false :reason "missing-schema" :detail "settled_microusd not declared cardinality single — run `north spend init`"})
   (let [cap    (parse-long (str (budget-single port target "budget_cap_microusd")))
         month  (utc-month)
         period (period-id target month)]
     (loop [tries CAS-TRIES]
       (let [base      (north.coord/cur-ver port)
             reserved  (counter port period "reserved_microusd")
             settled   (counter port period "settled_microusd")
             overrides (active-override-micro port target (System/currentTimeMillis))
             limit     (+ cap overrides)]
         (if (> (+ reserved settled envelope-micro) limit)
           {:ok false :reason "over-cap" :period period
            :reserved reserved :settled settled :cap cap :overrides overrides :envelope envelope-micro}
           (let [res (north.coord/send-op
                      port {:op :assert-at-version :te (at period)
                            :p "reserved_microusd" :r (str (+ reserved envelope-micro)) :base base})]
             (cond
               (:ok res) {:ok true :period period :reserved (+ reserved envelope-micro) :envelope envelope-micro}
               (and (= :conflict (:reject res)) (> tries 1)) (recur (dec tries))
               (:reject res) {:ok false :reason "conflict-exhausted" :period period :reject (:reject res)}
               :else {:ok false :reason "commit-failed" :res res}))))))))

;; --- generic single-counter CAS increment (settlement) -----------------------
(defn cas-add!
  "Add DELTA (may be negative; floored at 0) to a single-valued counter on
   PERIOD. Same base-CAS loop, recomputing inside. Returns {:ok true/false}."
  [port period pred delta]
  (loop [tries CAS-TRIES]
    (let [base (north.coord/cur-ver port)
          cur  (counter port period pred)
          nv   (max 0 (+ cur delta))
          res  (north.coord/send-op
                port {:op :assert-at-version :te (at period) :p pred :r (str nv) :base base})]
      (cond
        (:ok res) {:ok true :value nv}
        (and (= :conflict (:reject res)) (> tries 1)) (recur (dec tries))
        :else {:ok false :reason "conflict-exhausted" :res res}))))

;; --- settlement: reservations only ever settle DOWN, only on exact evidence --
;; exact token evidence + fresh prices -> final = min(actual, reserved-micro),
;; release the remainder. Unknown/lower-bound coverage -> full reservation
;; stands (final = reserved-micro). Either way: settled += final (FIRST — the
;; conservative order, a concurrent reader never transiently sees MORE headroom),
;; then reserved -= reserved-micro.
(defn exact-cost-micro [port target input-tokens output-tokens]
  (let [pin  (parse-long (str (budget-single port target "price_in_per_mtok")))
        pout (parse-long (str (budget-single port target "price_out_per_mtok")))]
    (long (Math/round (+ (/ (* (double input-tokens) pin) 1e6)
                         (/ (* (double output-tokens) pout) 1e6))))))

(defn settle!
  "Settle a run's reservation. EVIDENCE is {:status exact|unknown, :input n, :output n}."
  [port target period reserved-micro {:keys [status input output]}]
  (or
   (when-not (declared-single? port "reserved_microusd")
     {:ok false :reason "missing-schema"})
   (let [exact? (= status "exact")
         final  (if exact?
                  (min reserved-micro (exact-cost-micro port target (or input 0) (or output 0)))
                  reserved-micro)
         s-res  (cas-add! port period "settled_microusd" final)]
     (if-not (:ok s-res)
       {:ok false :reason "settle-commit-failed" :detail s-res}
       (let [r-res (cas-add! port period "reserved_microusd" (- reserved-micro))]
         {:ok (boolean (:ok r-res))
          :final final
          :evidence (if exact? "exact" "reserved-worst-case")
          :released (if exact? (- reserved-micro final) 0)
          :settled (:value s-res)
          :reserved (:value r-res)})))))

;; --- flag parsing ------------------------------------------------------------
(defn parse-flags [args]
  (loop [a args m {}]
    (if-let [[k v & more] (seq a)]
      (if (str/starts-with? (str k) "--")
        (recur more (assoc m (subs k 2) v))
        (recur (rest a) m))
      m)))

;; ============================================================================
;; VERBS
;; ============================================================================

(defn cmd-init [target args]
  (when (str/blank? target) (throw (ex-info "usage: north spend init <target> --cap-usd X --envelope-default-usd Y --envelope-max-usd Z --burn-limit-usd-hr W --i-confirm-layer1 \"prepaid, auto-topup off, <date>\"" {})))
  (let [f (parse-flags args)
        cap  (require-pos-micro "--cap-usd" (get f "cap-usd"))
        edef (require-pos-micro "--envelope-default-usd" (get f "envelope-default-usd"))
        emax (require-pos-micro "--envelope-max-usd" (get f "envelope-max-usd"))
        burn (require-pos-micro "--burn-limit-usd-hr" (get f "burn-limit-usd-hr"))
        layer1 (get f "i-confirm-layer1")]
    (when (str/blank? (str layer1))
      (throw (ex-info "refusing to create a budget without layer-1 confirmation: pass --i-confirm-layer1 \"prepaid, auto-topup off, <date>\" (auto-top-up MUST be off; this is the balance-independent safety floor)" {})))
    (when (> edef emax)
      (throw (ex-info "--envelope-default-usd cannot exceed --envelope-max-usd" {})))
    (let [id (budget-id target)]
      ;; 1) declare the counter schema FIRST — a reservation fail-closes without it.
      (doseq [p ["reserved_microusd" "settled_microusd"]]
        (let [res (north.coord/send-op port {:op :assert :te (at p) :p "cardinality" :r "single"})]
          (when-not (:ok res) (throw (ex-info (str "failed to declare " p " cardinality single: " (pr-str res)) {})))
          (println (str "  declared @" p " cardinality single  (schema)"))))
      ;; 2) create the budget entity (single-valued config facts).
      (doseq [[p v] [["kind" "spend-budget"] ["billing" "api"]
                     ["budget_cap_microusd" (str cap)]
                     ["lane_envelope_default_microusd" (str edef)]
                     ["lane_envelope_max_microusd" (str emax)]
                     ["burn_limit_microusd_per_hour" (str burn)]
                     ["budget_period" "month"]
                     ["layer1_confirmed" (str layer1)]]]
        (north.coord/put! port (at id) p v)
        (println (str "  @" id " " p " " v)))
      (println (str "created budget @" id " (cap $" (micro->usd cap) "/mo, envelope default $"
                    (micro->usd edef) " / max $" (micro->usd emax) ")"))
      (println "NOTE: set price_in_per_mtok + price_out_per_mtok (micro-USD/Mtok, per model family)")
      (println "      before reserving — admission fail-closes without fresh price facts."))))

(defn cmd-override [target args]
  (when (str/blank? target) (throw (ex-info "usage: north spend override <target> --add-usd N --until <iso8601> --reason \"...\"" {})))
  (let [f (parse-flags args)
        amount (require-pos-micro "--add-usd" (get f "add-usd"))
        until  (get f "until")
        reason (get f "reason")]
    (when (str/blank? (str reason)) (throw (ex-info "--reason is mandatory for an override" {})))
    (when (str/blank? (str until)) (throw (ex-info "--until <iso8601> is mandatory for an override" {})))
    (let [until-ms (try (.toEpochMilli (java.time.Instant/parse (str until)))
                        (catch Exception _ (throw (ex-info (str "--until must be an ISO-8601 instant (e.g. 2026-07-20T00:00:00Z): got " (pr-str until)) {}))))
          now-ms (System/currentTimeMillis)
          max-ms (+ now-ms (* 48 60 60 1000))]
      (when (<= until-ms now-ms) (throw (ex-info "--until must be in the future" {})))
      (when (> until-ms max-ms) (throw (ex-info "--until must be at most 48h from now (mandatory expiry cap)" {})))
      (when (budget-defect port target)
        (throw (ex-info (str "no complete budget for " target " — create it with `north spend init` first") {})))
      (let [fact (json/generate-string {"amount_microusd" amount "until_ms" until-ms
                                        "reason" (str reason)
                                        "created_by" (or (System/getenv "NORTH_AUTHOR") "unknown")})]
        (north.coord/append! port (at (budget-id target)) "spend_override" fact)
        (println (str "  @" (budget-id target) " spend_override " fact))
        (println (str "override active: +$" (micro->usd amount) " until " until " — " reason))))))

(defn print-budget-status [port target]
  (if-let [defect (budget-defect port target)]
    (println (str target ": INCOMPLETE — " defect))
    (let [cap   (parse-long (str (budget-single port target "budget_cap_microusd")))
          month (utc-month)
          period (period-id target month)
          reserved (counter port period "reserved_microusd")
          settled  (counter port period "settled_microusd")
          overrides (active-override-micro port target (System/currentTimeMillis))
          headroom (- (+ cap overrides) reserved settled)]
      (println (str target "  (period " month ")"))
      (println (str "  cap        $" (micro->usd cap)))
      (when (pos? overrides) (println (str "  overrides +$" (micro->usd overrides) " (unexpired)")))
      (println (str "  reserved   $" (micro->usd reserved) "   (@" period " reserved_microusd)"))
      (println (str "  settled    $" (micro->usd settled) "   (@" period " settled_microusd)"))
      (println (str "  HEADROOM   $" (micro->usd headroom))))))

(defn all-budget-targets [port]
  (->> (:ok (north.coord/send-op
             port {:op :query
                   :query {:find "b"
                           :rules [{:head {:rel "b" :args [{:var "b"}]}
                                    :body [{:rel "triple" :args [{:var "b"} "kind" "spend-budget"]}]}]}}))
       (map first)
       (map #(str/replace (str %) #"^@?spend-budget:" ""))
       sort))

(defn cmd-status [target]
  (if (str/blank? target)
    (let [targets (all-budget-targets port)]
      (if (empty? targets)
        (println "no spend budgets configured (create one with `north spend init`)")
        (doseq [t targets] (print-budget-status port t) (println))))
    (print-budget-status port target)))

;; machine verbs: one JSON line for the shelling caller
(defn cmd-reserve [target args]
  (let [f (parse-flags args)
        envelope (if-let [e (get f "envelope-microusd")]
                   (parse-long (str e))
                   (let [d (budget-single port target "lane_envelope_default_microusd")]
                     (when d (parse-long (str d)))))]
    (println (json/generate-string
              (if (nil? envelope)
                {:ok false :reason "budget-incomplete" :detail "no lane_envelope_default_microusd"}
                (reserve! port target envelope))))))

(defn cmd-settle [target args]
  (let [f (parse-flags args)
        period (or (get f "period") (period-id target (utc-month)))
        reserved-micro (parse-long (str (get f "reserved-microusd")))
        status (or (get f "status") "unknown")
        input (parse-long (str (or (get f "input-tokens") "0")))
        output (parse-long (str (or (get f "output-tokens") "0")))]
    (println (json/generate-string
              (settle! port target period reserved-micro
                       {:status status :input input :output output})))))

(defn cmd-headroom [target args]
  ;; read-only eligibility probe: does TARGET have headroom for one envelope?
  (let [f (parse-flags args)
        envelope (if-let [e (get f "envelope-microusd")]
                   (parse-long (str e))
                   (let [d (budget-single port target "lane_envelope_default_microusd")]
                     (when d (parse-long (str d)))))]
    (println (json/generate-string
              (if-let [defect (budget-defect port target)]
                {:ok false :reason "budget-incomplete" :detail defect}
                (let [cap (parse-long (str (budget-single port target "budget_cap_microusd")))
                      period (period-id target (utc-month))
                      reserved (counter port period "reserved_microusd")
                      settled (counter port period "settled_microusd")
                      overrides (active-override-micro port target (System/currentTimeMillis))
                      headroom (- (+ cap overrides) reserved settled)]
                  {:ok (>= headroom (or envelope 0)) :headroom headroom :envelope envelope}))))))

(defn -main [& args]
  (let [[verb target & rest] args]
    (try
      (case verb
        "init"     (cmd-init target rest)
        "status"   (cmd-status target)
        "override" (cmd-override target rest)
        "reserve"  (cmd-reserve target rest)
        "settle"   (cmd-settle target rest)
        "headroom" (cmd-headroom target rest)
        ("help" "--help" "-h" nil)
        (do (println "north spend — API-billing budget ledger")
            (println "  north spend init <target> --cap-usd X --envelope-default-usd Y --envelope-max-usd Z --burn-limit-usd-hr W --i-confirm-layer1 \"...\"")
            (println "  north spend status [target]")
            (println "  north spend override <target> --add-usd N --until <iso8601> --reason \"...\"")
            (println "  (reserve/settle/headroom are machine verbs used by the admission + terminal seams)"))
        (throw (ex-info (str "unknown verb: " verb) {})))
      (catch clojure.lang.ExceptionInfo e
        (binding [*out* *err*] (println (str "north spend: " (.getMessage e))))
        (System/exit 2)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
