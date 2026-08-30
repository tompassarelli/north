(ns beagle.user
  (:require [babashka.process :as proc]
            [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(def ^String store (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT") (System/getenv "BEAGLE_STORE_HOME") "/home/tom/code/beagle/main/store"))

(def ^String presence-cli (str root "/cli/presence-cli.clj"))

(def ^String north-cli (str root "/bin/north"))

(def ^String runtime-classpath (str root "/out:" store "/out"))

(def ^String babashka-bin (let [configured (System/getenv "NORTH_BB")
   ^String runtime-profile (str (System/getenv "HOME") "/.local/state/north/runtime-profile/bin/bb")]
  (if (some? configured) configured (if (.isFile (io/file runtime-profile)) runtime-profile "bb"))))

(if (not (.isFile (io/file store "bin/beagle-store-server"))) (do
  (throw (ex-info "current Beagle Store engine is required" {:store store}))))

(load-file (str root "/cli/coord.clj"))

(def checks (atom []))

(def test-log (atom nil))

(defn check! [^String label ok?]
  (swap! checks conj [label (boolean ok?)]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
  (.getLocalPort socket)))

(defn ^Boolean await-port [port process]
  (loop [attempt 0]
  (cond
  (>= attempt 800) false
  (not (proc/alive? process)) false
  :else (let [status (try
  (north.coord/status! port)
  (catch Throwable _
    nil))]
  (if (and (= :ready (:state status)) (= "north-coordination" (:space-id status))) true (do
  (Thread/sleep 25)
  (recur (inc attempt))))))))

(defn fixture-env [port ^String temp]
  {"HOME" temp "BEAGLE_STORE_HOME" store "BEAGLE_STORE_BIN" (str store "/bin") "BEAGLE_STORE_OUT" (str store "/out") "BEAGLE_STORE_LOG" (or (deref test-log) "") "BEAGLE_STORE_SERVER_CONNECT" "127.0.0.1" "BEAGLE_STORE_SERVER_PORT" (str port) "BEAGLE_STORE_SPACE_ID" "north-coordination" "NORTH_STORE_HOST" "127.0.0.1" "NORTH_STORE_READ_TIMEOUT_MS" "2000" "NORTH_PORT" (str port) "NORTH_TELEMETRY_PARTITION" "0" "NORTH_VERB_SLOTS" "0" "NORTH_AGENTS_LIB" "0" "NORTH_HOME" root "NORTH_BIN" north-cli "NORTH_BB" babashka-bin "NORTH_BUN" "/bin/false" "NORTH_POLICY_BUN" "/bin/false" "NORTH_STREAM_DIR" (str temp "/streams") "NORTH_AGENT_LOGS_DIR" (str temp "/agents") "NO_COLOR" "1"})

(defn run-presence [port ^String temp & $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (apply proc/sh {:out :string :err :string :continue true :extra-env (fixture-env port temp)} babashka-bin "-cp" runtime-classpath presence-cli (str port) args)))

(defn run-agents [port ^String temp]
  (proc/sh {:out :string :err :string :continue true :extra-env (fixture-env port temp)} north-cli "agents" "--json"))

(defn ^Boolean control-row? [row ^String control]
  (and (map? row) (= control (get row "control_id"))))

(let [port (free-port)
   tmp (.toFile (java.nio.file.Files/createTempDirectory "north-presence-online" (make-array java.nio.file.attribute.FileAttribute 0)))
   ^String temp (.getCanonicalPath tmp)
   facts (io/file tmp "coordination.storelog")
   ^String log (.getCanonicalPath facts)
   daemon (proc/process {:dir store :out :string :err :string :extra-env {"BEAGLE_STORE_SERVER_QUIET" "1" "BEAGLE_STORE_SERVER_XMX" "1g"}} (str store "/bin/beagle-store-server") "serve" (str port) log "north-coordination")]
  (reset! test-log log)
  (try
  (let [started? (await-port port daemon)]
  (check! "throwaway current Beagle Store server starts" started?)
  (if (not started?) (do
  (proc/destroy-tree daemon)
  (throw (ex-info "throwaway Beagle Store coordinator did not start" {:stdout (deref (:out daemon)) :stderr (deref (:err daemon))})))))
  (let [live-registration (run-presence port temp "register" "live-session" "/tmp/live" "live-session")
   lapsed-registration (run-presence port temp "register" "lapsed-session" "/tmp/lapsed" "lapsed-session")]
  (check! "live session registers through canonical lease acquisition" (zero? (:exit live-registration)))
  (check! "historical session registers through canonical lease acquisition" (zero? (:exit lapsed-registration)))
  (if (not (and (zero? (:exit live-registration)) (zero? (:exit lapsed-registration)))) (do
  (throw (ex-info "presence registration child failed" {:live-error (:err live-registration) :lapsed-error (:err lapsed-registration)}))))
  (north.coord/release-lease! port (json/parse-string (str/trim (:out lapsed-registration)) true)))
  (let [now (System/currentTimeMillis)
   leases (north.coord/online-session-leases! port now)
   lease (first leases)]
  (check! "nested Store lease scan returns only the live session" (and (= 1 (count leases)) (= "live-session" (:handle lease))))
  (check! "nested lease parser exposes the exact roster DTO" (= #{:handle :exp} (set (keys lease))))
  (check! "nested lease expiry remains a future integer" (and (integer? (:exp lease)) (> (:exp lease) now))))
  (let [identity-results [(north.coord/append! port "@agent:live-session" "kind" "session") (north.coord/append! port "@agent:live-session" "provider" "fixture") (north.coord/append! port "@agent:live-session" "model" "fixture")]]
  (check! "throwaway agent identity facts commit" (every? (fn [result] (some? (:ok result))) identity-results)))
  (let [agents-result (run-agents port temp)
   parsed (if (zero? (:exit agents-result)) (json/parse-string (str/trim (:out agents-result)) false) nil)
   raw-rows (if (map? parsed) (do
  (get parsed "agents")))
   rows (if (vector? raw-rows) raw-rows [])
   live-rows (filterv (fn [row] (control-row? row "live-session")) rows)
   lapsed-rows (filterv (fn [row] (control-row? row "lapsed-session")) rows)
   live-row (first live-rows)]
  (check! "fresh-process north agents accepts the lease projection" (and (zero? (:exit agents-result)) (not (str/includes? (str (:out agents-result) (:err agents-result)) "liveness lease projection was malformed"))))
  (check! "fresh-process roster joins the live lease to agent identity" (and (= "north:agent-roster:v1" (get parsed "version")) (= 1 (count live-rows)) (= "session" (get live-row "kind")) (= true (get live-row "online"))))
  (check! "fresh-process roster excludes the released historical lease" (empty? lapsed-rows)))
  (let [live-result (run-presence port temp "live-leases")
   full-result (run-presence port temp "roster")
   json-result (run-presence port temp "live-leases-json")
   ^String live (:out live-result)
   ^String full (:out full-result)
   machine (json/parse-string (str/trim (:out json-result)) false)]
  (check! "live-only projection includes the unexpired session" (str/includes? live "live-session"))
  (check! "live-only projection excludes historical lapsed sessions" (not (str/includes? live "lapsed-session")))
  (check! "full historical projection remains available" (and (str/includes? full "live-session") (str/includes? full "lapsed-session") (str/includes? full "lapsed")))
  (check! "machine projection exposes the current versioned contract" (and (zero? (:exit json-result)) (= "north:live-leases:v1" (get machine "version")))))
  (finally
    (proc/destroy-tree daemon)
    (doseq [file (reverse (file-seq tmp))]
  (io/delete-file file true)))))

(let [results (deref checks)
   passed (count (filter second results))]
  (doseq [[label ok?] results]
  (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\npresence online integration: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
