#!/usr/bin/env bb
;; spend-cli-test.clj — the spend-guard LEDGER against a real Fram daemon.
;;
;; The concurrent-reservation RACE is the point of build-order step 2: two (here
;; ten) reservers contend for headroom sufficient for only some — exactly the
;; winners commit, every loser gets a refusal, and the ledger total NEVER exceeds
;; the cap. Proven against the real :assert-at-version CAS wire, not a mock:
;; a bare read-then-tell would silently lose updates here (fram-cas-verification
;; §3). Also covers fail-closed schema/price gates, exact-vs-worst-case
;; settlement, and override honoring + expiry.
;;   bb cli/tests/spend-cli-test.clj
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[cheshire.core :as json])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(def fram (.getCanonicalPath
           (io/file (or (System/getenv "FRAM_PATH") (str root "/../fram")))))
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw (ex-info "Fram checkout not found; set FRAM_PATH or clone it beside North" {:fram fram})))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/spend-cli.clj"))

(defn free-port [] (with-open [s (java.net.ServerSocket. 0)] (.getLocalPort s)))
(defn port-open? [p] (try (with-open [s (java.net.Socket. "127.0.0.1" (int p))] true) (catch Exception _ false)))
(defn eventually [f] (loop [n 200] (cond (try (f) (catch Exception _ false)) true (zero? n) false :else (do (Thread/sleep 25) (recur (dec n))))))

;; write a budget directly through coord (mirrors `north spend init`, but each
;; toggle is independent so a subtest can drop schema or prices to prove
;; fail-closed behaviour).
(defn setup-budget! [port target {:keys [cap env schema? prices?]
                                  :or {schema? true prices? true}}]
  (when schema?
    (doseq [p ["reserved_microusd" "settled_microusd"]]
      (north.coord/send-op port {:op :assert :te (str "@" p) :p "cardinality" :r "single"})))
  (doseq [[p v] [["kind" "spend-budget"] ["billing" "api"]
                 ["budget_cap_microusd" (str cap)]
                 ["lane_envelope_default_microusd" (str env)]
                 ["lane_envelope_max_microusd" (str env)]
                 ["burn_limit_microusd_per_hour" "10000000"]
                 ["budget_period" "month"]
                 ["layer1_confirmed" "prepaid, auto-topup off, 2026-07-19"]]]
    (north.coord/put! port (str "@spend-budget:" target) p v))
  (when prices?
    (north.coord/put! port (str "@spend-budget:" target) "price_in_per_mtok" "3000000")
    (north.coord/put! port (str "@spend-budget:" target) "price_out_per_mtok" "15000000")))

