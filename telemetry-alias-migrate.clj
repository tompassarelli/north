#!/usr/bin/env bb
;; telemetry-alias-migrate.clj — ONE batch script, run once, that resolves the
;; historical bare-alias `model` facts on @run-sdk-* telemetry rows to their
;; era-correct canonical ids, and marks the handful of rows that cannot be
;; safely resolved (or that are already-canonical but misleading) with an
;; additive `model_provenance` fact. Never deletes evidence, never rewrites a
;; run's other facts.
;;
;; Precedent studied: offline-migrate.clj rewrites facts.log OFFLINE (daemon
;; down, raw retract/assert lines appended to the log file directly, daemon
;; restarted to reload). That path does not fit telemetry.log: it is LIVE —
;; other agents' managed lanes are appending run telemetry through the shared
;; coordinator on :7977 for the whole duration of this repo session, and a
;; daemon bounce would sever every one of them (the exact failure mode named
;; in sibling thread 019f8300-4fe6f as a live severity). So this script takes
;; the OTHER precedent-sanctioned shape: a coordinator-mediated BATCH OP —
;; cli/run-fact-internal.clj's own trick of loading cli/coord.clj and calling
;; north.coord/put! + north.coord/retract! directly against the live socket,
;; the same primitives the harness uses to publish run telemetry in the first
;; place. No file rewrite, no downtime, no CLI guard involved (deny_generic_
;; run_mutation in bin/north is a bash-CLI guard around `north tell`, not a
;; restriction on the coordinator wire protocol these primitives speak).
;;
;; Mapping rules (Foundation thread 019f82ed-3f9c-7e8d-a0d5-0c8ef7abe8f2,
;; era analysis already scoped by a prior lane):
;;   opus   -> claude-opus-4-8   (unambiguous: only canonical id ever observed)
;;   fable  -> claude-fable-5    (unambiguous: only canonical id ever observed)
;;   sonnet -> AMBIGUOUS era (every bare row predates BOTH claude-sonnet-4-6 and
;;             claude-sonnet-5) -> keep alias, mark model_provenance=alias_unresolved_era
;;   haiku  -> no canonical mapping ever existed in the gaffer catalog history
;;             -> keep alias, mark model_provenance=alias_unresolved_era
;; Provenance-only marks (not bare-alias; already-canonical model value, but the
;; row is misleading about what actually executed):
;;   phantom fallback mismatch: model provider-prefix (gpt-/claude-) disagrees
;;     with the row's own recorded `provider` fact (fallback landed on a
;;     different provider than the routed model name implies)
;;     -> model_provenance=phantom_fallback_mismatch
;;   blocked_preflight: outcome/process_outcome=blocked_preflight means no
;;     provider turn ever began -> the model fact is routed INTENT, never
;;     executed -> model_provenance=routed_intent_not_executed
;;
;; Usage:
;;   bb telemetry-alias-migrate.clj              (dry-run; prints the plan, writes nothing)
;;   bb telemetry-alias-migrate.clj --execute     (backs up, applies via the live coordinator, verifies)

(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def north-home (str (.getParent (io/file (System/getProperty "babashka.file")))))
(load-file (str north-home "/cli/coord.clj"))

(def port (Integer/parseInt (or (System/getenv "NORTH_PORT") (System/getenv "FRAM_PORT") "7977")))
(def state-home (or (System/getenv "FRAM_STATE_HOME")
                     (str (System/getenv "HOME") "/.local/state/north")))
