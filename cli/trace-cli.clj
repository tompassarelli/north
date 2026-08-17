#!/usr/bin/env bb
;; trace-cli.clj — `north trace <agent-id>`: single-agent lifecycle diagnosis.
;;
;; READ-ONLY. Walks the INVARIANT SPINE checklist (workflow-map §2) for ONE id and
;; flags the FIRST failing stage, printing the exact confirm command per stage. It is
;; LINEAGE-AWARE and TERMINALITY-AWARE — an absence that is EXPECTED for a lineage is
;; marked `·` not `✗` (native sessions have partial identity; a cleanly FINISHED lane
;; legitimately has inactive presence). Managed dispatch and spawn lanes both require the
;; same committed identity projection. The verdict maps the failure to a
;; workflow-map F-mode (F1–F7) with the remedy.
;;
;;   ✓ present/healthy   ! incomplete proof   · expected-absent / n-a   ✗ genuine failure
;;   usage: north trace <agent-id>
(require '[clojure.java.io :as io]
         '[clojure.string :as str])
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/agent-provenance.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/lifecycle-projection.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/run-ledger.clj"))
(def cur-ver  north.coord/cur-ver)
(def resolved north.coord/resolved)
(def session-online? north.coord/session-online?)

(def NORTH (some-> (System/getProperty "babashka.file")
                   io/file .getCanonicalFile .getParentFile .getParentFile str))
(def PORT (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))
(def NOW (System/currentTimeMillis))
(def max-trace-run-candidates 128)
(def max-trace-query-rows 4096)
(def trace-agent-predicates north.lifecycle-projection/trace-agent-predicates)
(def forensic-run-header-predicates
  ["kind" "wire_run_id" "thread" "agent" "at" "outcome" "effort" "model_tier"
   "wire_ledger_version" "wire_version" "wire_ledger_status"
   "parent_run" "parent_thread" "run_coordinator" "capability_class"
   "wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence"
   "wire_terminal_event_id" "wire_ledger_sha256" "wire_run_lifecycle"
   "wire_termination_code"])

(def use-color? (some? (System/console)))
(defn c [code s] (if use-color? (str "\033[" code "m" s "\033[0m") (str s)))
(defn dim [s] (c "2" s)) (defn bold [s] (c "1" s)) (defn grn [s] (c "32" s))
(defn red [s] (c "31" s)) (defn ylw [s] (c "33" s)) (defn cyn [s] (c "36" s))

(defn iso->ms [s] (try (.toEpochMilli (java.time.Instant/parse (str s))) (catch Exception _ nil)))
(defn identity-route-detail [facts]
  (str "model=" (or (get facts "model") "?")
       " effort=" (or (get facts "effort") "?")))
(defn ago [ms] (if (nil? ms) "?"
  (let [s (quot ms 1000)]
    (cond (< s 60) (str s "s") (< s 3600) (str (quot s 60) "m")
          (< s 86400) (str (quot s 3600) "h") :else (str (quot s 86400) "d")))))

;; ---- per-id reads ------------------------------------------------------------
(defn shown-values [domain subjects]
  (if (seq subjects)
    (let [response (north.coord/show-many-in-domain PORT domain subjects)]
      (into {}
            (map
             (fn [[subject rows]]
               [subject
                (reduce
                 (fn [facts [predicate value]]
                   (update facts predicate (fnil conj []) value))
                 {}
                 rows)]))
            (:rows response)))
    {}))

(defn agent-facts [id]
  (let [subject (str "@agent:" id)
        values (get (shown-values :coordination [subject]) subject {})]
    (north.lifecycle-projection/folded-agent-point-facts
     (fn [_ predicate] (get values predicate []))
     subject
     trace-agent-predicates)))
(defn online-session? [id] (session-online? PORT id))
(defn q [project body]
  (:rows
   (north.coord/bounded-query
    PORT
    {:find "row"
     :rules [{:head {:rel "row" :args (mapv (fn [v] {:var v}) project)}
              :body body}]}
    max-trace-query-rows)))

;; ---- forensic AgentRun ledger ------------------------------------------------
(defn point-facts [domain subject predicates]
  (let [shown (get (shown-values domain [subject]) subject {})]
    (into {}
          (keep (fn [predicate]
                  (let [values (vec (distinct (get shown predicate [])))]
                    (when (= 1 (count values)) [predicate (first values)]))))
          predicates)))

(defn run-event-entries [wire-run]
  (let [response (north.coord/bounded-query-in-domain
                  PORT
                  :telemetry
                  {:find "forensic_run_event"
                   :rules [{:head {:rel "forensic_run_event" :args [{:var "e"}]}
                            :body [{:rel "triple" :args [{:var "e"} "wire_run_id" wire-run]}
                                   {:rel "triple" :args [{:var "e"} "kind" "wire_event"]}]}]}
                  north.run-ledger/max-events)]
    (let [subjects (mapv first (:rows response))
          shown (shown-values :telemetry subjects)]
      (mapv
       (fn [subject]
         (let [values (get shown subject {})
               facts (mapcat (fn [predicate]
                               (map (fn [value] [predicate value])
                                    (get values predicate [])))
                             north.run-ledger/event-predicates)]
           (north.run-ledger/validate-event-facts! subject facts)))
       subjects))))

(defn thread-run-ids [thread-id]
  (let [canonical-thread (north.run-ledger/canonical-entity thread-id "thread")
        response (north.coord/bounded-query-in-domain
                  PORT
                  :telemetry
                  {:find "forensic_thread_run"
                   :rules [{:head {:rel "forensic_thread_run" :args [{:var "e"}]}
                            :body [{:rel "triple" :args [{:var "e"} "thread" canonical-thread]}
                                   {:rel "triple" :args [{:var "e"} "kind" "run"]}]}]}
                  128)]
    (->> (:rows response) (map first) distinct sort vec)))

(defn forensic-run [run-id header events]
  (assoc (north.run-ledger/timeline
          (or (get header "wire_run_id") (str/replace run-id #"^@" ""))
          events)
         :header header))

(defn unknown-label [value source]
  (or value (str "unknown (source coverage " (or source "unavailable") ")")))

(defn render-forensic-run [{:keys [run thread agent parent-thread coordinator
                                   events valid-order? finalized? digest header]}]
  (let [lineage-source (when (or parent-thread coordinator) "exact")
        header-digest (get header "wire_ledger_sha256")]
    (str/join
     "\n"
     (concat
      [(str "run " run)
       (str "  thread: " (unknown-label thread nil))
       (str "  agent: " (unknown-label agent nil))
       (str "  parent thread: " (unknown-label parent-thread lineage-source))
       (str "  coordinator: " (unknown-label coordinator lineage-source))
       (str "  ledger order: " (if valid-order? "exact" "invalid"))
       (str "  ledger finalization: " (if finalized? "exact" "incomplete"))
       (str "  header digest: " (cond
                                  (nil? header-digest) "unavailable"
                                  (= header-digest digest) "consistent"
                                  :else "invalid"))]
      (map (fn [event]
             (format "  %08d %-22s essential=%s at=%s id=%s"
                     (get event "sequence") (get event "kind")
                     (get event "essential") (get event "at") (get event "id")))
           events)))))

(defn live-forensic-run [run-id]
  (let [canonical-run (north.run-ledger/canonical-entity run-id "run")
        header (point-facts :telemetry canonical-run forensic-run-header-predicates)
        wire-run (or (get header "wire_run_id") (subs canonical-run 1))]
    (forensic-run canonical-run header (run-event-entries wire-run))))

(defn forensic-main! [kind selector]
  (let [run-ids (if (= kind :run) [selector] (thread-run-ids selector))]
    (println (str (bold "north trace ") selector "  ·  Wire ledger v2"))
    (if (seq run-ids)
      (doseq [run-id run-ids]
        (println)
        (println (render-forensic-run (live-forensic-run run-id))))
      (println "no durable wire events observed"))))

(defn owned-concerns [id]
  (let [subjects (->> (q ["e"] [{:rel "triple" :args [{:var "e"} "kind" "concern"]}
                                  {:rel "triple" :args [{:var "e"} "agent" (str "@" id)]}])
                      (map first)
                      vec)
        shown (shown-values :coordination subjects)]
    (map
     (fn [subject]
       (let [facts (get shown subject {})
             reached (set (get facts "reached" []))]
         {:id subject
          :status (cond (reached "landed") "landed"
                        (reached "abandoned-stale") "abandoned-stale"
                        (reached "likely-to-land") "likely-to-land"
                        (reached "building") "building"
                        :else "?")
          :repo (first (get facts "repo" []))}))
     subjects)))

(defn agent-run-entries [id]
  (let [response
        (north.coord/bounded-query-in-domain
         PORT
         :telemetry
         {:find "trace_run_candidate"
          :rules
          [{:head {:rel "trace_run_candidate" :args [{:var "e"}]}
            :body [{:rel "triple"
                    :args [{:var "e"} "agent" id]}]}]}
         max-trace-run-candidates)
        rows (:rows response)]
    (when (and (vector? rows)
               (every? #(and (vector? %) (= 1 (count %))
                             (string? (first %)))
                       rows))
      (let [subjects (->> rows
                          (map first)
                          (filter north.terminal-projection/valid-run-entity?)
                          distinct
                          sort
                          vec)
            shown (shown-values :telemetry subjects)
            predicates (conj (vec north.terminal-projection/run-resolution-predicates)
                             "provider_error_detail")]
        (mapv
         (fn [subject]
           (let [values (get shown subject {})]
             {:subject subject
              :facts
              (into {}
                    (keep
                     (fn [predicate]
                       (let [members (set (get values predicate []))]
                         (when (seq members) [predicate members]))))
                    predicates)}))
         subjects)))))

(defn provider-error-detail
  "Return the latest run's provider failure detail, ordered by its `at` fact."
  [run-entries]
  (->> run-entries
       (keep (fn [{:keys [subject facts]}]
               (when-let [detail (first (get facts "provider_error_detail"))]
                 {:at (some-> (north.terminal-projection/singleton-value facts "at")
                              iso->ms)
                  :detail detail})))
       (sort-by #(or (:at %) 0))
       last
       :detail))

(defn agent-runs [run-entries]         ; [{:outcome :ms} ...] display history
  (->> run-entries
       (keep (fn [{:keys [facts]}]
               (when-let [ms (some->
                              (north.terminal-projection/singleton-value facts "at")
                              iso->ms)]
                 {:outcome
                  (north.terminal-projection/committed-run-process-outcome facts)
                  :ms ms})))
       (sort-by #(or (:ms %) 0))))

(defn deaths-for [id]                 ; agent_death lines on @swarm mentioning this id
  (->> (get-in (shown-values :coordination ["@swarm"]) ["@swarm" "agent_death"] [])
       (filter #(str/starts-with? (str %) (str id " ")))
       (map (fn [line] (let [[_ reason ts] (map str/trim (str/split (str line) #"\|" 3))]
                         {:reason reason :ms (iso->ms ts)})))))

(defn inbox-to [id]
  (count (q ["e"] [{:rel "triple" :args [{:var "e"} "to" id]}])))

(defn execution-terminal-state
  "Resolve execution truth without promoting a death notification into a
  terminal. Any lane terminal evidence owns the decision: a partial/conflicting
  lane projection fails closed and cannot fall through to a secondary run
  trail. A committed run is consulted only when the lane has no terminal body."
  [control facts run-entries deaths]
  (let [resolution
        (if (vector? run-entries)
          (north.terminal-projection/lane-resolution control facts run-entries)
          {:status :indeterminate :reason :run-projection-unavailable})
        outcome (when (= :resolved (:status resolution)) (:outcome resolution))]
    {:outcome outcome
     :source (:source resolution)
     :resolution-status (:status resolution)
     :resolution-reason (:reason resolution)
     :facts (:facts resolution)
     :terminal? (boolean outcome)
     :kind (cond
             (= "ran" outcome) :ran
             (= "died" outcome) :died
             (= "died-unreported" outcome) :died-unreported
             outcome :stopped
             :else nil)
     :death-notifications (count deaths)}))

(defn terminal-delivery-state
  "Expose delivery only from the same committed projection that established
  process truth."
  [facts terminal-state]
  (when (:terminal? terminal-state)
    (let [terminal-facts (or (:facts terminal-state) facts)]
      {:outcome (or (north.terminal-projection/singleton-value
                   terminal-facts "delivery_outcome")
                  "unrecorded")
       :reason (north.terminal-projection/singleton-value
                terminal-facts "delivery_reason")})))

(defn terminal-summary [terminal-state delivery-state]
  (str "process=" (:outcome terminal-state)
       " · delivery=" (or (:outcome delivery-state) "unrecorded")
       (when-let [reason (:reason delivery-state)]
         (str " (" reason ")"))))

(defn delivery-proof-class [delivery-state]
  (case (or (:outcome delivery-state) "unrecorded")
    "reported" :reported
    "unverified" :incomplete
    "unrecorded" :incomplete
    "blocked" :blocked
    :inconsistent))

(defn trace-verdict
  [{:keys [id on-roster terminal-state delivery-state online lineage
           identity-complete deaths]}]
  (let [terminal? (:terminal? terminal-state)
        terminal-kind (:kind terminal-state)
        delivery-class (delivery-proof-class delivery-state)
        summary (terminal-summary terminal-state delivery-state)]
    (cond
      (not on-roster)
      (red "F4 — not on any roster: a zombie fork, a bad id, or an unmanaged actor. Confirm via git author vs `north agents`.")
      (= :indeterminate (:resolution-status terminal-state))
      (red (str "lifecycle evidence is inconsistent ("
                (name (:resolution-reason terminal-state))
                "); this lane is neither active nor finished. Repair the lane/run projection before messaging or cleanup."))
      (= terminal-kind :died)
      (str (red (str "F1 — API-death mid-lane; " summary "."))
           " agent_death recorded. Remedy: re-dispatch the thread (idempotent); for chronic deaths re-dispatch at the next tier per the D2 execution-axis move; read the partial result first.")
      (= terminal-kind :died-unreported)
      (str (red (str "F3 — silent death; " summary "."))
           " The lease/telemetry missed the death; trust the lifecycle-janitor verdict.")
      (= terminal-kind :stopped)
      (str (red (str "terminal execution did not succeed; " summary "."))
           (when online " The still-live lease is stale presence, not evidence of healthy execution."))
      (and (seq deaths) (not terminal?))
      (str (red "F1/F3 — death notification received but execution remains unresolved.")
           " A notification is diagnostic only; require a committed lane terminal or committed run before treating the lane as finished.")
      (and on-roster (not terminal?) (not online))
      (str (red "F2/F3 — offline with NO completion signal.")
           " Presence is inactive; the lane lifecycle janitor must resolve it as died-unreported (confirm: `north show @agent:"
           id "` for outcome=died-unreported).")
      (and (= lineage :sdk-lane) (not identity-complete))
      (red "F6 — SDK-lane missing identity facts: possible id-collision/aliasing, or writeAgentFacts failed. Check `north show @agent:<id>` for contradictory repos/goals.")
      (and (= terminal-kind :ran) (= delivery-class :reported))
      (grn (str "execution succeeded; " summary
                ". Delivery is evidence-backed same-UID self-report, not independent verification"
                (if online "; presence remains active." "; presence is inactive as expected.")))
      (and (= terminal-kind :ran) (= delivery-class :incomplete))
      (ylw (str "execution succeeded but delivery proof is incomplete; " summary
                ". This is not a done claim"
                (if online "; presence remains active." "; presence is inactive as expected.")))
      (= terminal-kind :ran)
      (red (str "terminal inconsistency; " summary
                ". A ran process with blocked or inconsistent delivery is not a done claim"
                (if online "; presence remains active." ".")))
      online
      (grn "healthy — online and advancing (no terminal signal yet). No failure.")
      :else (dim "no failing stage detected."))))

;; ---- render one stage line ---------------------------------------------------
(defn stage [n mark label detail cmd]
  (let [g (case mark :ok (grn "✓") :warn (ylw "!") :na (dim "·") :fail (red "✗"))]
    (format "%s %-11s %s %-46s %s" g label (str "") (str detail) (dim cmd))))

(defn -main [args]
  (let [raw (first args)]
    (when (str/blank? raw)
      (println (red "usage:") "north trace <agent-id|run:ID|thread:ID>") (System/exit 2))
    (let [run-selector (when (re-find #"^@?run:" raw)
                         (north.run-ledger/canonical-entity raw "run"))
          thread-selector (when (str/starts-with? raw "thread:")
                            (north.run-ledger/canonical-entity
                             (subs raw (count "thread:")) "thread"))]
      (when (or run-selector thread-selector)
        (let [probe (try (cur-ver PORT) (catch Exception _ ::down))]
          (when (= probe ::down)
            (println (red (str "north trace — Beagle Store server :" PORT " unreachable")))
            (System/exit 1))
          (forensic-main! (if run-selector :run :thread)
                          (or run-selector thread-selector))
          (System/exit 0))))
    (let [id (str/replace raw #"^@?(agent:)?" "")
          probe (try (cur-ver PORT) (catch Exception _ ::down))]
      (when (= probe ::down)
        (println (red (str "north trace — Beagle Store server :" PORT " unreachable"))) (System/exit 1))
      (let [facts (agent-facts id)
            kind (get facts "kind")
            online (online-session? id)
            sess-agent (resolved PORT (str "@session:" id) "agent")
            on-roster (boolean (or kind sess-agent online))
            ;; managed identity is valid only after exact readback + marker commit
            identity-defects (when (= kind "lane")
                               (north.agent-provenance/identity-defects facts))
            idfull (and (= kind "lane") (empty? identity-defects))
            ;; lineage
            lineage (cond (= kind "session") :session
                          (= kind "lane")    :sdk-lane
                          (= kind "cron")    :cron
                          (and on-roster (nil? kind)) :corrupt-managed
                          :else :unknown)
            id-expect (case lineage :sdk-lane "committed full projection"
                            :session "partial native (kind+repo)"
                            :corrupt-managed "CORRUPT (rostered without kind/manifest)"
                            :cron "partial" "unknown")
            ;; work
            concerns (owned-concerns id)
            active-concern (first (filter #(= (:status %) "building") concerns))
            ;; completion / death
            run-entries (agent-run-entries id)
            runs (agent-runs run-entries)
            last-run (last runs)
            deaths (deaths-for id)
            terminal-state (execution-terminal-state id facts run-entries deaths)
            terminal? (:terminal? terminal-state)
            terminal-kind (:kind terminal-state)
            delivery-state (terminal-delivery-state facts terminal-state)
            inbox (inbox-to id)]
        ;; header
        (println (str (bold "north trace ") (bold id) "  ·  :" PORT))
        (println (str "lineage  " (name lineage) "   " (dim (str "(expects: " id-expect ")"))))
        (println)
        ;; 1 ROSTER
        (println (stage 1 (if on-roster :ok :fail) "1 ROSTER"
                        (if on-roster (str "on roster (" id ")")
                            (red "NOT on roster — no identity / session presence"))
                        "north agents"))
        ;; 2 IDENTITY
        (let [mark (cond idfull :ok
                         (= lineage :sdk-lane) :fail
                         (= lineage :corrupt-managed) :fail
                         :else :na)
              provenance (north.agent-provenance/provenance-detail facts)
              detail (cond idfull (str "kind=" kind " role=" (get facts "role")
                                       " " (identity-route-detail facts)
                                       " " (:label provenance)
                                       (when-let [co (get facts "coordinator")] (str " coord=" co)))
                           (= lineage :sdk-lane) (str "CORRUPT: " (str/join ", " identity-defects))
                           (= kind "session") (str "kind=session repo=" (or (get facts "repo") "?")
                                                   " orchestration:not-selected (native — expected)")
                           (= lineage :corrupt-managed) "CORRUPT: roster evidence without managed identity kind"
                           :else "absent")]
          (println (stage 2 mark "2 IDENTITY" detail (str "north show @agent:" id)))
          (when (= lineage :sdk-lane)
            (println (str "    composition  " (:label provenance)))
            (case (:kind provenance)
              "preset" (when (seq (:overrides provenance))
                         (println (str "    override     " (str/join "," (:overrides provenance))
                                       " · why: " (:override-reason provenance))))
              "bespoke" (do
                          (println (str "    why          " (or (:why provenance) "MISSING")))
                          (when-let [nearest (:nearest-reference-only provenance)]
                            (println (str "    nearest      orchestration:" nearest " (reference only; no inherited authority)")))
                          (println (str "    promotion    " (or (:promotion-candidate provenance) "MISSING")))
                          (println (str "    contract     sha256:" (or (:contract-sha256 provenance) "MISSING"))))
              nil)))
        ;; 3 PRESENCE
        (let [mark (cond online :ok terminal? :na :else :fail)
              detail (cond online (grn "ONLINE")
                           terminal? "inactive (finished — expected)"
                           :else (red "no live session"))]
          (println (stage 3 mark "3 PRESENCE" detail "north agents")))
        ;; 4 WORK
        (let [mark (if (seq concerns) :ok :na)
              detail (if active-concern
                       (str "concern " (:status active-concern) " [" (or (:repo active-concern) "?") "]")
                       (if (seq concerns) (str (count concerns) " concern(s)") "no concern"))]
          (println (stage 4 mark "4 WORK" detail
                          (if active-concern (str "concern ls " (or (:repo active-concern) "")) (str "north watch " id)))))
        ;; 5 MSG
        (println (stage 5 :na "5 MSG"
                        (if (pos? inbox) (str inbox " message(s) addressed to it") (dim "none sent"))
                        (str "bb " NORTH "/cli/msg-cli.clj " PORT " inbox " id)))
        ;; 6 COMPLETION / DEATH
        (let [death-notification (last deaths)
              proof-class (delivery-proof-class delivery-state)
              mark (cond (and (= terminal-kind :ran) (= proof-class :reported)) :ok
                         (and (= terminal-kind :ran) (= proof-class :incomplete)) :warn
                         (= terminal-kind :ran) :fail
                         terminal-kind :fail
                         (= :indeterminate (:resolution-status terminal-state)) :fail
                         death-notification :fail
                         online :na
                         :else :fail)
              detail (case terminal-kind
                       :ran (let [summary (str (terminal-summary terminal-state delivery-state)
                                               (when last-run (str " " (ago (- NOW (:ms last-run))) " ago")))]
                              (case proof-class
                                :reported (grn summary)
                                :incomplete (ylw summary)
                                (red summary)))
                       :died (str (red (terminal-summary terminal-state delivery-state))
                                  (when death-notification
                                    (str " · notification: \"" (:reason death-notification) "\"")))
                       :died-unreported
                       (red (str (terminal-summary terminal-state delivery-state)
                                 " (lifecycle-reaped silent death)"))
                       :stopped (red (terminal-summary terminal-state delivery-state))
                       (cond
                         (= :indeterminate (:resolution-status terminal-state))
                         (red (str "lifecycle evidence inconsistent: "
                                   (name (:resolution-reason terminal-state))))
                         death-notification
                         (str (red "agent_death notification without committed terminal")
                              ": \"" (:reason death-notification) "\"")
                         online (dim "still running — no terminal signal yet")
                         :else (red "NO committed completion terminal (offline, unrecorded)")))]
          (println (stage 6 mark "6 COMPLETION" detail "north show @swarm"))
          (when (= mark :fail)
            (when-let [cause (provider-error-detail run-entries)]
              (println (str "    " (dim "cause  ") (red cause))))))
        ;; 7 REAPING
        (let [stale-concern (first (filter #(and (= (:status %) "building")) concerns))
              detail (str (cond online "live — not reaped"
                                terminal? (str "presence inactive" (when (= terminal-kind :died-unreported) " · lifecycle reaped"))
                                :else "presence inactive — awaiting lifecycle-janitor verdict")
                          (when (and stale-concern (not online))
                            (ylw (str " · concern still " (:status stale-concern) " (STALE)"))))]
          (println (stage 7 :na "7 REAPING" detail "north agents / concern ls")))
        (println)
        ;; ---- verdict (first genuine failure + F-mode) ----
        (let [verdict
              (trace-verdict
               {:id id :on-roster on-roster :terminal-state terminal-state
                :delivery-state delivery-state :online online
                :lineage lineage :identity-complete idfull :deaths deaths})]
          (println (str (bold "verdict: ") verdict)))))
    (System/exit 0)))

(when-not (= "1" (System/getProperty "north.trace.lib"))
  (try (-main (vec *command-line-args*))
       (catch Throwable t
         (binding [*out* *err*] (println (str "north trace: " (.getMessage t))))
         (System/exit 1))))