(let [port (free-port)
      dir (.toFile (java.nio.file.Files/createTempDirectory "spend-cli-test" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "facts.log")
      _ (spit log "")
      daemon (proc/process
              {:dir fram :out :string :err :string :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
              "bb" "-cp" "out" "coord_daemon.clj" "serve-flat" (str port) (.getPath log))
      checks (atom [])
      check! (fn [label value] (swap! checks conj [label (boolean value)]))]
  (alter-var-root #'north.coord/expected-log (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check! "real Fram daemon starts" (eventually #(port-open? port)))

    ;; --- fail-closed: missing cardinality declaration --------------------------
    (setup-budget! port "t-schema" {:cap 60000000 :env 1500000 :schema? false})
    (let [r (reserve! port "t-schema" 1500000)]
      (check! "reservation refuses when reserved_microusd is not declared single (fail-closed)"
              (= "missing-schema" (:reason r))))

    ;; --- fail-closed: missing price fact ---------------------------------------
    (setup-budget! port "t-price" {:cap 60000000 :env 1500000 :prices? false})
    (let [r (reserve! port "t-price" 1500000)]
      (check! "reservation refuses at admission when a price fact is missing (fail-closed)"
              (and (= "budget-incomplete" (:reason r))
                   (re-find #"price_" (str (:detail r))))))

    ;; --- exact settlement releases the remainder -------------------------------
    (setup-budget! port "t-settle" {:cap 60000000 :env 1500000})
    (let [rv (reserve! port "t-settle" 1500000)
          ;; 100k in * $3/Mtok + 20k out * $15/Mtok = 0.3 + 0.3 = $0.60
          sv (settle! port "t-settle" (:period rv) 1500000 {:status "exact" :input 100000 :output 20000})
          period (:period rv)]
      (check! "exact evidence settles DOWN to actual and releases the remainder"
              (and (:ok rv) (:ok sv)
                   (= 600000 (:final sv)) (= "exact" (:evidence sv)) (= 900000 (:released sv))))
      (check! "after exact settle: reserved back to 0, settled = actual"
              (and (= 0 (counter port period "reserved_microusd"))
                   (= 600000 (counter port period "settled_microusd")))))

    ;; --- unknown coverage: full reservation STANDS -----------------------------
    (setup-budget! port "t-unknown" {:cap 60000000 :env 1500000})
    (let [rv (reserve! port "t-unknown" 1500000)
          sv (settle! port "t-unknown" (:period rv) 1500000 {:status "unknown" :input 100000 :output 20000})
          period (:period rv)]
      (check! "unknown coverage keeps the full reservation as the final charge (worst-case)"
              (and (:ok sv) (= 1500000 (:final sv)) (= "reserved-worst-case" (:evidence sv)) (= 0 (:released sv))))
      (check! "after worst-case settle: settled = full envelope"
              (= 1500000 (counter port period "settled_microusd"))))

    ;; --- THE CONCURRENT RESERVATION RACE ---------------------------------------
    ;; cap = exactly 3 envelopes; 10 reservers fire concurrently (real threads,
    ;; each its own socket). The CAS admits exactly 3; the other 7 are refused;
    ;; the counter never exceeds the cap.
    (setup-budget! port "t-race" {:cap 4500000 :env 1500000})
    (let [n 10
          results (->> (range n)
                       (mapv (fn [_] (future (reserve! port "t-race" 1500000))))
                       (mapv deref))
          winners (filter :ok results)
          losers  (remove :ok results)
          period  (period-id "t-race" (utc-month))
          final   (counter port period "reserved_microusd")]
      (println (format "  [RACE] %d reservers, cap=3 envelopes -> %d committed, %d refused, ledger reserved=$%.2f"
                       n (count winners) (count losers) (/ final 1e6)))
      (check! "concurrent race: exactly 3 reservations commit (one-wins-per-slot)"
              (= 3 (count winners)))
      (check! "concurrent race: every loser gets a spend-guard refusal (over-cap or conflict-exhausted)"
              (and (= 7 (count losers))
                   (every? #(#{"over-cap" "conflict-exhausted"} (:reason %)) losers)))
      (check! "concurrent race: ledger total NEVER exceeds the cap"
              (and (= 4500000 final) (<= final 4500000))))

    ;; --- deterministic cap enforcement (sequential complement) -----------------
    (setup-budget! port "t-cap" {:cap 1500000 :env 1500000})
    (let [a (reserve! port "t-cap" 1500000)
          b (reserve! port "t-cap" 1500000)]
      (check! "sequential: first reservation fills the last dollar, second is refused over-cap"
              (and (:ok a) (not (:ok b)) (= "over-cap" (:reason b)))))

    ;; --- override honored + expiry ignored -------------------------------------
    (setup-budget! port "t-override" {:cap 1500000 :env 1500000})
    (let [budget (str "@spend-budget:t-override")
          now (System/currentTimeMillis)
          future-ms (+ now (* 24 60 60 1000))
          past-ms (- now (* 60 1000))]
      (reserve! port "t-override" 1500000) ; exhaust the base cap
      (check! "cap exhausted: reservation refused before any override"
              (= "over-cap" (:reason (reserve! port "t-override" 1500000))))
      ;; an EXPIRED override adds no headroom
      (north.coord/append! port budget "spend_override"
                           (json/generate-string {"amount_microusd" 5000000 "until_ms" past-ms "reason" "expired" "created_by" "test"}))
      (check! "an expired override is ignored (no headroom added)"
              (and (= 0 (active-override-micro port "t-override" now))
                   (= "over-cap" (:reason (reserve! port "t-override" 1500000)))))
      ;; an ACTIVE override lifts headroom and admits the reservation
      (north.coord/append! port budget "spend_override"
                           (json/generate-string {"amount_microusd" 5000000 "until_ms" future-ms "reason" "surge" "created_by" "test"}))
      (check! "an unexpired override lifts headroom and admits the reservation"
              (and (= 5000000 (active-override-micro port "t-override" now))
                   (:ok (reserve! port "t-override" 1500000)))))

    ;; --- CLI override guardrails (48h expiry cap, mandatory reason) -------------
    (let [run (fn [& a] (apply proc/shell
                               {:dir root :out :string :err :string :continue true
                                :extra-env {"NORTH_PORT" (str port) "FRAM_LOG" (.getCanonicalPath log) "NORTH_AUTHOR" "test"}}
                               "bb" "cli/spend-cli.clj" a))
          far (.toString (.plusSeconds (java.time.Instant/now) (* 49 60 60)))
          near (.toString (.plusSeconds (java.time.Instant/now) (* 24 60 60)))]
      (setup-budget! port "t-cli-ovr" {:cap 1500000 :env 1500000})
      (check! "CLI override refuses an expiry beyond 48h"
              (not= 0 (:exit (run "override" "t-cli-ovr" "--add-usd" "5" "--until" far "--reason" "too far"))))
      (check! "CLI override refuses a missing reason"
              (not= 0 (:exit (run "override" "t-cli-ovr" "--add-usd" "5" "--until" near))))
      (check! "CLI override accepts a valid within-48h override"
              (= 0 (:exit (run "override" "t-cli-ovr" "--add-usd" "5" "--until" near "--reason" "surge")))))

    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks] (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed" (- (count @checks) (count failed)) (count @checks)))
        (when (seq failed) (System/exit 1))))))
