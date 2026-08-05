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

(defn denied-type [operation]
  (try
    (operation)
    nil
    (catch clojure.lang.ExceptionInfo error (:type (ex-data error)))))

(let [temp (.toFile
            (java.nio.file.Files/createTempDirectory
             "north-framrpc-runtime-attestation-"
             (make-array java.nio.file.attribute.FileAttribute 0)))
      source (.getCanonicalPath (io/file temp "fram"))
      artifact (.getCanonicalPath (io/file temp "fram-server-graal"))
      log (.getCanonicalPath (io/file temp "coordination.framlog"))
      record (.getCanonicalPath (io/file temp "north-coord.runtime"))
      revision (apply str (repeat 40 "a"))
      tree (apply str (repeat 40 "b"))
      space-id "north-coordination"
      port 47977
      pid 4242
      birth "proc:987654"
      unit "north-coord.service"
      source-identity-var
      (ns-resolve 'north.runtime-attestation 'source-identity!)
      values
      (atom nil)]
  (try
    (.mkdirs (io/file source))
    (spit artifact "sealed executable fixture\n")
    (set-mode! artifact artifact-permissions)
    (spit log "FRAMLOG fixture\n")
    (reset!
     values
     {"FORMAT" attestation/active-runtime-record-format
      "FRAM_SOURCE" source
      "FRAM_REVISION" revision
      "FRAM_TREE" tree
      "FRAM_ARTIFACT" artifact
      "FRAM_ARTIFACT_SHA256" (sha256-file artifact)
      "FRAM_SPACE_ID" space-id
      "FRAM_PORT" (str port)
      "FRAM_LOG" log
      "PID" (str pid)
      "PID_BIRTH" birth
      "CONTROLLER_UNIT" unit
      "CONTROLLER_MAIN_PID" (str pid)})
    (write-record! record @values)
    (let [environment
          {"FRAM_HOME" source
           "FRAM_SERVER_RUNTIME" "graal"
           "FRAM_GRAAL_ARTIFACT" artifact
           "FRAM_SPACE_ID" space-id
           "FRAM_SERVER_PORT" (str port)
           "FRAM_LOG" log
           "NORTH_COORD_SYSTEMD_UNIT" unit}
          valid-redefs
          {source-identity-var
           (fn [actual-source actual-revision actual-tree]
             (when-not (= [source revision tree]
                          [actual-source actual-revision actual-tree])
               (throw (ex-info "wrong source identity" {})))
             {:source source :revision revision :tree tree
              :published revision})
           #'attestation/listener-pids (fn [_] [pid])
           #'attestation/process-birth-token (fn [_] birth)
           #'attestation/process-start-millis (fn [_] 123456789)
           #'attestation/systemd-main-pid! (fn [_] pid)
           #'attestation/process-path
           (fn [_ leaf] (case leaf "cwd" source "exe" artifact nil))
           #'attestation/process-cmdline
           (fn [_] [artifact "serve" (str port) log space-id])
           #'attestation/process-environment (fn [_] environment)}
          request {:port port :served-log log :space-id space-id
                   :record-path record :controller-unit unit}
          verified
          (with-redefs-fn valid-redefs
            #(attestation/attest-active-runtime! request))]
      (check! "canonical record binds exact source, artifact, FRAMLOG, and SpaceId"
              (and (= attestation/attestation-format (:format verified))
                   (= artifact (get-in verified [:identity :artifact :path]))
                   (= space-id (get-in verified [:identity :space-id]))
                   (= pid (get-in verified [:authority :pid]))))
      (check! "unchanged canonical runtime re-attests"
              (true?
               (with-redefs-fn valid-redefs
                 #(attestation/assert-current! verified))))

      (check! "noncanonical process arguments are rejected"
              (= :runtime-process-attestation-failed
                 (with-redefs-fn
                   (assoc valid-redefs #'attestation/process-cmdline
                          (fn [_] [artifact (str port) log space-id]))
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

    (write-record! record (assoc @values "FRAM_ARTIFACT_SHA256"
                                 (apply str (repeat 64 "0"))))
    (check! "artifact digest disagreement is rejected"
            (= :runtime-record-invalid
               (denied-type
                #(attestation/attest-active-runtime!
                  {:port port :served-log log :space-id space-id
                   :record-path record :controller-unit unit}))))

    (spit record
          (str "FORMAT=" attestation/active-runtime-record-format "\n"
               "FRAM_SOURCE=" source "\n"))
    (set-mode! record record-permissions)
    (check! "partial runtime identity is rejected before process trust"
            (= :runtime-record-invalid
               (denied-type
                #(attestation/attest-active-runtime!
                  {:port port :served-log log :space-id space-id
                   :record-path record :controller-unit unit}))))
    (finally
      (delete-tree! temp))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nCanonical runtime attestation: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
