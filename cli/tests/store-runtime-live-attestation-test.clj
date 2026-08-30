(ns north.store-runtime-live-attestation-test
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [north.store-runtime-manifest :as manifest])
  (:import [java.io File]
           [java.nio.file Files]
           [java.nio.file Paths]
           [java.nio.file.attribute FileAttribute]))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/runtime-attestation.clj"))

(load-file (str root "/cli/store-runtime-generation.clj"))

(defn loaded-var [^String namespace-name ^String var-name]
  (or (ns-resolve (symbol namespace-name) (symbol var-name)) (throw (ex-info "required loaded var is unavailable" {:namespace namespace-name :var var-name}))))

(def active-generation-evidence! (loaded-var "north.runtime-attestation" "active-generation-evidence!"))

(def systemd-properties-var (loaded-var "north.runtime-attestation" "systemd-properties"))

(def systemd-main-pid! (loaded-var "north.runtime-attestation" "systemd-main-pid!"))

(def allow-controller-starting-var (loaded-var "north.runtime-attestation" "*allow-controller-starting?*"))

(def listener-pids-var (loaded-var "north.runtime-attestation" "listener-pids"))

(def process-birth-token-var (loaded-var "north.runtime-attestation" "process-birth-token"))

(def process-start-millis-var (loaded-var "north.runtime-attestation" "process-start-millis"))

(def process-path-var (loaded-var "north.runtime-attestation" "process-path"))

(def process-cmdline-var (loaded-var "north.runtime-attestation" "process-cmdline"))

(def process-environment-var (loaded-var "north.runtime-attestation" "process-environment"))

(def jvm-runtime-facts! (loaded-var "north.runtime-attestation" "jvm-runtime-facts!"))

(def launch-spec! (loaded-var "north.runtime-attestation" "launch-spec!"))

(def publish-runtime-record! (loaded-var "north.runtime-attestation" "publish-runtime-record!"))

(def ^String jvm-runtime-record-format (var-get (loaded-var "north.runtime-attestation" "jvm-runtime-record-format")))

(def ^String managed-native-runtime-record-format (var-get (loaded-var "north.runtime-attestation" "managed-native-runtime-record-format")))

(def store-status-command! (loaded-var "north.store-runtime-generation" "store-status-command!"))

(def babashka-executable (loaded-var "north.store-runtime-generation" "babashka-executable"))

(def ^String output "/nix/store/ab44cw87jg2dlwlljwqahbsa91fliv67-beagle-store-jvm-composite-1-318ff1cb0b237800c8f91db3a9d1d1f5372fdfdb")

(def ^String manifest-text (slurp (manifest/manifest-path-for output)))

(def jvm (manifest/accepted-jvm-runtime! output manifest/accepted-jvm-nar-sha256 manifest/accepted-jvm-manifest-sha256 manifest-text))

(def native manifest/accepted-native-runtime)

(def ^String native-release-root (let [match__0 native]
  (cond
    (instance? north.store-runtime-manifest.Native match__0) (let [release-root (:release-root match__0) _ (:beagle-revision match__0) _ (:beagle-tree match__0) _ (:artifact-root match__0) _ (:closure-sha256 match__0) _ (:server-artifact match__0) _ (:server-sha256 match__0)] release-root)
    (instance? north.store-runtime-manifest.JVM match__0) (let [_ (:output match__0) _ (:package-nar-sha256 match__0) _ (:beagle-revision match__0) _ (:beagle-tree match__0) _ (:manifest-path match__0) _ (:manifest-bytes match__0) _ (:manifest-sha256 match__0) _ (:manifest match__0)] (throw (ex-info "accepted Native runtime has JVM shape" {}))))))

(def ^String native-server-artifact (let [match__1 native]
  (cond
    (instance? north.store-runtime-manifest.Native match__1) (let [_ (:release-root match__1) _ (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:artifact-root match__1) _ (:closure-sha256 match__1) server-artifact (:server-artifact match__1) _ (:server-sha256 match__1)] server-artifact)
    (instance? north.store-runtime-manifest.JVM match__1) (let [_ (:output match__1) _ (:package-nar-sha256 match__1) _ (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:manifest-path match__1) _ (:manifest-bytes match__1) _ (:manifest-sha256 match__1) _ (:manifest match__1)] (throw (ex-info "accepted Native runtime has JVM shape" {}))))))

(def jvm-generation (manifest/initial-promotion-transition! jvm))

(def native-generation (manifest/rollback-transition! jvm-generation))

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj (->Check label (boolean value)))
  nil))

(defn denied-type [operation]
  (try
  (do
  (operation)
  nil)
  (catch Throwable error
    (:type (ex-data error)))))

(defn delete-tree! [file]
  (do
  (if (and (.isDirectory file) (not (Files/isSymbolicLink (.toPath file)))) (do
  (doseq [child (or (.listFiles file) (make-array File 0))]
  (delete-tree! child))))
  (Files/deleteIfExists (.toPath file))
  nil))

(check! "status probes use the client matching the selected member" (and (= [(manifest/jvm-dispatcher-path-for output) "store" "status"] (store-status-command! jvm)) (= [(babashka-executable) "-cp" (manifest/native-client-classpath-for native-release-root) (manifest/native-client-path-for native-release-root) "status"] (store-status-command! native))))

