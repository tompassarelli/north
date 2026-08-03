#!/usr/bin/env bb
;; S4 operator runbook. Production subcommands are intentionally separate;
;; `rehearse` drives the same functions against copied logs and high ports.
(require '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(import '[java.io File]
        '[java.net InetSocketAddress Socket ServerSocket]
        '[java.nio.charset StandardCharsets]
        '[java.nio.file Files Path StandardCopyOption]
        '[java.security MessageDigest]
        '[java.util Arrays UUID]
        '[java.util.concurrent TimeUnit])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))
(def fram (.getCanonicalPath
           (io/file (or (System/getenv "FRAM_PATH") "/home/tom/code/fram/main"))))
(def v03-out (.getCanonicalPath
              (io/file (or (System/getenv "NORTH_V03_FRAM_OUT")
                           (str (System/getProperty "user.home")
                                "/.local/state/north/fram-runtime/active/current/out")))))
(def bb (or (System/getenv "NORTH_BB") "bb"))
(def state-home (str (System/getProperty "user.home") "/.local/state/north"))
(def systemd-units
  ["north-coord-pair.target" "north-coord.socket"
   "north-telemetry-coord.socket" "north-coord.service"
   "north-telemetry-coord.service"])
(def head-units
  ["north-fram-head-coordination.service"
   "north-fram-head-telemetry.service"])

(def baseline-expression
  "(require '[fram.rt :as rt] '[fram.fold :as fold] '[clojure.java.io :as io])
   (let [[source triples-path summary-path] *command-line-args*
         folded (fold/fold (rt/read-log source)) facts (:facts folded)]
     (with-open [writer (io/writer triples-path)]
       (doseq [fact facts]
         (.write writer (pr-str [(:l fact) (:p fact) (:r fact)]))
         (.write writer \"\\n\")))
     (spit summary-path (pr-str {:version (:version folded) :live-count (count facts)})))")

(defn fail! [message data]
  (throw (ex-info message data)))

(defn sha256-bytes [^bytes bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
    (apply str (map #(format "%02x" (bit-and (int %) 255)) digest))))

(defn sha256-file [path]
  (sha256-bytes (Files/readAllBytes (.toPath (io/file path)))))

(defn process-result [arguments directory environment]
  (let [builder (ProcessBuilder. ^java.util.List (mapv str arguments))
        process-environment (.environment builder)]
    (.directory builder (io/file directory))
    (doseq [[key value] environment]
      (.put process-environment (name key) (str value)))
    (let [process (.start builder)
          stdout (future (slurp (.getInputStream process)))
          stderr (future (slurp (.getErrorStream process)))
          exit (.waitFor process)]
      {:exit exit :stdout @stdout :stderr @stderr})))

(defn process! [label arguments directory environment]
  (let [result (process-result arguments directory environment)]
    (when-not (zero? (:exit result))
      (fail! (str label " failed")
             {:arguments arguments :exit (:exit result)
              :stdout (:stdout result) :stderr (:stderr result)}))
    result))

(defn state-path [directory] (str directory "/cutover.edn"))

(defn read-state [directory]
  (let [path (state-path directory)]
    (when-not (.isFile (io/file path))
      (fail! "cutover state is absent; run quiesce or rehearse first"
             {:directory directory}))
    (edn/read-string (slurp path))))

(defn write-state! [directory state]
  (let [target (.toPath (io/file (state-path directory)))
        tmp (Files/createTempFile (.toPath (io/file directory))
                                  ".cutover-state-" ".tmp"
                                  (make-array java.nio.file.attribute.FileAttribute 0))]
    (spit (.toFile tmp) (str (pr-str state) "\n"))
    (Files/move tmp target
                (into-array StandardCopyOption
                            [StandardCopyOption/ATOMIC_MOVE
                             StandardCopyOption/REPLACE_EXISTING]))
    state))

(defn update-state! [directory f & arguments]
  (write-state! directory (apply f (read-state directory) arguments)))

(defn free-port []
  (with-open [socket (ServerSocket. 0)] (.getLocalPort socket)))

(defn base-state [directory mode sources ports]
  {:format :north/framlog-cutover-v1
   :thread "019fc335-58fa-734f-b473-38f875f5cbc5"
   :mode mode :directory directory :fram fram :v03-out v03-out
   :sources sources :ports ports
   :spaces {:coordination "north-coordination" :telemetry "north-telemetry"}
   :env-file (if (= mode :production)
               (or (System/getenv "NORTH_FRAMRPC_ENV")
                   (str state-home "/framrpc.env"))
               (str directory "/framrpc.env"))
   :steps {}})

(defn ensure-production-state! [directory]
  (Files/createDirectories (.toPath (io/file directory))
                           (make-array java.nio.file.attribute.FileAttribute 0))
  (when-not (.exists (io/file (state-path directory)))
    (write-state!
     directory
     (base-state
      directory :production
      {:coordination (or (System/getenv "NORTH_CUTOVER_COORD_LOG")
                         (str state-home "/coordination.log"))
       :telemetry (or (System/getenv "NORTH_CUTOVER_TELEMETRY_LOG")
                      (str state-home "/telemetry.log"))}
      {:coordination 7977 :telemetry 7978})))
  (read-state directory))

(defn port-open? [port]
  (try
    (with-open [socket (Socket.)]
      (.connect socket (InetSocketAddress. "127.0.0.1" port) 150)
      true)
    (catch Throwable _ false)))

(defn quiesce! [directory]
  (let [{:keys [mode ports]} (read-state directory)]
    (if (= mode :production)
      (do
        (process! "stop socket-activated North pair"
                  (into ["sudo" "systemctl" "stop"] systemd-units) root {})
        (doseq [unit head-units]
          (process-result ["sudo" "systemctl" "stop" unit] root {}))
        (doseq [unit (concat systemd-units head-units)]
          (let [result (process-result ["systemctl" "is-active" unit] root {})]
            (when (= "active" (str/trim (:stdout result)))
              (fail! "systemd unit remained active after quiesce" {:unit unit})))))
      nil)
    (doseq [[store port] ports]
      (when (port-open? port)
        (fail! "listener remained after quiesce" {:store store :port port})))
    (update-state! directory assoc-in [:steps :quiesce]
                   {:ok true :verified :listeners-absent})
    (println "QUIESCE ok — services and socket units are stopped; listeners absent")))

(defn copy-frozen! [source target]
  (when-not (.isFile (io/file source))
    (fail! "source log is absent" {:source source}))
  (if (.exists (io/file target))
    (when-not (= (sha256-file source) (sha256-file target))
      (fail! "existing frozen copy differs from the quiesced source"
             {:source source :target target}))
    (Files/copy (.toPath (io/file source)) (.toPath (io/file target))
                (into-array StandardCopyOption [])))
  {:path target :bytes (.length (io/file target)) :sha256 (sha256-file target)})

(defn freeze! [directory]
  (let [state (read-state directory)
        frozen-dir (str directory "/frozen")]
    (Files/createDirectories (.toPath (io/file frozen-dir))
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (let [frozen
          (into {}
                (for [[store source] (:sources state)]
                  [store (copy-frozen! source (str frozen-dir "/" (name store) ".log"))]))]
      (update-state! directory #(-> % (assoc :frozen frozen)
                                    (assoc-in [:steps :freeze]
                                              {:ok true :verified :sha256-copies})))
      (doseq [[store value] frozen]
        (println (str "FREEZE " (name store) " bytes=" (:bytes value)
                      " sha256=" (:sha256 value)))))))

(defn migrate-one! [directory store state]
  (let [migration-dir (str directory "/migrated")
        source (get-in state [:frozen store :path])
        normalized (str migration-dir "/" (name store) ".normalized.log")
        target (str migration-dir "/" (name store) ".framlog")
        space (get-in state [:spaces store])]
    (Files/createDirectories (.toPath (io/file migration-dir))
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (let [normalization
          (edn/read-string
           (:stdout
            (process! (str (name store) " normalization pre-pass")
                      [bb (str root "/scripts/framlog-normalize.clj") source normalized]
                      root {})))]
      (let [manifest-path (str target ".migration.edn")
            target-exists? (.isFile (io/file target))
            manifest-exists? (.isFile (io/file manifest-path))]
        (when (not= target-exists? manifest-exists?)
          (fail! "migration target and manifest must both exist or both be absent"
                 {:store store :target target :manifest manifest-path}))
        (when-not target-exists?
          (process! (str (name store) " FRAMLOG migration")
                    [(str fram "/bin/fram-migrate-triple-log") normalized space target]
                    fram {:CLJ_CONFIG (str directory "/clj-config")}))
        (let [manifest (edn/read-string (slurp manifest-path))
              output-sha (sha256-file target)
              source-sha (sha256-file normalized)]
          (when-not (and (= "fram-triple-log-migration-manifest/v1"
                            (:format manifest))
                         (= space (:space-id manifest))
                         (= source-sha (get-in manifest [:source :sha256]))
                         (= output-sha (get-in manifest [:output :sha256]))
                         (zero? (get-in manifest [:summary :diagnostic-count] -1)))
            (fail! "migration artifacts failed sealed-manifest verification"
                   {:store store :target target :manifest manifest-path}))
          {:path target :space space :sha256 output-sha
           :manifest manifest :normalization normalization})))))

(defn migrate! [directory]
  (let [state (read-state directory)
        _ (Files/createDirectories (.toPath (io/file directory "clj-config"))
                                   (make-array java.nio.file.attribute.FileAttribute 0))
        migrated (into {} (for [store [:coordination :telemetry]]
                            [store (migrate-one! directory store state)]))]
    (update-state! directory #(-> % (assoc :migrated migrated)
                                  (assoc-in [:steps :migrate]
                                            {:ok true :verified :sealed-manifests})))
    (doseq [[store value] migrated]
      (println (str "MIGRATE " (name store)
                    " descents=" (get-in value [:normalization :nonmonotonic-descents])
                    " tx0=" (get-in value [:normalization :remapped-zero])
                    " framlog-sha256=" (:sha256 value))))))

(defn helper-result [state store command & arguments]
  (let [result
        (process! (str (name store) " FRAMRPC " command)
                  (into [bb "-cp" (str fram "/out")
                         (str root "/cli/framrpc-command.clj") command
                         "127.0.0.1" (str (get-in state [:ports store]))
                         (get-in state [:spaces store])]
                        arguments)
                  root {})]
    (edn/read-string (:stdout result))))

(defn ready-eventually! [state store]
  (loop [attempt 0]
    (let [result (try (helper-result state store "status")
                      (catch Throwable _ nil))]
      (cond
        (= :ready (:state result)) result
        (>= attempt 240)
        (fail! "head daemon did not become FRAMRPC-ready"
               {:store store :port (get-in state [:ports store])})
        :else (do (Thread/sleep 250) (recur (inc attempt)))))))

(defn start-head! [directory state store]
  (let [port (get-in state [:ports store])]
    (if (port-open? port)
      {:pid (get-in state [:heads store :pid])
       :status (ready-eventually! state store) :reused true}
      (let [log (str directory "/" (name store) ".head.log")
            production? (= :production (:mode state))
            unit (str "north-fram-head-" (name store) ".service")
            launch-arguments
            [(str fram "/bin/fram-daemon") "serve" (str port)
             (get-in state [:migrated store :path])
             (get-in state [:spaces store])]
            builder
            (doto (ProcessBuilder. ^java.util.List launch-arguments)
              (.directory (io/file fram))
              (.redirectErrorStream true)
              (.redirectOutput (io/file log)))
            environment (.environment builder)]
        (let [daemon-environment
              {:CLJ_CONFIG (str directory "/clj-config")
               :FRAM_DAEMON_XMX "1g" :FRAM_DAEMON_QUIET "1"
               :FRAM_DAEMON_LOG (str directory "/" (name store) ".requests.log")}
              process
              (if production?
                (let [user (or (System/getenv "USER")
                               (fail! "USER is required for transient head units" {}))
                      group (str/trim
                             (:stdout (process! "resolve operator group"
                                                ["id" "-gn"] root {})))
                      systemd-arguments
                      (into ["sudo" "systemd-run" "--quiet" "--collect"
                             (str "--unit=" unit) (str "--uid=" user)
                             (str "--gid=" group)
                             (str "--working-directory=" fram)
                             "--property=Type=simple"]
                            (concat
                             (map (fn [[key value]]
                                    (str "--setenv=" (name key) "=" value))
                                  daemon-environment)
                             launch-arguments))]
                  (process! (str "start transient " unit) systemd-arguments root {})
                  nil)
                (do
                  (doseq [[key value] daemon-environment]
                    (.put environment (name key) value))
                  (.start builder)))
              status (ready-eventually! state store)]
          {:pid (if process
                  (.pid process)
                  (parse-long
                   (str/trim
                    (:stdout
                     (process! (str "read " unit " MainPID")
                               ["systemctl" "show" unit "-p" "MainPID" "--value"]
                               root {})))))
           :unit (when production? unit)
           :status status :reused false :log (when-not production? log)})))))

(defn boot! [directory]
  (let [state (read-state directory)
        heads (into {} (for [store [:coordination :telemetry]]
                         [store (start-head! directory state store)]))]
    (update-state! directory #(-> % (assoc :heads heads)
                                  (assoc :boot-versions
                                         (into {} (map (fn [[store head]]
                                                        [store (get-in head [:status :served-version])])
                                                      heads)))
                                  (assoc-in [:steps :boot]
                                            {:ok true :verified :framrpc-status})))
    (doseq [[store value] heads]
      (println (str "BOOT " (name store) " pid=" (:pid value)
                    " port=" (get-in state [:ports store])
                    " version=" (get-in value [:status :served-version]))))))

(defn read-triples [path]
  (with-open [reader (io/reader path)]
    (->> (line-seq reader) (remove str/blank?) (map edn/read-string) doall)))

(defn baseline! [directory store state]
  (let [triples (str directory "/" (name store) ".v03-live.edn")
        summary (str directory "/" (name store) ".v03-summary.edn")]
    (process! (str (name store) " frozen v0.3 fold")
              [bb "-cp" v03-out "-e" baseline-expression "--"
               (get-in state [:frozen store :path]) triples summary]
              root {})
    (assoc (edn/read-string (slurp summary)) :triples-path triples)))

(defn canonical-bytes [triples]
  (.getBytes
   (str (str/join "\n" (sort (map pr-str triples)))
        (when (seq triples) "\n"))
   StandardCharsets/UTF_8))

(defn sampled-subjects [triples]
  (->> triples (map first) distinct
       (sort-by (fn [subject]
                  [(sha256-bytes (.getBytes (pr-str subject) StandardCharsets/UTF_8))
                   (pr-str subject)]))
       (take 100) vec))

(defn parity-one! [directory store state]
  (let [baseline (baseline! directory store state)
        expected (read-triples (:triples-path baseline))
        actual-result (helper-result state store "scan-all")
        actual (:rows actual-result)
        status (helper-result state store "status")
        expected-by-subject (group-by first expected)
        actual-by-subject (group-by first actual)
        subjects (sampled-subjects expected)
        sample-mismatches
        (filterv
         identity
         (mapv (fn [subject]
                 (let [left (canonical-bytes (get expected-by-subject subject))
                       right (canonical-bytes (get actual-by-subject subject))]
                   (when-not (Arrays/equals left right)
                     {:subject subject :expected (sha256-bytes left)
                      :actual (sha256-bytes right)})))
               subjects))
        expected-bytes (canonical-bytes expected)
        actual-bytes (canonical-bytes actual)
        report
        {:v03-count (count expected) :framrpc-count (count actual)
         :count-match (= (count expected) (count actual))
         :v03-sha256 (sha256-bytes expected-bytes)
         :framrpc-sha256 (sha256-bytes actual-bytes)
         :full-byte-projection-match (Arrays/equals expected-bytes actual-bytes)
         :sample-subjects (count subjects) :sample-mismatches sample-mismatches
         :sample-byte-match (and (= 100 (count subjects)) (empty? sample-mismatches))
         :status-live-count (:live-count status)
         :scan-raw-live-count (:raw-live-count actual-result)
         :status-scan-match (= (:live-count status) (:raw-live-count actual-result))
         :pages (:pages actual-result) :served-version (:served-version actual-result)}]
    (when-not (every? true? (map report [:count-match :full-byte-projection-match
                                         :sample-byte-match :status-scan-match]))
      (fail! "FRAMRPC parity verification failed" {:store store :report report}))
    report))

(defn verify! [directory]
  (let [state (read-state directory)
        parity (into {} (for [store [:coordination :telemetry]]
                          [store (parity-one! directory store state)]))]
    (update-state! directory #(-> % (assoc :parity parity)
                                  (assoc-in [:steps :verify]
                                            {:ok true :verified :count-and-byte-parity})))
    (doseq [[store value] parity]
      (println (str "PARITY " (name store)
                    " v03=" (:v03-count value)
                    " framrpc=" (:framrpc-count value)
                    " full_sha=" (:framrpc-sha256 value)
                    " sample=" (:sample-subjects value) "/100 byte-exact"
                    " raw/status=" (:scan-raw-live-count value)
                    "/" (:status-live-count value)
                    " pages=" (:pages value))))))

(defn env-content [state]
  (str "# Generated by scripts/framlog-cutover.clj; rollback removes this selector.\n"
       "export NORTH_COORD_PROTOCOL=framrpc\n"
       "export NORTH_FRAMRPC_OUT=" (pr-str (str fram "/out")) "\n"
       "export FRAM_HOME=" (pr-str fram) "\n"
       "export FRAM_BIN=" (pr-str (str fram "/bin")) "\n"
       "export FRAM_OUT=" (pr-str v03-out) "\n"
       "export FRAM_SPACE_ID=north-coordination\n"
       "export NORTH_TELEMETRY_SPACE_ID=north-telemetry\n"
       "export NORTH_TELEMETRY_PARTITION=1\n"
       "export NORTH_PORT=" (get-in state [:ports :coordination]) "\n"
       "export FRAM_PORT=" (get-in state [:ports :coordination]) "\n"
       "export NORTH_TELEMETRY_PORT=" (get-in state [:ports :telemetry]) "\n"
       "export FRAM_LOG=" (pr-str (get-in state [:sources :coordination])) "\n"
       "export FRAM_TELEMETRY_LOG=" (pr-str (get-in state [:sources :telemetry])) "\n"))

(defn switch! [directory]
  (let [state (read-state directory)
        path (:env-file state)
        content (env-content state)]
    (when (and (.exists (io/file path)) (not= content (slurp path)))
      (fail! "FRAMRPC selector exists with different content" {:path path}))
    (spit path content)
    (let [digest (sha256-file path)]
      (update-state! directory #(-> % (assoc :env-sha256 digest)
                                    (assoc-in [:steps :switch]
                                              {:ok true :verified :selector-sha256})))
      (println (str "SWITCH ok — " path " sha256=" digest)))))

(defn north-result [state & arguments]
  (process! (str "north " (str/join " " arguments))
            (into [(str root "/bin/north")] arguments) root
            {:NORTH_FRAMRPC_ENV (:env-file state)
             :NORTH_FAST_LIST_RENDER "1"}))

(defn health! [directory]
  (let [state (read-state directory)
        thread (or (System/getenv "NORTH_CUTOVER_PROBE_THREAD")
                   "019fc335-58fa-734f-b473-38f875f5cbc5")
        predicate "s4_cutover_probe"
        value (str "roundtrip-" (UUID/randomUUID))
        before (into {} (for [store [:coordination :telemetry]]
                          [store (helper-result state store "version")]))
        show (north-result state "json" "show" thread)
        ready (north-result state "json" "ready")
        board (north-result state "json" "board")]
    (when-not (and (str/includes? (:stdout show) "title")
                   (sequential? (json/parse-string (:stdout ready)))
                   (sequential? (json/parse-string (:stdout board))))
      (fail! "North read health probes returned invalid projections"
             {:show (:stdout show) :ready (:stdout ready) :board (:stdout board)}))
    (north-result state "tell" thread predicate value)
    (when-not (str/includes? (:stdout (north-result state "json" "show" thread)) value)
      (fail! "tell probe was not visible through show" {:thread thread}))
    (north-result state "retract" thread predicate value)
    (when (str/includes? (:stdout (north-result state "json" "show" thread)) value)
      (fail! "retracted tell probe remained visible" {:thread thread}))
    (let [after (into {} (for [store [:coordination :telemetry]]
                           [store (helper-result state store "version")]))
          expected {:coordination (+ 2 (:coordination before))
                    :telemetry (:telemetry before)}]
      (when-not (= expected after)
        (fail! "health probes moved an unexpected native version"
               {:before before :expected expected :after after}))
      (update-state! directory #(-> % (assoc :health-versions after)
                                    (assoc-in [:steps :health]
                                              {:ok true :verified :core-roundtrip})))
      (println (str "HEALTH show=ok ready=ok board=ok tell=assert+show+retract"
                    " versions=" (pr-str after))))))

(defn stop-pid! [pid]
  (when (and pid (pos? pid))
    (let [proc-path (io/file (str "/proc/" pid))]
      (when (.exists proc-path)
        (process-result ["kill" "-TERM" (str pid)] root {})
        (loop [attempt 0]
          (when (and (.exists proc-path) (< attempt 50))
            (Thread/sleep 100)
            (recur (inc attempt))))
        (when (.exists proc-path)
          (process-result ["kill" "-KILL" (str pid)] root {}))))))

(defn stop-heads! [state]
  (if (= :production (:mode state))
    (doseq [store [:telemetry :coordination]
            :let [unit (or (get-in state [:heads store :unit])
                           (str "north-fram-head-" (name store) ".service"))]]
      (process-result ["sudo" "systemctl" "stop" unit] root {}))
    (doseq [store [:telemetry :coordination]]
      (stop-pid! (get-in state [:heads store :pid]))))
  (doseq [[store port] (:ports state)]
    (when (port-open? port)
      (fail! "head listener remained after stop" {:store store :port port}))))

(defn safe-rollback-version! [state]
  (when (every? #(port-open? (get-in state [:ports %]))
                [:coordination :telemetry])
    (let [current (into {} (for [store [:coordination :telemetry]]
                             [store (helper-result state store "version")]))
          expected (or (:health-versions state) (:boot-versions state))]
      (when-not (= expected current)
        (fail! "rollback refused after a non-probe native write; fix forward"
               {:expected expected :current current})))))

(defn restore-frozen! [state]
  (doseq [store [:coordination :telemetry]]
    (let [source (get-in state [:sources store])
          frozen (get-in state [:frozen store :path])]
      (Files/copy (.toPath (io/file frozen)) (.toPath (io/file source))
                  (into-array StandardCopyOption
                              [StandardCopyOption/REPLACE_EXISTING]))
      (when-not (= (sha256-file source) (get-in state [:frozen store :sha256]))
        (fail! "rollback restore checksum mismatch" {:store store :source source})))))

(defn rollback! [directory]
  (let [state (read-state directory)
        env-file (:env-file state)]
    (safe-rollback-version! state)
    (stop-heads! state)
    (when (.exists (io/file env-file))
      (when-not (= (:env-sha256 state) (sha256-file env-file))
        (fail! "rollback selector differs from the installed cutover selector"
               {:path env-file}))
      (Files/move (.toPath (io/file env-file))
                  (.toPath (io/file (str directory "/framrpc.env.rolled-back")))
                  (into-array StandardCopyOption [StandardCopyOption/REPLACE_EXISTING])))
    (restore-frozen! state)
    (when (= :production (:mode state))
      (process! "restart v0.3 socket-activated pair"
                ["sudo" "systemctl" "start" "north-coord.socket"
                 "north-telemetry-coord.socket" "north-coord-pair.target"] root {}))
    (update-state! directory assoc-in [:steps :rollback]
                   {:ok true :verified :frozen-restored})
    (println "ROLLBACK ok — heads stopped, selector removed, frozen v0.3 logs restored")))

(defn run! [directory]
  (doseq [step [quiesce! freeze! migrate! boot! verify! switch! health!]]
    (step directory)))

(defn prepare-rehearsal! [directory]
  (when (.exists (io/file directory))
    (fail! "rehearsal target must not exist" {:directory directory}))
  (Files/createDirectories (.toPath (io/file directory "live"))
                           (make-array java.nio.file.attribute.FileAttribute 0))
  (let [input-coordination (or (System/getenv "NORTH_CUTOVER_COORD_LOG")
                               (str state-home "/coordination.log"))
        input-telemetry (or (System/getenv "NORTH_CUTOVER_TELEMETRY_LOG")
                            (str state-home "/telemetry.log"))
        live-coordination (str directory "/live/coordination.log")
        live-telemetry (str directory "/live/telemetry.log")]
    (Files/copy (.toPath (io/file input-coordination))
                (.toPath (io/file live-coordination))
                (into-array StandardCopyOption []))
    (Files/copy (.toPath (io/file input-telemetry))
                (.toPath (io/file live-telemetry))
                (into-array StandardCopyOption []))
    (let [first-port (free-port)
          second-port (loop [candidate (free-port)]
                        (if (= candidate first-port) (recur (free-port)) candidate))]
      (write-state!
       directory
       (base-state directory :rehearsal
                   {:coordination live-coordination :telemetry live-telemetry}
                   {:coordination first-port :telemetry second-port})))))

(defn rehearse! [directory]
  (prepare-rehearsal! directory)
  (try
    (run! directory)
    (rollback! directory)
    (let [state (read-state directory)]
      (println (str "FINAL " (pr-str (select-keys state [:thread :parity :health-versions :steps])))))
    (catch Throwable error
      (try (stop-heads! (read-state directory)) (catch Throwable _ nil))
      (throw error))))

(defn usage! []
  (binding [*out* *err*]
    (println
     (str "usage: scripts/framlog-cutover.clj COMMAND CUTOVER_DIRECTORY\n"
          "  production: quiesce | freeze | migrate | boot | verify | switch | health | rollback | run\n"
          "  copy-only:  rehearse (directory must not exist)")))
  (System/exit 2))

(try
  (let [[command directory & extra] *command-line-args*]
    (when (or (nil? directory) (seq extra)) (usage!))
    (let [directory (.getCanonicalPath (io/file directory))]
      (case command
        "rehearse" (rehearse! directory)
        "quiesce" (do (ensure-production-state! directory) (quiesce! directory))
        "freeze" (freeze! directory)
        "migrate" (migrate! directory)
        "boot" (boot! directory)
        "verify" (verify! directory)
        "switch" (switch! directory)
        "health" (health! directory)
        "rollback" (rollback! directory)
        "run" (do (ensure-production-state! directory) (run! directory))
        (usage!))))
  (catch Throwable error
    (binding [*out* *err*]
      (prn {:error (or (:type (ex-data error)) :cutover-failed)
            :message (.getMessage error) :data (ex-data error)}))
    (System/exit 1)))
