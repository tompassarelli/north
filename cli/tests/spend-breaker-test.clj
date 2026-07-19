#!/usr/bin/env bb
;; spend-breaker-test.clj — the spend-guard CIRCUIT BREAKER + reactor burn/kill/reap
;; primitives (build-order step 3) against a REAL Fram daemon. Reuses the step-2
;; harness (temp daemon + temp log). Covers, in dependency order (the breaker is
;; GLOBAL, so trip/reset ordering is explicit):
;;   1. dead-lane settlement is idempotent (re-sweep never double-settles);
;;   2. a trailing-window burn breach TRIPS the global breaker;
;;   3. a tripped breaker refuses every reservation (breaker-distinct reason);
;;   4. sweep-kill fires ONLY on a verified live envelope-lease pid (simulated);
;;   5. the reset ceremony refuses under a live trip condition, the --force path
;;      works with the typed confirmation, provenance facts land, and a non-human
;;      actor is flagged for needs-review.
;;   bb cli/tests/spend-breaker-test.clj   (FRAM_PATH or ../fram)
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[cheshire.core :as json])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(def fram (.getCanonicalPath
           (io/file (or (System/getenv "FRAM_PATH") (str root "/../fram")))))
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw (ex-info "Fram checkout not found; set FRAM_PATH or clone it beside North" {:fram fram})))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/spend-cli.clj"))   ; also loads spend-breaker.clj

(defn free-port [] (with-open [s (java.net.ServerSocket. 0)] (.getLocalPort s)))
(defn port-open? [p] (try (with-open [s (java.net.Socket. "127.0.0.1" (int p))] true) (catch Exception _ false)))
(defn eventually [f] (loop [n 1200] (cond (try (f) (catch Exception _ false)) true (zero? n) false :else (do (Thread/sleep 25) (recur (dec n))))))
(defn now [] (System/currentTimeMillis))

(defn setup-budget! [port target {:keys [cap env burn]
                                  :or {cap 60000000 env 1500000 burn 10000000}}]
  (doseq [p ["reserved_microusd" "settled_microusd"]]
    (north.coord/send-op port {:op :assert :te (str "@" p) :p "cardinality" :r "single"}))
  (doseq [[p v] [["kind" "spend-budget"] ["billing" "api"]
                 ["budget_cap_microusd" (str cap)]
                 ["lane_envelope_default_microusd" (str env)]
                 ["lane_envelope_max_microusd" (str env)]
                 ["burn_limit_microusd_per_hour" (str burn)]
                 ["budget_period" "month"]
                 ["layer1_confirmed" "prepaid, auto-topup off, 2026-07-19"]
                 ["price_in_per_mtok" "3000000"] ["price_out_per_mtok" "15000000"]]]
    (north.coord/put! port (str "@spend-budget:" target) p v)))

(defn write-lane! [port id {:keys [lane pid target period reserved]}]
  (doseq [[p v] [["kind" "spend-lane"] ["lane" lane] ["pid" (str pid)]
                 ["target" target] ["period" period] ["reserved_microusd" (str reserved)]]]
    (north.coord/append! port (str "@" id) p v)))

