#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/runtime-attestation.clj"))
(require '[north.runtime-attestation :as attestation])

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn sha256-file [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")
        buffer (byte-array 65536)]
    (with-open [input (io/input-stream path)]
      (loop []
        (let [n (.read input buffer)]
          (when (pos? n)
            (.update digest buffer 0 n)
            (recur)))))
    (apply str (map #(format "%02x" (bit-and (int %) 255)) (.digest digest)))))

(defn delete-tree! [file]
  (when (and (.isDirectory file)
             (not (java.nio.file.Files/isSymbolicLink (.toPath file))))
    (doseq [child (or (.listFiles file) (make-array java.io.File 0))]
      (delete-tree! child)))
  (java.nio.file.Files/deleteIfExists (.toPath file)))

(defn set-mode! [path permissions]
  (java.nio.file.Files/setPosixFilePermissions
   (.toPath (io/file path))
   (java.util.HashSet. ^java.util.Collection permissions)))

(def record-permissions
  [java.nio.file.attribute.PosixFilePermission/OWNER_READ
   java.nio.file.attribute.PosixFilePermission/OWNER_WRITE])
(def artifact-permissions
  [java.nio.file.attribute.PosixFilePermission/OWNER_READ
   java.nio.file.attribute.PosixFilePermission/OWNER_EXECUTE])

(defn write-record! [path values]
  (spit path
        (str (str/join "\n"
                       (map #(str % "=" (get values %))
                            attestation/runtime-record-order))
             "\n"))
  (set-mode! path record-permissions)
  path)

(defn write-receipt! [path values]
  (spit path
        (str (str/join "\n"
                       (map #(str % "=" (get values %))
                            attestation/release-receipt-order))
             "\n"))
  path)

(defn denied-type [operation]
  (try
    (operation)
    nil
    (catch clojure.lang.ExceptionInfo error (:type (ex-data error)))))

(let [temp (.toFile
            (java.nio.file.Files/createTempDirectory
             "north-store-runtime-attestation-"
             (make-array java.nio.file.attribute.FileAttribute 0)))
      source (.getCanonicalPath (io/file temp "store"))
      artifact-directory (.getCanonicalPath (io/file temp "native-build"))
      artifact (.getCanonicalPath
                (io/file artifact-directory "bin" "beagle-store-server-native"))
      ready (.getCanonicalPath (io/file artifact-directory "READY"))
      input-manifest
      (.getCanonicalPath (io/file artifact-directory "input.manifest"))
      receipt (.getCanonicalPath (io/file source attestation/release-receipt-name))
      log (.getCanonicalPath (io/file temp "coordination.storelog"))
      record (.getCanonicalPath (io/file temp "north-store.runtime"))
      revision (apply str (repeat 40 "a"))
      tree (apply str (repeat 40 "b"))
      manifest-payload "sealed Native input manifest fixture\n"
      space-id "north-coordination"
      port 47977
      pid 4242
      birth "proc:987654"
      unit "north-store.service"
      closure-sha256
      (atom nil)
      values
      (atom nil)
      release-redef {#'attestation/sealed-release-home (fn [] source)}
      attest-record!
      (fn []
        (with-redefs-fn release-redef
          #(attestation/attest-active-runtime!
            {:port port :served-log log :space-id space-id
             :record-path record :controller-unit unit})))]
  (try
    (.mkdirs (io/file source))
    (.mkdirs (.getParentFile (io/file artifact)))
    (spit input-manifest manifest-payload)
    (reset! closure-sha256 (sha256-file input-manifest))
    (spit ready (str "beagle-store-native-build/v1 " @closure-sha256 "\n"))
    (set-mode! ready record-permissions)
    (spit artifact "sealed executable fixture\n")
    (set-mode! artifact artifact-permissions)
    (spit log "STORELOG fixture\n")
    (write-receipt! receipt
                    {"format" attestation/release-receipt-format
                     "source" "/var/empty/store-cut-from"
                     "revision" revision
                     "tree" tree
                     "native_artifact_dir" artifact-directory
                     "native_closure_sha256" @closure-sha256
                     "server_artifact_sha256" (sha256-file artifact)
                     "created" "2026-08-09T20:58:38+08:00"})
    (reset!
     values
     {"FORMAT" attestation/active-runtime-record-format
      "BEAGLE_STORE_SOURCE" source
      "BEAGLE_STORE_REVISION" revision
      "BEAGLE_STORE_TREE" tree
      "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" artifact-directory
      "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" @closure-sha256
      "BEAGLE_STORE_SERVER_ARTIFACT" artifact
      "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" (sha256-file artifact)
      "BEAGLE_STORE_SPACE_ID" space-id
      "BEAGLE_STORE_PORT" (str port)
      "BEAGLE_STORE_LOG" log
      "PID" (str pid)
      "PID_BIRTH" birth
      "CONTROLLER_UNIT" unit
      "CONTROLLER_MAIN_PID" (str pid)})
    (write-record! record @values)
    (let [environment
          {"BEAGLE_STORE_HOME" source
           "BEAGLE_STORE_SERVER_RUNTIME" "native"
           "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" artifact-directory
           "BEAGLE_STORE_SPACE_ID" space-id
           "BEAGLE_STORE_SERVER_PORT" (str port)
           "BEAGLE_STORE_LOG" log
           "NORTH_COORD_SYSTEMD_UNIT" unit}
          valid-redefs
          {#'attestation/sealed-release-home (fn [] source)
           #'attestation/listener-pids (fn [_] [pid])
           #'attestation/process-birth-token (fn [_] birth)
           #'attestation/process-start-millis (fn [_] 123456789)
           #'attestation/systemd-main-pid! (fn [_] pid)
           #'attestation/process-path
           (fn [_ leaf] (case leaf "cwd" source "exe" artifact nil))
           #'attestation/process-cmdline
           (fn [_] [artifact])
           #'attestation/process-environment (fn [_] environment)}
          request {:port port :served-log log :space-id space-id
                   :record-path record :controller-unit unit}
          verified
          (with-redefs-fn valid-redefs
            #(attestation/attest-active-runtime! request))]
      (check! "canonical record binds exact source, artifact, STORELOG, and SpaceId"
              (and (= attestation/attestation-format (:format verified))
                   (= artifact-directory
                      (get-in verified [:identity :native-artifact :directory]))
                   (= @closure-sha256
                      (get-in verified
                              [:identity :native-artifact :closure-sha256]))
                   (= artifact
                      (get-in verified [:identity :native-artifact :server :path]))
                   (= space-id (get-in verified [:identity :space-id]))
                   (= pid (get-in verified [:authority :pid]))))
      (check! "unchanged canonical runtime re-attests"
              (true?
               (with-redefs-fn valid-redefs
                 #(attestation/assert-current! verified))))

      (check! "any argument after argv[0] is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-cmdline
                          (fn [_] [artifact ""]))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "the sealed release receipt supplies the expected revision and tree"
              (and (= revision (get-in verified [:identity :revision]))
                   (= tree (get-in verified [:identity :tree]))
                   (= source (get-in verified [:identity :source]))
                   ;; provenance only: the checkout a release was cut from is
                   ;; mutable and is never resolved
                   (= "/var/empty/store-cut-from"
                      (get-in verified [:identity :cut-from]))))

      (check! "selection-file variables consistent with the sealed release are accepted"
              (map? (with-redefs-fn
                      (assoc valid-redefs #'attestation/process-environment
                             (fn [_]
                               (assoc environment
                                      "BEAGLE_STORE_SERVER_ARTIFACT" artifact
                                      "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" (sha256-file artifact)
                                      "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" @closure-sha256
                                      "BEAGLE_STORE_BIN" (str source "/bin")
                                      "BEAGLE_STORE_OUT" (str source "/out")
                                      "NORTH_PORT" (str port)
                                      "BEAGLE_STORE_MAX_ACTIVE_CLIENTS" "64"
                                      "NORTH_TELEMETRY_PORT" "7978")))
                      #(attestation/attest-active-runtime! request))))

      (check! "an environment that redirects the sealed artifact is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-environment
                          (fn [_] (assoc environment
                                         "BEAGLE_STORE_SERVER_ARTIFACT" "/tmp/other-server")))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "a socket-activation or JVM selector in the environment is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-environment
                          (fn [_] (assoc environment "BEAGLE_STORE_LISTEN_FD" "3")))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "a missing required identity variable is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-environment
                          (fn [_] (dissoc environment "NORTH_COORD_SYSTEMD_UNIT")))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "a second listener owner is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/listener-pids
                          (fn [_] [pid (inc pid)]))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "systemd MainPID disagreement is rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/systemd-main-pid!
                          (fn [_] (inc pid)))
                   #(denied-type
                     (fn [] (attestation/attest-active-runtime! request))))))

      (check! "process replacement invalidates captured authority"
              (= :runtime-authority-lost
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-birth-token
                          (fn [_] "proc:replacement"))
                   #(denied-type
                     (fn [] (attestation/assert-current! verified)))))))

    (write-record! record (assoc @values "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"
                                 (apply str (repeat 64 "0"))))
    (check! "artifact digest disagreement is rejected"
            (= :runtime-record-invalid
               (denied-type attest-record!)))

    (write-record! record @values)
    (spit ready (str "beagle-store-native-build/v1 " @closure-sha256))
    (check! "READY receipt must have the exact closure and final LF"
            (= :runtime-record-invalid
               (denied-type attest-record!)))
    (spit ready (str "beagle-store-native-build/v1 " @closure-sha256 "\n"))

    (spit input-manifest "changed Native input manifest fixture\n")
    (check! "input manifest must hash to the READY closure"
            (= :runtime-record-invalid
               (denied-type attest-record!)))
    (spit input-manifest manifest-payload)

    (spit record
          (str "FORMAT=" attestation/active-runtime-record-format "\n"
               "BEAGLE_STORE_SOURCE=" source "\n"))
    (set-mode! record record-permissions)
    (check! "partial runtime identity is rejected before process trust"
            (= :runtime-record-invalid
               (denied-type attest-record!)))

    ;; The staleness case the hardcoded literal used to cause: the selected
    ;; release moves on and the record still names the previous engine.
    (write-record! record @values)
    (write-receipt! receipt
                    {"format" attestation/release-receipt-format
                     "source" "/var/empty/store-cut-from"
                     "revision" (apply str (repeat 40 "c"))
                     "tree" (apply str (repeat 40 "d"))
                     "native_artifact_dir" artifact-directory
                     "native_closure_sha256" @closure-sha256
                     "server_artifact_sha256" (sha256-file artifact)
                     "created" "2026-08-10T00:00:00+08:00"})
    (check! "a record naming a superseded engine generation is rejected"
            (= :runtime-record-invalid
               (denied-type attest-record!)))

    (spit receipt "format=north-store-release/v1\nrevision=nope\n")
    (check! "a malformed sealed release receipt is rejected"
            (= :runtime-release-invalid
               (denied-type attest-record!)))

    (java.nio.file.Files/delete (.toPath (io/file receipt)))
    (check! "a selected release without a receipt is rejected"
            (= :runtime-path-invalid
               (denied-type attest-record!)))
    (finally
      (delete-tree! temp))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nCanonical runtime attestation: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
