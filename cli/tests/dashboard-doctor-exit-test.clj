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

(defn exercise-doctor
  ([failed?] (exercise-doctor failed? []))
  ([failed? dead-letters]
  (with-redefs [coord-doctor-probe
                (fn [] (if failed?
                         {:ok false
                          :err "coordinator runtime identity UNHEALTHY — stale source"
                          :timeout-ms 100
                          :workload {:bytes 0 :files 0}}
                         {:ok true :out "coordinator runtime identity OK\n" :err ""}))
                daemon-health (fn [] {:north true})
                reactor-doctor-line (fn [_] "[ok]  last sweep now")
                north-health (fn [_] {:ok true})
                parse-health (fn [_] {:lanes-ran-24h 1
                                      :lanes-died-24h 0
                                      :concerns-active 1
                                      :concerns-stale 0})
                source-revision (fn [_ _] {:revision "test-rev" :origin "checkout HEAD"})
                north.message-routing/dead-letter-scan
                (fn [_] {:rows dead-letters})
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
  (check "doctor is unhealthy while dead letters exist" (false? healthy))
  (check "doctor dead-letter section names sender, recipient, and age"
         (and (str/includes? output "dead letters")
              (str/includes? output "release-coordinator")
              (str/includes? output "dead-session")
              (str/includes? output "1h"))))

;; AGE DECIDES FAULT vs DEBRIS. Mail that just became undeliverable means a live
;; sender is addressing a recipient that went away — a real coordination break.
;; Mail whose recipient has been gone for days is settled garbage: the send
;; already failed, nothing retries it, and no action today changes it.
;;
;; Failing on both is what destroyed the signal: 83 messages aged 1-2d held
;; doctor at rc=1 permanently, so it was red on a healthy coordinator and red
;; stopped carrying information. This pair pins the distinction, because the
;; behaviour was reverted once (53a87d2) with no rationale and it passes the
;; older assertions above either way.
(let [day-ms (* 24 60 60 1000)
      {:keys [healthy output]}
      (exercise-doctor
       false
       [{:sender "old-sender" :recipient "long-dead" :resolved-recipient "long-dead"
         :age "2d" :age-ms (* 2 day-ms)}])]
  (check "aged-out dead letters do NOT fail doctor" (true? healthy))
  (check "aged-out dead letters are still reported, as debris"
         (and (str/includes? output "dead letters")
              (str/includes? output "2d")))
  (check "aged-out dead letters are not rendered as an error"
         (not (str/includes? output "[ERR]"))))

(let [{:keys [healthy output]}
      (exercise-doctor
       false
       [{:sender "live-sender" :recipient "just-died" :resolved-recipient "just-died"
         :age "30s" :age-ms 30000}])]
  (check "recently-undeliverable mail DOES fail doctor" (false? healthy))
  (check "recently-undeliverable mail names the sender"
         (str/includes? output "live-sender")))

(let [child @(p/process ["env"
                         "NORTH_DASHBOARD_LIB=1"
                         "NORTH_DOCTOR_EXIT_CHILD=1"
                         "bb" test-script]
                        {:out :string :err :string})
      public-source (slurp (str root "/cli/dashboard-cli.clj"))]
  (check "failed aggregate process exits nonzero after rendering the full report"
         (and (= 1 (:exit child))
              (str/includes? (:out child) "runtime identity UNHEALTHY")
              (str/includes? (:out child) "guard hooks")))
  (check "public doctor dispatch maps the aggregate verdict to process status"
         (str/includes? public-source
                        "\"doctor\"          (when-not (cmd-doctor args) (System/exit 1))")))

(let [failed (remove second @checks)]
  (println (str "dashboard doctor exit: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
