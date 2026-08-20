(require '[clojure.string :as str]
         '[north.main :as north])

(def failures (atom 0))
(defn check! [label passed?]
  (println (str (if passed? "PASS" "FAIL") " — " label))
  (when-not passed? (swap! failures inc)))

(def log "/tmp/north diagnostics.storelog")
(def down (north/store-rpc-failure-message -1 7977 log "capture was not recorded"))
(def mismatch (north/store-rpc-failure-message -2 7977 log "still clocked in"))
(def incompatible (north/store-rpc-failure-message -3 7977 log "schema seed was not recorded"))

(check! "unreachable STORE RPC server names the configured service remedy"
        (and (str/includes? down "STORE RPC SERVER UNREACHABLE")
             (str/includes? down "Start the configured Beagle Store service")
             (not (str/includes? down "STORE RPC SPACE MISMATCH"))))
(check! "wrong SpaceId names the selected STORELOG database"
        (and (str/includes? mismatch "STORE RPC SPACE MISMATCH")
             (str/includes? mismatch log)
             (str/includes? mismatch "SpaceId")
             (str/includes? mismatch "still clocked in")
             (not (str/includes? mismatch "SERVER UNREACHABLE"))))
(check! "incompatible STORE RPC protocol has a matched-release remedy"
        (and (str/includes? incompatible "STORE RPC PROTOCOL INCOMPATIBLE")
             (str/includes? incompatible "matched North + Beagle Store release")
             (not= mismatch incompatible)))

(if (zero? @failures)
  (do (println "coordinator diagnostics: PASS") (System/exit 0))
  (do (println (str "coordinator diagnostics: " @failures " FAIL"))
      (System/exit 1)))
