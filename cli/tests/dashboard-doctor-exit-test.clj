#!/usr/bin/env bb
(require '[babashka.process :as p]
         '[clojure.string :as str]
         '[clojure.java.io :as io])

(def test-script (or (System/getProperty "babashka.file") *file*))

;; dashboard-cli's library guard is process-environment based because its public
;; entrypoint is also the executable. Re-enter once with the guard set.
(when-not (= "1" (System/getenv "NORTH_DASHBOARD_LIB"))
  (let [result @(p/process ["env" "NORTH_DASHBOARD_LIB=1" "bb" test-script]
                           {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))

(def root (-> test-script io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(let [dashboard-script (str root "/cli/dashboard-cli.clj")]
  ;; dashboard-cli resolves its sibling sources through babashka.file. Preserve
  ;; the test entrypoint for recursive assertions, but give the loaded executable
  ;; the same source identity it has when invoked through `north doctor`.
  (System/setProperty "babashka.file" dashboard-script)
  (try
    (load-file dashboard-script)
    (finally
      (System/setProperty "babashka.file" test-script))))

(def healthy-coordination-probe
  {:version "north:coordination-probe:v1"
   :expected_log "/data/coordination.log"
   :served_log "/data/coordination.log"
   :log_fence_ok true
   :lease_write_readback_ok true
   :live_session_leases 3
   :lineage_registrations_in_ttl 4
   :lease_ttl_ms 1800000})

(def healthy-activation-health
  {:version "north:rebuild-activation-health:v1"
   :nowMs 1000000
   :coordinationOn false
   :windowSeconds 3600
   :unreadOlder 0
   :openCount 0
   :open []
   :gauge {:count 0}
   :urgent {:total 0 :urgent 0 :rate 0.0 :periodHours 24}
   :lastWindow nil
   :promote {:available false
             :path "/var/lib/north-enforcement/active/current"
             :note "promote infra not yet deployed"}})

(defn exercise-doctor
  ([failed?] (exercise-doctor failed? []))
  ([failed? dead-letters]
   (exercise-doctor failed? dead-letters (fn [_ _] {:available true :behind 0 :dirty-files 0})))
  ([failed? dead-letters drift] (exercise-doctor failed? dead-letters drift {}))
  ([failed? dead-letters drift coordination]
  (with-redefs [coordination-probe
                (fn [] (get coordination :probe healthy-coordination-probe))
                activation-health-probe
                (fn [] (get coordination :activation healthy-activation-health))
                roster-projection-probe
                (fn [] (get coordination :roster {:entries 3}))
                coord-safety-probe
                (fn [] (if failed?
                         {:ok false
                          :err "coordinator runtime identity UNHEALTHY — stale source"
                          :timeout-ms 100}
                         {:ok true :out "coordinator runtime identity OK\n" :err ""}))
                daemon-health (fn [] {:north true})
                maintenance-doctor-lines (fn [_] ["[ok]  maintenance current"])
                cache-get (fn [& _] {:lanes-ran-24h 1
                                     :lanes-died-24h 0})
                source-revision (fn [_ _] {:revision "test-rev" :origin "checkout HEAD"})
                deployment-drift drift
                north.message-routing/readiness-dead-letter-scan
                (fn [& _] {:rows dead-letters})
                run (fn [& _] {:ok true :out "/nix/store/test-runtime/bin/tool\n" :err ""})]
    (let [healthy (atom nil)
          output (with-out-str (reset! healthy (cmd-doctor [])))]
      {:healthy @healthy :output output}))))

(when (= "1" (System/getenv "NORTH_DOCTOR_EXIT_CHILD"))
  (let [{:keys [healthy output]} (exercise-doctor true)]
    (print output)
    (flush)
    (System/exit (if healthy 0 1))))

(def checks (atom []))
(defn check [label ok]
  (swap! checks conj [label ok])
  (println (if ok (str "PASS " label) (str "FAIL " label))))

(let [{:keys [healthy output]} (exercise-doctor true)]
  (check "doctor returns unhealthy on coordinator runtime identity failure" (false? healthy))
  (check "doctor renders the runtime identity error"
         (and (str/includes? output "[ERR]")
              (str/includes? output "runtime identity UNHEALTHY")))
  (check "doctor continues rendering aggregate sections after coordinator failure"
         (str/includes? output "guard hooks")))

(let [{:keys [healthy output]} (exercise-doctor false)]
  (check "doctor returns healthy when every critical section is healthy"
         (and healthy (str/includes? output "coordinator runtime identity OK"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false
       [{:sender "release-coordinator"
         :recipient "dead-session"
         :resolved-recipient "dead-session"
         :age "1h"}])]
  ;; Missing/malformed timestamps fail closed instead of hiding a delivery
  ;; failure whose age cannot be established.
  (check "doctor is unhealthy while an unaged dead letter exists" (false? healthy))
  (check "doctor dead-letter section names sender, recipient, and age"
         (and (str/includes? output "dead letters")
              (str/includes? output "release-coordinator")
              (str/includes? output "dead-session")
              (str/includes? output "1h"))))

(let [day-ms (* 24 60 60 1000)
      {:keys [healthy output]}
      (exercise-doctor
       false
       [{:sender "old-sender"
         :recipient "long-dead"
         :resolved-recipient "long-dead"
         :age "2d"
         :age-ms (* 2 day-ms)}])]
  (check "historical dead letters do not fail doctor" (true? healthy))
  (check "historical dead letters remain visible as warnings"
         (and (str/includes? output "[warn]")
              (str/includes? output "outside the 1h action window")
              (str/includes? output "2d")))
  (check "historical dead letters are not rendered as errors"
         (not (str/includes? output "[ERR]"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false
       [{:sender "live-sender"
         :recipient "just-died"
         :resolved-recipient "just-died"
         :age "30s"
         :age-ms 30000}])]
  (check "recently undeliverable mail fails doctor" (false? healthy))
  (check "recently undeliverable mail names the sender"
         (and (str/includes? output "[ERR]")
              (str/includes? output "live-sender"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false []
       (fn [name _]
         (if (= name "beagle")
           {:available true :behind 3 :dirty-files 2}
           {:available true :behind 0 :dirty-files 0})))]
  (check "doctor warns when a runtime is behind primary main"
         (str/includes? output "[warn] beagle: running 3 commits behind repo main"))
  (check "dirty primary checkout fails doctor with snapshot exclusion alarm"
         (and (false? healthy)
              (str/includes? output "[ERR] PRIMARY DIRTY: 2 files — snapshot builds EXCLUDE these (silent-exclusion risk)"))))

;; THE NEVER-AGAIN CLAUSE: doctor was green for months while the roster was dark.
(let [{:keys [healthy output]} (exercise-doctor false)]
  (check "coordination health is green when the hook path registers and reads back"
         (and healthy
              (str/includes? output "coordination health")
              (str/includes? output "hook-path log fence /data/coordination.log")
              (str/includes? output "presence write + readback")
              (str/includes? output "presence 3 live lease(s)")
              (str/includes? output "roster projection north:agent-roster:v1"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe (assoc healthy-coordination-probe
                      :log_fence_ok false
                      :expected_log "/data/facts.log")})]
  (check "hook-path log fence mismatch fails doctor" (false? healthy))
  (check "hook-path log fence mismatch names both logs"
         (and (str/includes? output "[ERR]")
              (str/includes? output "/data/facts.log")
              (str/includes? output "/data/coordination.log"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe {:version "north:coordination-probe:v1"
                :expected_log "/data/facts.log"
                :served_log "/data/coordination.log"
                :log_fence_ok false
                :lease_write_readback_ok false
                :error "coordinator returned a malformed resolved response"}})]
  ;; Everything downstream of a broken fence throws. Name the fence, not the
  ;; exception it caused.
  (check "a broken fence is diagnosed by cause, not by the exception it produced"
         (and (false? healthy)
              (str/includes? output "hook-path log fence mismatch")
              (str/includes? output "/data/facts.log"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe (assoc healthy-coordination-probe :lease_write_readback_ok false)})]
  (check "a presence lease that does not survive write+readback fails doctor"
         (and (false? healthy)
              (str/includes? output "did not survive write + readback"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe (assoc healthy-coordination-probe
                      :live_session_leases 0
                      :lineage_registrations_in_ttl 7)})]
  (check "zero live leases while sessions registered inside the TTL fails doctor"
         (and (false? healthy)
              (str/includes? output "presence is DARK")
              (str/includes? output "7 session registration(s)"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe (assoc healthy-coordination-probe
                      :live_session_leases 0
                      :lineage_registrations_in_ttl 0)})]
  (check "an honestly idle machine (no leases, no registrations) stays green"
         (and healthy (str/includes? output "presence 0 live lease(s)"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:roster {:err "agent subject projection unavailable"}})]
  (check "an erroring agent/roster projection fails doctor"
         (and (false? healthy)
              (str/includes? output "agent subject projection unavailable"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:probe {:err "coordination probe exceeded its 20000ms budget"}})]
  (check "an unavailable coordination probe fails doctor rather than reading green"
         (and (false? healthy)
              (str/includes? output "20000ms budget"))))

;; ACTIVATION HEALTH — the queue verb exists before any guard points at it, so
;; doctor must report the queue's state without the (unlanded) promote infra.
(let [{:keys [healthy output]} (exercise-doctor false)]
  (check "activation health renders with an empty queue and no promote infra"
         (and healthy
              (str/includes? output "activation health")
              (str/includes? output "no open rebuild requests")
              (str/includes? output "immediate admission")
              (str/includes? output "0 coordinated rebuild(s) observed in trailing 60m")
              (str/includes? output "urgent rate 0/0 request(s) (0%) in 24h")
              (str/includes? output "drift-without-promote: promote infra not yet deployed"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:activation
        (assoc healthy-activation-health
               :openCount 2
               :open [{:id "1-a" :requester "agent-a" :why "profile hook change"
                       :urgent false :ageMs 600000 :age "10m"}
                      {:id "1-b" :requester "agent-b" :why "guard wiring"
                       :urgent true :ageMs 60000 :age "1m"}]
               :urgent {:total 2 :urgent 1 :rate 0.5 :periodHours 24})})]
  ;; The queue is PARKED by design until the rebuild-coordination flip; open
  ;; asks must therefore be visible without failing doctor.
  (check "a parked queue reports open requests without failing doctor"
         (and healthy
              (str/includes? output "2 open rebuild request(s), queue PARKED")
              (str/includes? output "agent-a")
              (str/includes? output "[urgent]")))
  (check "urgent rate is warned, never refused"
         (and (str/includes? output "[warn]")
              (str/includes? output "urgent rate 1/2 request(s) (50%) in 24h"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:activation
        (assoc healthy-activation-health
               :coordinationOn true
               :gauge {:count 5})})]
  (check "rebuild volume remains observable without manufacturing a rate-cap failure"
         (and healthy
              (str/includes? output "5 coordinated rebuild(s) observed in trailing 60m")
              (not (str/includes? output "threshold"))
              (not (str/includes? output "the queue is being bypassed")))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:activation
        (assoc healthy-activation-health
               :coordinationOn true
               :openCount 1
               :open [{:id "1-c" :requester "agent-c" :why "stale request"
                       :urgent false :ageMs (* 5 3600 1000) :age "5h"}])})]
  (check "an open armed queue reports pending immediate drain without an hourly claim"
         (and healthy
              (str/includes? output "pending immediate serialized drain")
              (not (str/includes? output "exceeds two 60m windows")))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false [] (fn [_ _] {:available true :behind 0 :dirty-files 0})
       {:activation {:err "rebuild queue probe exceeded its 20000ms budget"}})]
  (check "an unavailable rebuild queue probe fails doctor rather than reading green"
         (and (false? healthy)
              (str/includes? output "20000ms budget"))))

(let [child @(p/process ["env"
                         "NORTH_DASHBOARD_LIB=1"
                         "NORTH_DOCTOR_EXIT_CHILD=1"
                         "bb" test-script]
                        {:out :string :err :string})
      public-source (slurp (str root "/cli/dashboard-cli.clj"))
      doctor-start (str/index-of public-source "(defn cmd-doctor")
      doctor-end (str/index-of public-source ";; ---- dispatch" doctor-start)
      doctor-source (subs public-source doctor-start doctor-end)]
  (check "failed aggregate process exits nonzero after rendering the full report"
         (and (= 1 (:exit child))
              (str/includes? (:out child) "runtime identity UNHEALTHY")
              (str/includes? (:out child) "guard hooks")))
  (check "public doctor dispatch maps the aggregate verdict to process status"
         (str/includes? public-source
                        "\"doctor\"          (when-not (cmd-doctor args) (System/exit 1))"))
  (check "public doctor uses bounded coordinator safety, not the full corpus audit"
         (and (str/includes? doctor-source "(coord-safety-probe)")
              (str/includes? doctor-source "\"coord-safety\"")
              (str/includes? doctor-source "(cache-get \"health.edn\" 300000)")
              (not (str/includes? doctor-source "coord-doctor-probe"))
              (not (str/includes? doctor-source "north-health")))))

(let [failed (remove second @checks)]
  (println (str "dashboard doctor exit: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