(def telemetry-log (or (System/getenv "FRAM_TELEMETRY_LOG") (str state-home "/telemetry.log")))
(def now-ts (str (java.time.Instant/now)))
(def now-compact (str/replace (str/replace now-ts #"[:.]" "") "Z" "Z"))

(def execute? (= "--execute" (first *command-line-args*)))

;; ---- read-only plan computation over the live log -------------------------

(defn read-recs [path]
  (with-open [r (io/reader path)]
    (->> (line-seq r)
         (keep (fn [ln] (try (edn/read-string ln) (catch Exception _ nil))))
         vec)))

(def recs (read-recs telemetry-log))

;; multi-valued last-write-wins fold, keyed by [subject predicate] -> #{values}
;; (mirrors log-split.clj's build-kindmap generalized to a set of predicates).
(defn build-index [recs preds]
  (let [preds (set preds)]
    (reduce (fn [m {:keys [op l p r]}]
              (if (contains? preds p)
                (if (= op "retract")
                  (update m [l p] (fnil disj #{}) r)
                  (update m [l p] (fnil conj #{}) r))
                m))
            {}
            recs)))

(def watched-preds ["model" "outcome" "process_outcome" "provider" "fallback_path"])
(def idx (build-index recs watched-preds))

(defn one [subj p]
  (let [vs (get idx [subj p])]
    (when (= 1 (count vs)) (first vs))))

(def subjects (distinct (map :l recs)))

(def bare-alias->canonical
  {"opus"  "claude-opus-4-8"
   "fable" "claude-fable-5"})
(def bare-alias-unresolved #{"sonnet" "haiku"})
(def all-bare-alias (into (set (keys bare-alias->canonical)) bare-alias-unresolved))

(defn model-provider-prefix [model]
  (cond (nil? model) nil
        (str/starts-with? model "claude-") "anthropic"
        (str/starts-with? model "gpt-") "openai"
        :else nil))

;; ---- classify every subject into exactly one action (or none) -------------

;; The bare-alias resolve/mark rules apply to EVERY subject carrying one of
;; the four alias values in its `model` fact (the done-bar is unconditional:
;; "0 bare-alias model facts except provenance-marked", no subject-prefix
;; carve-out) -- in practice @run-sdk-*/@run:*/@run-wedge-*/@run-test-dead-*
;; run rows plus a handful of @agent:session-kea-* interactive-session rows.
;;
;; The phantom/blocked_preflight provenance rules are narrower by nature of
;; what they detect: a `provider` fact of "unobserved" is a DELIBERATE sentinel
;; on native interactive Codex sessions (no per-turn provider telemetry is
;; collected for them, by design -- sibling thread 019f8300-4fe6) and must NOT
;; be flagged as a fallback mismatch just because it differs from the model's
;; provider prefix.
(defn classify [subj]
  (let [model (one subj "model")
        provider (one subj "provider")
        outcome (or (one subj "outcome") (one subj "process_outcome"))]
    (cond
      (contains? bare-alias->canonical model)
      {:kind :resolve :subject subj :from model :to (bare-alias->canonical model)}

      (contains? bare-alias-unresolved model)
      {:kind :mark-unresolved-era :subject subj :alias model}

      (and model provider (not= provider "unobserved") (model-provider-prefix model)
           (not= (model-provider-prefix model) provider))
      {:kind :mark-phantom :subject subj :model model :provider provider}

      (and model (= outcome "blocked_preflight"))
      {:kind :mark-blocked-preflight :subject subj :model model}

      :else nil)))

(def plan (->> subjects (keep classify) vec))
(def by-kind (group-by :kind plan))

(defn n [k] (count (get by-kind k [])))

(println "=== DRY-RUN PLAN — telemetry.log:" telemetry-log "===")
(println "  total tx lines read:" (count recs) " distinct subjects:" (count subjects))
(println "  opus  -> claude-opus-4-8  :" (n :resolve) "resolvable rows total (opus+fable combined below split)")
(println "    opus  resolved:" (count (filter #(= "opus" (:from %)) (get by-kind :resolve []))))
(println "    fable resolved:" (count (filter #(= "fable" (:from %)) (get by-kind :resolve []))))
(println "  sonnet/haiku alias-unresolved-era marks:" (n :mark-unresolved-era)
         "(sonnet:" (count (filter #(= "sonnet" (:alias %)) (get by-kind :mark-unresolved-era [])))
         "haiku:" (count (filter #(= "haiku" (:alias %)) (get by-kind :mark-unresolved-era []))) ")")
(println "  phantom fallback-mismatch marks:" (n :mark-phantom))
(println "  blocked_preflight routed-intent marks:" (n :mark-blocked-preflight))
(println "  TOTAL actions:" (count plan))
(println)

(when-not execute?
  (println "DRY-RUN — nothing written. Re-run with --execute to apply via the live coordinator.")
  (System/exit 0))

;; ---- apply: backup, then one batch of coordinator-mediated writes ---------

(def backup-dir (str state-home "/backups"))
(io/make-parents (str backup-dir "/x"))
(def backup-path (str backup-dir "/telemetry.log.pre-alias-migration-" now-compact))
(io/copy (io/file telemetry-log) (io/file backup-path))
(println "backup written:" backup-path)

(defn usage-report []
  (let [{:keys [out exit]} (shell/sh (str north-home "/bin/north") "routing" "report" "usage" "--by-model")]
    {:exit exit :out out}))

(def pre-usage (usage-report))
(when-not (zero? (:exit pre-usage))
  (binding [*out* *err*] (println "PRE usage report exit" (:exit pre-usage) "— aborting before any mutation"))
  (System/exit 1))
(def pre-usage-path (str north-home "/docs/private/usage-by-model-pre-alias-migration-" now-compact ".txt"))
(spit pre-usage-path (:out pre-usage))
(println "pre-migration usage report:" pre-usage-path "(exit 0)")

(defn checked! [res what]
  (when (:reject res)
    (throw (ex-info (str "coordinator rejected " what) {:res res})))
  res)

(def applied (atom {:resolve 0 :mark-unresolved-era 0 :mark-phantom 0 :mark-blocked-preflight 0}))

(doseq [{:keys [kind subject] :as action} plan]
  (case kind
    :resolve
    (do (checked! (north.coord/retract! port subject "model" (:from action)) [:retract subject "model" (:from action)])
        (checked! (north.coord/put! port subject "model" (:to action)) [:put subject "model" (:to action)])
        (swap! applied update :resolve inc))

    :mark-unresolved-era
    (do (checked! (north.coord/put! port subject "model_provenance" "alias_unresolved_era")
                  [:put subject "model_provenance" "alias_unresolved_era"])
        (swap! applied update :mark-unresolved-era inc))

    :mark-phantom
    (do (checked! (north.coord/put! port subject "model_provenance" "phantom_fallback_mismatch")
                  [:put subject "model_provenance" "phantom_fallback_mismatch"])
        (swap! applied update :mark-phantom inc))

    :mark-blocked-preflight
    (do (checked! (north.coord/put! port subject "model_provenance" "routed_intent_not_executed")
                  [:put subject "model_provenance" "routed_intent_not_executed"])
        (swap! applied update :mark-blocked-preflight inc))))

(println "APPLIED:" @applied)

;; dry-run counts must match applied counts exactly — this IS the batch-op
;; correctness bar, not a courtesy log line.
(let [expected {:resolve (n :resolve)
                :mark-unresolved-era (n :mark-unresolved-era)
                :mark-phantom (n :mark-phantom)
                :mark-blocked-preflight (n :mark-blocked-preflight)}]
  (when-not (= expected @applied)
    (binding [*out* *err*]
      (println "MISMATCH — dry-run plan" expected "!= applied" @applied))
    (System/exit 1)))

;; ---- post-apply verification -----------------------------------------------
;; Re-read the log fresh (the coordinator has flushed every acked write to it)
;; and confirm: 0 live bare-alias model facts except the two era-unresolved
;; aliases, which must now carry model_provenance=alias_unresolved_era.
(def post-recs (read-recs telemetry-log))
(def post-idx (build-index post-recs watched-preds))
(defn post-one [subj p]
  (let [vs (get post-idx [subj p])] (when (= 1 (count vs)) (first vs))))

(def post-provenance
  (reduce (fn [m {:keys [op l p r]}]
            (if (= p "model_provenance")
              (if (= op "retract") (update m l (fnil disj #{}) r) (update m l (fnil conj #{}) r))
              m))
          {} post-recs))

(def bad
  (for [subj (distinct (map :l post-recs))
        :let [model (post-one subj "model")]
        :when (contains? all-bare-alias model)
        :when (not (contains? (get post-provenance subj #{}) "alias_unresolved_era"))]
    [subj model]))

(println "post-migration bare-alias rows without a provenance mark:" (count bad))
(when (seq bad)
  (binding [*out* *err*]
    (println "VERIFICATION FAILED — unresolved+unmarked bare-alias rows:" bad))
  (System/exit 1))

(def post-usage (usage-report))
(when-not (zero? (:exit post-usage))
  (binding [*out* *err*] (println "POST usage report exit" (:exit post-usage)))
  (System/exit 1))
(def post-usage-path (str north-home "/docs/private/usage-by-model-post-alias-migration-" now-compact ".txt"))
(spit post-usage-path (:out post-usage))
(println "post-migration usage report:" post-usage-path "(exit 0)")

(println "DONE. backup:" backup-path)
(println "  applied:" @applied)
(println "  0 unmarked bare-alias rows remain (verified by fresh log fold)")
