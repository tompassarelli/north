(ns north.embedded-store-coordinator-test
  (:require [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(def ^String store (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT") (System/getenv "BEAGLE_STORE_HOME") (throw (ex-info "embedded coordinator test requires BEAGLE_STORE_TEST_CHECKOUT or BEAGLE_STORE_HOME" {}))))

(def ^String coordinator (str root "/bin/north-coordinator"))

(load-file (str root "/cli/coord.clj"))

(def checks (atom []))

(defn check! [^String label ok?]
  (do
  (swap! checks conj [label (boolean ok?)])
  nil))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
  (.getLocalPort socket)))

(defn fixture-environment [port conflicting-store-port ^String log]
  {"BEAGLE_STORE_HOME" store "BEAGLE_STORE_BIN" (str store "/bin") "BEAGLE_STORE_OUT" (str store "/out") "BEAGLE_STORE_LOG" log "BEAGLE_STORE_SPACE_ID" "north-coordination" "BEAGLE_STORE_SERVER_PORT" (str conflicting-store-port) "BEAGLE_STORE_SERVER_QUIET" "1" "BEAGLE_STORE_BIND" "127.0.0.1" "NORTH_HOME" root "NORTH_PORT" (str port) "NORTH_STORE_HOST" "127.0.0.1" "NORTH_TELEMETRY_PARTITION" "0"})

(defn start-coordinator! [port conflicting-store-port ^String log]
  (proc/process {:dir root :out :string :err :string :extra-env (fixture-environment port conflicting-store-port log)} coordinator))

(defn ^Boolean await-ready [port process]
  (loop [attempt 0]
  (cond
  (>= attempt 800) false
  (not (proc/alive? process)) false
  :else (let [status (try
  (north.coord/status! port)
  (catch Throwable _
    nil))]
  (if (and (= :ready (:state status)) (= "north-coordination" (:space-id status)) (= :rpc/jvm (:engine status))) true (do
  (Thread/sleep 25)
  (recur (inc attempt))))))))

(defn stop-coordinator! [process]
  (do
  (proc/destroy-tree process)
  (loop [attempt 0]
  (cond
  (not (proc/alive? process)) (deref process)
  (>= attempt 400) nil
  :else (do
  (Thread/sleep 25)
  (recur (inc attempt)))))))

(defn await-exit [process]
  (loop [attempt 0]
  (cond
  (not (proc/alive? process)) (deref process)
  (>= attempt 600) nil
  :else (do
  (Thread/sleep 25)
  (recur (inc attempt))))))

(let [tmp (.toFile (java.nio.file.Files/createTempDirectory "north-embedded-coordinator" (make-array java.nio.file.attribute.FileAttribute 0)))
   ^String log (.getCanonicalPath (io/file tmp "coordination.storelog"))
   first-port (free-port)
   ignored-store-port (free-port)
   first-process (start-coordinator! first-port ignored-store-port log)]
  (try
  (let [started? (await-ready first-port first-process)]
  (check! "NORTH_PORT names the North control endpoint even when the legacy Store port conflicts" started?)
  (if (not started?) (do
  (throw (ex-info "North embedded coordinator did not become ready" {:stdout (deref (:out first-process)) :stderr (deref (:err first-process))})))))
  (let [contender-port (free-port)
   contender (start-coordinator! contender-port (free-port) log)
   result (await-exit contender)]
  (check! "the canonical log admits exactly one active coordinator writer" (and (map? result) (not (zero? (:exit result))) (str/includes? (str (:err result)) "holds writer authority")))
  (if (nil? result) (stop-coordinator! contender) nil)
  (check! "a refused second writer leaves the admitted coordinator usable" (= :ready (:state (north.coord/status! first-port)))))
  (let [commit (north.coord/append! first-port "@coordinator:restart-proof" "state" "durable")
   committed-version (:ok commit)]
  (check! "the embedded coordinator acknowledges the durable Store commit" (and (integer? committed-version) (pos? committed-version)))
  (stop-coordinator! first-process)
  (let [restart-port (free-port)
   restarted (start-coordinator! restart-port (free-port) log)]
  (try
  (let [ready? (await-ready restart-port restarted)
   snapshot (if ready? (north.coord/show-envelope! restart-port "@coordinator:restart-proof") nil)]
  (check! "the same Store log and SpaceId reconstruct after process restart" (and ready? (>= (:version snapshot) committed-version) (= [["state" "durable"]] (:rows snapshot)))))
  (finally
    (stop-coordinator! restarted)))))
  (finally
    (if (proc/alive? first-process) (stop-coordinator! first-process) nil)
    (doseq [file (reverse (file-seq tmp))]
  (io/delete-file file true)))))

(let [results (deref checks)
   passed (count (filter second results))]
  (doseq [[label ok?] results]
  (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nembedded Store coordinator: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