(defn select-generation! [state-root runtime-generation ^String id]
  (let [generations (io/file state-root "generations")
   generation-root (io/file generations id)
   record (io/file generation-root "generation.edn")
   selector (io/file state-root "active")]
  (.mkdirs generation-root)
  (spit record (str (pr-str runtime-generation) "\n"))
  (Files/deleteIfExists (.toPath selector))
  (Files/createSymbolicLink (.toPath selector) (Paths/get (str "generations/" id) (make-array String 0)) (make-array FileAttribute 0))
  (active-generation-evidence! (.getCanonicalPath state-root))))

(defn jvm-process-arguments [facts port ^String log ^String space-id]
  [(:java facts) "-Xmx2g" "-XX:+UseG1GC" "-XX:+ExitOnOutOfMemoryError" "-XX:+HeapDumpOnOutOfMemoryError" (str "-XX:HeapDumpPath=" log ".requests.log.heap.hprof") "-cp" (:classpath facts) "clojure.main" "server.clj" "serve" (str port) log space-id])

(let [^String unit "north-store.service"
   starting {"Id" unit "LoadState" "loaded" "ActiveState" "activating" "SubState" "start-post" "MainPID" "4343"}
   properties (fn [_ignored] {:exit 0 :error "" :values starting})]
  (check! "ExecStartPost may bind the exact starting MainPID" (= 4343 (with-redefs-fn {systemd-properties-var properties} (fn [] (with-bindings {allow-controller-starting-var true} (systemd-main-pid! unit))))))
  (check! "ordinary attestation still requires the active running unit" (= :runtime-controller-invalid (with-redefs-fn {systemd-properties-var properties} (fn [] (denied-type (fn [] (systemd-main-pid! unit))))))))

(let [temp (.toFile (Files/createTempDirectory "north-store-managed-attestation-" (make-array FileAttribute 0)))
   state-root (io/file temp "store-runtime")
   ^String record (.getCanonicalPath (io/file temp "north-store.runtime"))
   ^String log (.getCanonicalPath (io/file temp "coordination.storelog"))
   port 47979
   ^String space-id "north-coordination"
   ^String unit "north-store.service"
   pid 4343
   ^String birth "proc:12345"
   selection {"BEAGLE_STORE_SPACE_ID" space-id "BEAGLE_STORE_SERVER_PORT" (str port) "BEAGLE_STORE_LOG" log}
   base-redefs {listener-pids-var (fn [_port] [pid]) process-birth-token-var (fn [_pid] birth) process-start-millis-var (fn [_pid] 123456789) systemd-main-pid! (fn [_unit] pid)}]
  (try
  (.mkdirs state-root)
  (spit log "STORELOG fixture\n")
  (let [evidence (select-generation! state-root jvm-generation "jvm")
   facts (jvm-runtime-facts! jvm)
   spec (launch-spec! jvm selection)
   environment (assoc (:environment spec) "NORTH_COORD_SYSTEMD_UNIT" unit)
   redefs (merge base-redefs {process-path-var (fn [_pid ^String leaf] (case leaf
    "cwd" (:home facts)
    "exe" (:java-executable facts)
    nil)) process-cmdline-var (fn [_pid] (jvm-process-arguments facts port log space-id)) process-environment-var (fn [_pid] environment)})
   verified (with-redefs-fn redefs (fn [] (publish-runtime-record! {:member jvm :generation-evidence evidence :pid pid :controller-unit unit :record-path record :selection selection})))]
  (check! "JVM launch derives the immutable package server and JVM environment" (and (= (manifest/jvm-server-launcher-for output) (:executable spec)) (= "jvm" (get-in spec [:environment "BEAGLE_STORE_SERVER_RUNTIME"])) (= (str log ".requests.log") (get-in spec [:environment "BEAGLE_STORE_SERVER_LOG"])) (= (:java facts) (get-in spec [:environment "BEAGLE_STORE_JAVA"]))))
  (check! "JVM producer binds selected generation to the actual listener" (and (= "jvm" (get-in verified [:identity :runtime-kind])) (= output (get-in verified [:identity :output])) (= jvm-generation (get-in verified [:identity :generation :generation])) (= jvm-runtime-record-format (-> record slurp str/split-lines first (str/replace "FORMAT=" "")))))
  (let [native-evidence (select-generation! state-root native-generation "native")]
  (check! "record publication rejects a valid member that is not selected current" (= :runtime-record-invalid (with-redefs-fn base-redefs (fn [] (denied-type (fn [] (publish-runtime-record! {:member jvm :generation-evidence native-evidence :pid pid :controller-unit unit :record-path record :selection selection})))))))))
  (let [evidence (select-generation! state-root native-generation "native-2")
   spec (launch-spec! native selection)
   environment (assoc (:environment spec) "NORTH_COORD_SYSTEMD_UNIT" unit)
   redefs (merge base-redefs {process-path-var (fn [_pid ^String leaf] (case leaf
    "cwd" native-release-root
    "exe" native-server-artifact
    nil)) process-cmdline-var (fn [_pid] [native-server-artifact]) process-environment-var (fn [_pid] environment)})
   verified (with-redefs-fn redefs (fn [] (publish-runtime-record! {:member native :generation-evidence evidence :pid pid :controller-unit unit :record-path record :selection selection})))]
  (check! "Native rollback producer binds selected generation to the actual listener" (and (= "native" (get-in verified [:identity :runtime-kind])) (= native-generation (get-in verified [:identity :generation :generation])) (= managed-native-runtime-record-format (-> record slurp str/split-lines first (str/replace "FORMAT=" ""))))))
  (finally
    (delete-tree! temp))))

(let [results (deref checks)
   passed (count (filter (fn [^Check result] (check-passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (check-passed result) "PASS" "FAIL") (check-label result))))
  (println (format "\nManaged runtime attestation: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