(let [port (free-port)
      dir (.toFile (java.nio.file.Files/createTempDirectory "spend-breaker-test" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "facts.log")
      ring (io/file dir "burn-ring.json")
      _ (spit log "")
      daemon (proc/process
              {:dir fram :out :string :err :string :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
              "bb" "-cp" "out" "coord_daemon.clj" "serve-flat" (str port) (.getPath log))
      checks (atom [])
      procs  (atom [])
      check! (fn [label value] (swap! checks conj [label (boolean value)]))]
  ;; both the in-process reads/writes AND the shelled CLI point at the SAME ring file.
  (alter-var-root #'north.coord/expected-log (constantly (fn [] (.getCanonicalPath log))))
  (alter-var-root #'north.spend-breaker/ring-file (constantly (fn [_] ring)))
  (try
    (check! "real Fram daemon starts" (eventually #(port-open? port)))

    ;; ============ 1. DEAD-LANE SETTLEMENT IS IDEMPOTENT ======================
    (setup-budget! port "t-reap" {})
    (let [rv (reserve! port "t-reap" 1500000)
          period (:period rv)]
      (write-lane! port "spend-lane:reap-1"
                   {:lane "@laneR" :pid 999999999 :target "t-reap" :period period :reserved 1500000})
      (let [n1 (north.spend-breaker/reap-settle-lane-reservations! port "laneR" false)
            settled-after-1 (north.spend-breaker/counter port period "settled_microusd")
            n2 (north.spend-breaker/reap-settle-lane-reservations! port "laneR" false)
            settled-after-2 (north.spend-breaker/counter port period "settled_microusd")]
        (check! "reaper settles a dead lane's open reservation at FULL (status unknown)"
                (and (= 1 n1) (= 1500000 settled-after-1)))
        (check! "re-sweep is idempotent: settled_at guards a double-settle"
                (and (= 0 n2) (= 1500000 settled-after-2)))
        (check! "settled lane carries a settled_at provenance stamp"
                (seq (str (north.coord/resolved port "@spend-lane:reap-1" "settled_at"))))))

    ;; ============ 2. BURN BREACH TRIPS THE GLOBAL BREAKER =====================
    ;; cumulative reserved = $6 over a seeded 20-min baseline of $0 ⇒ ~$18/hr > $10/hr.
    (setup-budget! port "t-burn" {})
    (reserve! port "t-burn" 6000000)
    (north.spend-breaker/write-ring! port {"t-burn" [{:ts (- (now) 1200000) :total 0}]})
    (let [r (north.spend-breaker/sweep-burn! port false)]
      (check! "burn-rate breach trips the global breaker"
              (and (:tripped r) (north.spend-breaker/tripped? port)))
      (check! "trip_reason names the burn-rate breach"
              (re-find #"burn-rate breach" (north.spend-breaker/trip-reason port))))

    ;; ============ 3. TRIPPED ⇒ RESERVE REFUSES (breaker-distinct) =============
    (let [r (reserve! port "t-burn" 1500000)]
      (check! "a tripped breaker refuses reservation with a breaker-distinct reason"
              (= "breaker-tripped" (:reason r)))
      (check! "the refusal carries the trip_reason detail (breaker vs headroom distinct)"
              (re-find #"burn-rate breach" (str (:detail r)))))

    ;; ============ 4. SWEEP-KILL FIRES ONLY ON VERIFIED LEASE PIDS =============
    (setup-budget! port "t-kill" {})
    (let [pv (proc/process {:out :string :err :string} "sleep" "300")
          pu (proc/process {:out :string :err :string} "sleep" "300")
          pid-v (.pid (:proc pv))
          pid-u (.pid (:proc pu))
          period (north.spend-breaker/period-id "t-kill" (north.spend-breaker/utc-month))]
      (swap! procs conj pv pu)
      (write-lane! port "spend-lane:kill-v"
                   {:lane "@laneV" :pid pid-v :target "t-kill" :period period :reserved 1000000})
      (write-lane! port "spend-lane:kill-u"
                   {:lane "@laneU" :pid pid-u :target "t-kill" :period period :reserved 1000000})
      ;; verified envelope-lease set holds ONLY the verified pid.
      (alter-var-root #'north.spend-breaker/active-envelope-pids (constantly (fn [] #{pid-v})))
      (let [killed (north.spend-breaker/sweep-kill! port false)]
        (Thread/sleep 200)
        (check! "sweep-kill kills exactly the one verified live envelope-lease lane"
                (= 1 killed))
        (check! "the verified lane's process is dead after sweep-kill"
                (not (north.spend-breaker/pid-alive? pid-v)))
        (check! "the UNVERIFIED lane (pid absent from envelope leases) is NOT killed"
                (north.spend-breaker/pid-alive? pid-u))
        (check! "the killed lane is stamped killed_at + settled_at"
                (and (seq (str (north.coord/resolved port "@spend-lane:kill-v" "killed_at")))
                     (seq (str (north.coord/resolved port "@spend-lane:kill-v" "settled_at")))))
        (check! "the unverified lane is left open (no killed_at)"
                (empty? (str (north.coord/resolved port "@spend-lane:kill-u" "killed_at"))))))

    ;; ============ 5. RESET CEREMONY: refuse-live, --force, provenance =========
    (let [trip (north.spend-breaker/trip-reason port)
          run (fn [author & a]
                (apply proc/shell
                       {:dir root :out :string :err :string :continue true
                        :extra-env {"NORTH_PORT" (str port)
                                    "FRAM_LOG" (.getCanonicalPath log)
                                    "NORTH_SPEND_BURN_STATE" (.getCanonicalPath ring)
                                    "NORTH_AUTHOR" author}}
                       "bb" "cli/spend-cli.clj" a))]
      (check! "reset ceremony REFUSES while the trip condition still evaluates true"
              (not= 0 (:exit (run "ci-agent" "reset-breaker" "--reason" "recovered"))))
      (check! "--force refuses when --i-understand does NOT quote the current trip_reason"
              (not= 0 (:exit (run "ci-agent" "reset-breaker" "--force" "--i-understand" "wrong phrase"))))
      (let [ok (run "ci-agent" "reset-breaker" "--force" "--i-understand" trip "--reason" "recovered")]
        (check! "--force with the exact typed confirmation resets the breaker"
                (= 0 (:exit ok)))
        (check! "the breaker is no longer tripped after reset"
                (not (north.spend-breaker/tripped? port)))
        (check! "reset_by provenance fact is written"
                (= "ci-agent" (str (north.coord/resolved port (str "@" north.spend-breaker/BREAKER) "reset_by"))))
        (check! "a durable reset audit event entity is recorded"
                (seq (str (north.coord/resolved port (str "@" north.spend-breaker/BREAKER) "reset_at")))))
      (let [audit (run "ci-agent" "reset-audit")]
        (check! "reset-audit surfaces a non-human reset actor for needs-review (nonzero exit)"
                (and (not= 0 (:exit audit)) (re-find #"NEEDS REVIEW" (str (:out audit)))))))

    (finally
      (doseq [p @procs] (try (proc/destroy-tree p) (catch Exception _ nil)))
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks] (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed" (- (count @checks) (count failed)) (count @checks)))
        (when (seq failed) (System/exit 1))))))
