#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[north.store-runtime-manifest :as manifest])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/runtime-attestation.clj"))
(require '[north.runtime-attestation :as attestation])
(load-file (str root "/cli/store-runtime-generation.clj"))
(require '[north.store-runtime-generation :as generation])

(def output
  "/nix/store/pk6fk3pv3vmq1kr4nkrhl6n3flpyx67q-beagle-store-jvm-composite-1-4aa8bcce8e6ea67d8767b43a5cf1152d424d253f")
(def manifest-text (slurp (manifest/manifest-path-for output)))
(def jvm
  (manifest/accepted-jvm-runtime!
   output manifest/accepted-jvm-nar-sha256
   manifest/accepted-jvm-manifest-sha256 manifest-text))
(def native manifest/accepted-native-runtime)
(def jvm-generation (manifest/initial-promotion-transition! jvm))
(def native-generation (manifest/rollback-transition! jvm-generation))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn denied-type [operation]
  (try
    (operation)
    nil
    (catch clojure.lang.ExceptionInfo error (:type (ex-data error)))))

(defn delete-tree! [file]
  (when (and (.isDirectory file)
             (not (java.nio.file.Files/isSymbolicLink (.toPath file))))
    (doseq [child (or (.listFiles file) (make-array java.io.File 0))]
      (delete-tree! child)))
  (java.nio.file.Files/deleteIfExists (.toPath file)))

(check! "status probes use the client matching the selected member"
        (and (= [(manifest/jvm-dispatcher-path-for output) "store" "status"]
                (generation/store-status-command! jvm))
             (= [(generation/babashka-executable)
                 "-cp"
                 (manifest/native-client-classpath-for
                  (:release-root native))
                 (manifest/native-client-path-for (:release-root native))
                 "status"]
                (generation/store-status-command! native))))

(defn select-generation! [state-root generation id]
  (let [generations (io/file state-root "generations")
        generation-root (io/file generations id)
        record (io/file generation-root "generation.edn")
        selector (io/file state-root "active")]
    (.mkdirs generation-root)
    (spit record (str (pr-str generation) "\n"))
    (java.nio.file.Files/deleteIfExists (.toPath selector))
    (java.nio.file.Files/createSymbolicLink
     (.toPath selector)
     (java.nio.file.Paths/get (str "generations/" id) (make-array String 0))
     (make-array java.nio.file.attribute.FileAttribute 0))
    (attestation/active-generation-evidence! (.getCanonicalPath state-root))))

(defn jvm-process-arguments [facts port log space-id]
  [(:java facts)
   "-Xmx2g" "-XX:+UseG1GC" "-XX:G1HeapRegionSize=32m"
   "-XX:+ExitOnOutOfMemoryError" "-XX:+HeapDumpOnOutOfMemoryError"
   (str "-XX:HeapDumpPath=" log ".heap.hprof")
   "-cp" (:classpath facts)
   "clojure.main" "server.clj" "serve" (str port) log space-id])

(let [unit "north-store.service"
      starting {"Id" unit "LoadState" "loaded" "ActiveState" "activating"
                "SubState" "start-post" "MainPID" "4343"}
      properties (fn [_] {:exit 0 :error "" :values starting})]
  (check! "ExecStartPost may bind the exact starting MainPID"
          (= 4343
             (with-redefs-fn
               {#'attestation/systemd-properties properties}
               #(binding [attestation/*allow-controller-starting?* true]
                  (attestation/systemd-main-pid! unit)))))
  (check! "ordinary attestation still requires the active running unit"
          (= :runtime-controller-invalid
             (with-redefs-fn
               {#'attestation/systemd-properties properties}
               #(denied-type
                 (fn [] (attestation/systemd-main-pid! unit)))))))

(let [temp (.toFile
            (java.nio.file.Files/createTempDirectory
             "north-store-managed-attestation-"
             (make-array java.nio.file.attribute.FileAttribute 0)))
      state-root (io/file temp "store-runtime")
      record (.getCanonicalPath (io/file temp "north-store.runtime"))
      log (.getCanonicalPath (io/file temp "coordination.storelog"))
      port 47979
      space-id "north-coordination"
      unit "north-store.service"
      pid 4343
      birth "proc:12345"
      selection {"BEAGLE_STORE_SPACE_ID" space-id
                 "BEAGLE_STORE_SERVER_PORT" (str port)
                 "BEAGLE_STORE_LOG" log}
      base-redefs {#'attestation/listener-pids (fn [_] [pid])
                   #'attestation/process-birth-token (fn [_] birth)
                   #'attestation/process-start-millis (fn [_] 123456789)
                   #'attestation/systemd-main-pid! (fn [_] pid)}]
  (try
    (.mkdirs state-root)
    (spit log "STORELOG fixture\n")
    (let [evidence (select-generation! state-root jvm-generation "jvm")
          facts (attestation/jvm-runtime-facts! jvm)
          spec (attestation/launch-spec! jvm selection)
          environment (assoc (:environment spec)
                             "NORTH_COORD_SYSTEMD_UNIT" unit)
          redefs
          (merge base-redefs
                 {#'attestation/process-path
                  (fn [_ leaf]
                    (case leaf
                      "cwd" (:home facts)
                      "exe" (:java-executable facts)
                      nil))
                  #'attestation/process-cmdline
                  (fn [_]
                    (jvm-process-arguments facts port log space-id))
                  #'attestation/process-environment (fn [_] environment)})
          verified
          (with-redefs-fn
            redefs
            #(attestation/publish-runtime-record!
              {:member jvm :generation-evidence evidence :pid pid
               :controller-unit unit :record-path record
               :selection selection}))]
      (check! "JVM launch derives the immutable package server and JVM environment"
              (and (= (manifest/jvm-server-launcher-for output)
                      (:executable spec))
                   (= "jvm" (get-in spec [:environment
                                           "BEAGLE_STORE_SERVER_RUNTIME"]))
                   (= (:java facts)
                      (get-in spec [:environment "BEAGLE_STORE_JAVA"]))))
      (check! "JVM producer binds selected generation to the actual listener"
              (and (= "jvm" (get-in verified [:identity :runtime-kind]))
                   (= output (get-in verified [:identity :output]))
                   (= jvm-generation
                      (get-in verified [:identity :generation :generation]))
                   (= attestation/jvm-runtime-record-format
                      (-> record slurp str/split-lines first
                          (str/replace "FORMAT=" "")))))

      (let [native-evidence
            (select-generation! state-root native-generation "native")]
        (check! "record publication rejects a valid member that is not selected current"
                (= :runtime-record-invalid
                   (with-redefs-fn
                     base-redefs
                     #(denied-type
                       (fn []
                         (attestation/publish-runtime-record!
                          {:member jvm :generation-evidence native-evidence
                           :pid pid :controller-unit unit :record-path record
                           :selection selection}))))))))

    (let [evidence (select-generation! state-root native-generation "native-2")
          spec (attestation/launch-spec! native selection)
          environment (assoc (:environment spec)
                             "NORTH_COORD_SYSTEMD_UNIT" unit)
          redefs
          (merge base-redefs
                 {#'attestation/process-path
                  (fn [_ leaf]
                    (case leaf
                      "cwd" (:release-root native)
                      "exe" (:server-artifact native)
                      nil))
                  #'attestation/process-cmdline
                  (fn [_] [(:server-artifact native)])
                  #'attestation/process-environment (fn [_] environment)})
          verified
          (with-redefs-fn
            redefs
            #(attestation/publish-runtime-record!
              {:member native :generation-evidence evidence :pid pid
               :controller-unit unit :record-path record
               :selection selection}))]
      (check! "Native rollback producer binds selected generation to the actual listener"
              (and (= "native" (get-in verified [:identity :runtime-kind]))
                   (= native-generation
                      (get-in verified [:identity :generation :generation]))
                   (= attestation/managed-native-runtime-record-format
                      (-> record slurp str/split-lines first
                          (str/replace "FORMAT=" ""))))))
    (finally
      (delete-tree! temp))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nManaged runtime attestation: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
