(ns north.runtime-attestation
  "Bind one canonical FRAMRPC listener to its exact frozen Fram source,
  sealed Native READY artifact, FRAMLOG, SpaceId, and systemd owner."
  (:require [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def attestation-format "north-framrpc-runtime-attestation/v1")
(def active-runtime-record-format "north-framrpc-runtime/v1")
(def frozen-fram-source "/home/tom/code/fram/wt-core-target-production-5db9b38")
(def frozen-fram-revision "5db9b38fb05a618d76bd6e386f6a542274d9a8b9")
(def frozen-fram-tree "e3c2bdedf8bd2f1e5c775d16628acc0169785e8f")
(def runtime-record-order
  ["FORMAT" "FRAM_SOURCE" "FRAM_REVISION" "FRAM_TREE"
   "FRAM_NATIVE_ARTIFACT_DIR" "FRAM_NATIVE_CLOSURE_SHA256"
   "FRAM_SERVER_ARTIFACT" "FRAM_SERVER_ARTIFACT_SHA256"
   "FRAM_SPACE_ID" "FRAM_PORT" "FRAM_LOG" "PID" "PID_BIRTH"
   "CONTROLLER_UNIT" "CONTROLLER_MAIN_PID"])
(def runtime-record-keys (set runtime-record-order))
(def proc-read-limit (* 16 1024 1024))
(def record-read-limit (* 1024 1024))

(defn- fail!
  ([message type] (fail! message type {}))
  ([message type data]
   (throw (ex-info message (merge {:type type} data)))))

(defn- sha256-bytes [^bytes payload]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        payload)]
    (apply str (map #(format "%02x" (bit-and (int %) 255)) digest))))

(defn- sha256-file [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")
        buffer (byte-array 65536)]
    (with-open [input (java.io.FileInputStream. (str path))]
      (loop []
        (let [n (.read input buffer)]
          (when (pos? n)
            (.update digest buffer 0 n)
            (recur)))))
    (apply str (map #(format "%02x" (bit-and (int %) 255)) (.digest digest)))))

(defn- no-follow-options []
  (into-array java.nio.file.LinkOption
              [java.nio.file.LinkOption/NOFOLLOW_LINKS]))

(defn- unix-file-state! [label path]
  (let [file (io/file (str path))
        nio (.toPath file)
        options (no-follow-options)]
    (when (or (java.nio.file.Files/isSymbolicLink nio)
              (not (java.nio.file.Files/isRegularFile nio options)))
      (fail! (str label " is missing, linked, or not a regular file: " path)
             :runtime-path-invalid {:label label :path (str path)}))
    (let [attrs (java.nio.file.Files/readAttributes
                 nio "unix:dev,ino,uid,mode,nlink,size,lastModifiedTime,ctime"
                 options)]
      {:dev (long (get attrs "dev"))
       :ino (long (get attrs "ino"))
       :uid (long (get attrs "uid"))
       :mode (bit-and (long (get attrs "mode")) 511)
       :nlink (long (get attrs "nlink"))
       :size (long (get attrs "size"))
       :mtime (str (get attrs "lastModifiedTime"))
       :ctime (str (get attrs "ctime"))})))

(defn- canonical-regular-file! [label path]
  (when (str/blank? (str path))
    (fail! (str label " is blank") :runtime-path-invalid {:label label}))
  (let [state (unix-file-state! label path)
        canonical (.getCanonicalPath (io/file (str path)))]
    {:path canonical :state state}))

(defn- canonical-directory! [label path]
  (let [raw (str path)
        file (io/file raw)
        nio (.toPath file)
        options (no-follow-options)]
    (when (or (str/blank? raw)
              (java.nio.file.Files/isSymbolicLink nio)
              (not (java.nio.file.Files/isDirectory nio options)))
      (fail! (str label " is missing, linked, or not a directory: " path)
             :runtime-path-invalid {:label label :path raw}))
    (let [canonical (.getCanonicalPath file)]
      (when-not (= raw canonical)
        (fail! (str label " is not an exact canonical directory: " path)
               :runtime-path-invalid
               {:label label :path raw :canonical canonical}))
      canonical)))

(defn artifact-record [label path]
  (let [{:keys [path state]} (canonical-regular-file! label path)
        sha256 (sha256-file path)
        after (unix-file-state! label path)]
    (when-not (= state after)
      (fail! (str label " changed while it was hashed")
             :runtime-artifact-raced {:label label :path path}))
    {:path path :bytes (:size after) :sha256 sha256 :state after}))

(defn- ready-record! [path closure-sha256]
  (let [{canonical :path before :state}
        (canonical-regular-file! "Native READY receipt" path)]
    (when-not (and (= 1 (:nlink before))
                   (zero? (bit-and (:mode before) 18)))
      (fail! "Native READY receipt is not a safe regular file"
             :runtime-record-invalid {:path canonical :state before}))
    (when (> (:size before) 256)
      (fail! "Native READY receipt exceeds its exact bound"
             :runtime-record-invalid {:path canonical :bytes (:size before)}))
    (let [payload (java.nio.file.Files/readAllBytes (.toPath (io/file canonical)))
          after (unix-file-state! "Native READY receipt" canonical)
          expected
          (.getBytes (str "fram-native-build/v1 " closure-sha256 "\n")
                     java.nio.charset.StandardCharsets/UTF_8)]
      (when-not (and (= before after)
                     (= (:size before) (alength payload))
                     (java.util.Arrays/equals ^bytes expected ^bytes payload))
        (fail! "Native READY receipt differs from the exact closure"
               :runtime-record-invalid
               {:path canonical :closure-sha256 closure-sha256}))
      {:path canonical :bytes (alength payload)
       :sha256 (sha256-bytes payload)})))

(defn- read-record! [path]
  (let [{canonical :path before :state}
        (canonical-regular-file! "FRAMRPC runtime identity" path)]
    (when-not (and (= 384 (:mode before)) (= 1 (:nlink before)))
      (fail! "FRAMRPC runtime identity must be one 0600 regular file"
             :runtime-record-invalid {:path canonical :state before}))
    (when (> (:size before) record-read-limit)
      (fail! "FRAMRPC runtime identity exceeds its read bound"
             :runtime-record-invalid
             {:path canonical :bytes (:size before) :limit record-read-limit}))
    (let [payload (java.nio.file.Files/readAllBytes (.toPath (io/file canonical)))
          after (unix-file-state! "FRAMRPC runtime identity" canonical)]
      (when-not (and (= before after) (= (:size before) (alength payload)))
        (fail! "FRAMRPC runtime identity changed while it was read"
               :runtime-record-raced {:path canonical}))
      (when (or (zero? (alength payload))
                (not= 10 (bit-and 255 (aget payload (dec (alength payload)))))
                (some #(= 13 (bit-and 255 %)) payload))
        (fail! "FRAMRPC runtime identity must be canonical LF text"
               :runtime-record-invalid {:path canonical}))
      (let [lines (str/split-lines
                   (String. ^bytes payload
                            java.nio.charset.StandardCharsets/UTF_8))
            pairs
            (mapv
             (fn [line]
               (let [index (str/index-of line "=")]
                 (when-not (and index (pos? index))
                   (fail! "FRAMRPC runtime identity has a malformed line"
                          :runtime-record-invalid {:path canonical :line line}))
                 [(subs line 0 index) (subs line (inc index))]))
             lines)
            values
            (reduce
             (fn [result [key value]]
               (when (or (str/blank? value) (contains? result key))
                 (fail! "FRAMRPC runtime identity has a blank or duplicate field"
                        :runtime-record-invalid {:path canonical :key key}))
               (assoc result key value))
             {} pairs)]
        (when-not (and (= runtime-record-order (mapv first pairs))
                       (= runtime-record-keys (set (keys values)))
                       (= active-runtime-record-format (get values "FORMAT")))
          (fail! "FRAMRPC runtime identity has the wrong exact field set"
                 :runtime-record-invalid
                 {:path canonical :fields (mapv first pairs)}))
        {:path canonical :bytes payload :sha256 (sha256-bytes payload)
         :state after :values values}))))

(defn- read-proc-bytes [path]
  (with-open [input (java.io.FileInputStream. (str path))
              output (java.io.ByteArrayOutputStream.)]
    (let [buffer (byte-array 65536)]
      (loop [total 0]
        (let [n (.read input buffer)]
          (cond
            (= -1 n) (.toByteArray output)
            (zero? n) (recur total)
            (> (+ total n) proc-read-limit)
            (fail! "local process evidence exceeds the attestation bound"
                   :listener-process-inspection-failed
                   {:path (str path) :limit proc-read-limit})
            :else (do (.write output buffer 0 n)
                      (recur (+ total n)))))))))

(defn- read-proc-text [path]
  (String. ^bytes (read-proc-bytes path)
           java.nio.charset.StandardCharsets/UTF_8))

(defn tcp-listener-inodes [port]
  (->> ["/proc/net/tcp" "/proc/net/tcp6"]
       (mapcat
        (fn [path]
          (if-not (.isFile (io/file path))
            []
            (keep
             (fn [line]
               (let [fields (str/split (str/trim line) #"\s+")]
                 (when (>= (count fields) 10)
                   (let [local (nth fields 1)
                         state (nth fields 3)
                         colon (.lastIndexOf ^String local ":")
                         local-port
                         (when (pos? colon)
                           (try (Integer/parseInt (subs local (inc colon)) 16)
                                (catch Exception _ nil)))]
                     (when (and (= port local-port) (= "0A" state))
                       (nth fields 9))))))
             (str/split-lines (read-proc-text path))))))
       set))

(defn listener-pids [port]
  (let [targets (set (map #(str "socket:[" % "]")
                          (tcp-listener-inodes port)))
        proc-root (io/file "/proc")]
    (->> (or (.listFiles proc-root) (make-array java.io.File 0))
         (keep
          (fn [pid-directory]
            (when (re-matches #"[0-9]+" (.getName pid-directory))
              (let [fds (io/file pid-directory "fd")
                    owns?
                    (some
                     (fn [fd]
                       (try
                         (contains?
                          targets
                          (str (java.nio.file.Files/readSymbolicLink
                                (.toPath fd))))
                         (catch Exception _ false)))
                     (or (.listFiles fds) (make-array java.io.File 0)))]
                (when owns? (parse-long (.getName pid-directory)))))))
         sort vec)))

(defn process-path [pid leaf]
  (try
    (.getCanonicalPath
     (io/file
      (.toString
       (java.nio.file.Files/readSymbolicLink
        (.toPath (io/file "/proc" (str pid) leaf))))))
    (catch Exception _ nil)))

(defn process-start-millis [pid]
  (let [optional (java.lang.ProcessHandle/of (long pid))]
    (when (.isPresent optional)
      (let [start (.startInstant (.info (.get optional)))]
        (when (.isPresent start) (.toEpochMilli (.get start)))))))

(defn process-birth-token [pid]
  (try
    (let [line (read-proc-text (str "/proc/" pid "/stat"))
          close (.lastIndexOf ^String line ") ")
          fields (when (pos? close)
                   (str/split (subs line (+ close 2)) #"\s+"))
          start-ticks (nth fields 19 nil)]
      (when (and start-ticks (re-matches #"[0-9]+" start-ticks))
        (str "proc:" start-ticks)))
    (catch Exception _ nil)))

(defn process-cmdline [pid]
  (let [payload (read-proc-bytes (str "/proc/" pid "/cmdline"))
        text (String. ^bytes payload java.nio.charset.StandardCharsets/UTF_8)
        arguments (vec (str/split text #"\u0000" -1))]
    (if (and (seq arguments) (= "" (peek arguments)))
      (pop arguments)
      arguments)))

(defn process-environment [pid]
  (let [payload (read-proc-bytes (str "/proc/" pid "/environ"))
        text (String. ^bytes payload java.nio.charset.StandardCharsets/UTF_8)]
    (reduce
     (fn [result entry]
       (let [index (str/index-of entry "=")]
         (if-not (and index (pos? index))
           result
           (let [key (subs entry 0 index) value (subs entry (inc index))]
             (when (contains? result key)
               (fail! "listener process has a duplicate environment field"
                      :listener-process-inspection-failed {:pid pid :key key}))
             (assoc result key value)))))
     {} (remove str/blank? (str/split text #"\u0000")))))

(defn- git-value! [source expression]
  (let [result (proc/shell {:out :string :err :string :continue true}
                           "git" "-C" source "rev-parse" "--verify" expression)
        value (str/trim (:out result))]
    (when-not (and (zero? (:exit result))
                   (re-matches #"[0-9a-f]{40,64}" value))
      (fail! "frozen Fram source lacks exact Git provenance"
             :runtime-source-invalid
             {:source source :expression expression :error (str/trim (:err result))}))
    value))

(defn- source-identity! [source expected-revision expected-tree]
  (let [canonical (.getCanonicalPath (io/file source))
        marker (.toPath (io/file canonical ".git"))]
    (when (or (not (.isDirectory (io/file canonical)))
              (java.nio.file.Files/isSymbolicLink marker)
              (not (java.nio.file.Files/exists marker (no-follow-options))))
      (fail! "frozen Fram source is not an exact Git checkout"
             :runtime-source-invalid {:source canonical}))
    (let [revision (git-value! canonical "HEAD")
          tree (git-value! canonical "HEAD^{tree}")
          dirty (proc/shell {:out :string :err :string :continue true}
                            "git" "-C" canonical "status" "--porcelain"
                            "--untracked-files=all")]
      (when-not (and (= expected-revision revision)
                     (= expected-tree tree)
                     (zero? (:exit dirty))
                     (str/blank? (:out dirty)))
        (fail! "listener source is not the exact clean frozen Fram"
               :runtime-source-mismatch
               {:source canonical
                :expected {:revision expected-revision :tree expected-tree}
                :actual {:revision revision :tree tree}
                :changes (str/split-lines (:out dirty))}))
      {:source canonical :revision revision :tree tree})))

(defn- parse-positive-long! [label value]
  (let [parsed (parse-long (str value))]
    (when-not (and parsed (pos? parsed))
      (fail! (str label " is not a positive integer")
             :runtime-record-invalid {:label label :value value}))
    parsed))

(defn- systemd-properties [unit]
  (let [result
        (proc/shell {:out :string :err :string :continue true}
                    "systemctl" "show" unit "--no-pager"
                    "--property" "Id" "--property" "LoadState"
                    "--property" "ActiveState" "--property" "SubState"
                    "--property" "MainPID")]
    {:exit (:exit result)
     :error (str/trim (:err result))
     :values
     (into {}
           (keep (fn [line]
                   (when-let [index (str/index-of line "=")]
                     [(subs line 0 index) (subs line (inc index))])))
           (str/split-lines (:out result)))}))

(defn systemd-main-pid! [unit]
  (when-not (re-matches #"[A-Za-z0-9@_.:-]+" (str unit))
    (fail! "FRAMRPC controller unit is unsafe"
           :runtime-controller-invalid {:unit unit}))
  (let [{:keys [exit error values]} (systemd-properties unit)
        pid (parse-long (get values "MainPID"))]
    (when-not (and (zero? exit) (= unit (get values "Id"))
                   (= "loaded" (get values "LoadState"))
                   (= "active" (get values "ActiveState"))
                   (= "running" (get values "SubState"))
                   pid (pos? pid))
      (fail! "FRAMRPC controller is not one loaded running systemd unit"
             :runtime-controller-invalid
             {:unit unit :properties values :error error}))
    pid))

(defn- exact-process-shape!
  [pid source artifact]
  (let [cwd (process-path pid "cwd")
        executable (process-path pid "exe")
        arguments (process-cmdline pid)
        expected [artifact]]
    (when-not (and (= source cwd) (= artifact executable)
                   (= expected arguments))
      (fail! "FRAMRPC listener is not the sealed Native executable"
             :runtime-process-attestation-failed
             {:reason :process-shape-mismatch :pid pid
              :expected {:cwd source :executable artifact :arguments expected}
              :actual {:cwd cwd :executable executable :arguments arguments}}))
    {:cwd cwd :arguments-sha256
     (sha256-bytes
      (.getBytes (pr-str arguments) java.nio.charset.StandardCharsets/UTF_8))}))

(defn- runtime-identity-environment [environment]
  (into {}
        (filter (fn [[key _]]
                  (or (str/starts-with? key "FRAM_")
                      (str/starts-with? key "NORTH_"))))
        environment))

(defn attest-active-runtime!
  "Attest one active canonical writer from its launcher-owned 0600 record."
  [{:keys [port served-log space-id record-path controller-unit]}]
  (when-not (and port served-log space-id record-path controller-unit)
    (fail! "FRAMRPC runtime attestation requires port, log, SpaceId, record, and unit"
           :runtime-attestation-request-invalid))
  (let [sealed (read-record! record-path)
        values (:values sealed)
        record-port (parse-positive-long! "FRAM_PORT" (get values "FRAM_PORT"))
        pid (parse-positive-long! "PID" (get values "PID"))
        main-pid (parse-positive-long! "CONTROLLER_MAIN_PID"
                                       (get values "CONTROLLER_MAIN_PID"))
        source-field (get values "FRAM_SOURCE")
        source (.getCanonicalPath (io/file source-field))
        closure-sha256 (get values "FRAM_NATIVE_CLOSURE_SHA256")
        _ (when-not (re-matches #"[0-9a-f]{64}" closure-sha256)
            (fail! "Native closure SHA-256 is not canonical"
                   :runtime-record-invalid {:sha256 closure-sha256}))
        artifact-directory
        (canonical-directory! "Native READY artifact directory"
                              (get values "FRAM_NATIVE_ARTIFACT_DIR"))
        ready-path (.getPath (io/file artifact-directory "READY"))
        manifest-path (.getPath (io/file artifact-directory "input.manifest"))
        expected-server-path
        (.getPath (io/file artifact-directory "bin" "fram-server-native"))
        ready (ready-record! ready-path closure-sha256)
        input-manifest (artifact-record "Native input manifest" manifest-path)
        server-artifact
        (artifact-record "sealed Native Fram server" expected-server-path)
        log (.getCanonicalPath (io/file (get values "FRAM_LOG")))
        requested-log (.getCanonicalPath (io/file served-log))]
    (when-not (and (= frozen-fram-source source-field source)
                   (= frozen-fram-revision (get values "FRAM_REVISION"))
                   (= frozen-fram-tree (get values "FRAM_TREE"))
                   (= artifact-directory
                      (get values "FRAM_NATIVE_ARTIFACT_DIR"))
                   (= expected-server-path
                      (get values "FRAM_SERVER_ARTIFACT")
                      (:path server-artifact))
                   (= closure-sha256 (:sha256 input-manifest))
                   (= (long port) record-port)
                   (= requested-log log)
                   (= space-id (get values "FRAM_SPACE_ID"))
                   (= controller-unit (get values "CONTROLLER_UNIT"))
                   (= pid main-pid)
                   (= (:sha256 server-artifact)
                      (get values "FRAM_SERVER_ARTIFACT_SHA256"))
                   (.canExecute (io/file expected-server-path)))
      (fail! "FRAMRPC runtime record does not bind the requested canonical writer"
             :runtime-record-invalid
             {:request {:port port :log requested-log :space-id space-id
                        :controller-unit controller-unit}
              :record (dissoc values "PID_BIRTH")}))
    (let [source-identity
          (source-identity! source frozen-fram-revision frozen-fram-tree)
          listener-owners (listener-pids record-port)
          birth (process-birth-token pid)
          start (process-start-millis pid)
          controller-pid (systemd-main-pid! controller-unit)
          shape (exact-process-shape! pid source expected-server-path)
          environment (process-environment pid)
          expected-environment
          {"FRAM_HOME" source
           "FRAM_SERVER_RUNTIME" "native"
           "FRAM_NATIVE_ARTIFACT_DIR" artifact-directory
           "FRAM_SPACE_ID" space-id
           "FRAM_SERVER_PORT" (str record-port)
           "FRAM_LOG" log
           "NORTH_COORD_SYSTEMD_UNIT" controller-unit}
          actual-environment (runtime-identity-environment environment)]
      (when-not (and (= [pid] listener-owners)
                     (= (get values "PID_BIRTH") birth)
                     (integer? start)
                     (= pid controller-pid)
                     (= expected-environment actual-environment))
        (fail! "FRAMRPC runtime record, listener, environment, and systemd owner disagree"
               :runtime-process-attestation-failed
               {:pid pid :listener-pids listener-owners
                :expected-birth (get values "PID_BIRTH") :actual-birth birth
                :process-start-millis start :controller-pid controller-pid
                :environment-keys (sort (keys actual-environment))}))
      {:format attestation-format
       :request {:port record-port :served-log log :space-id space-id
                 :record-path (:path sealed) :controller-unit controller-unit}
       :identity (merge source-identity
                        {:native-artifact
                         {:directory artifact-directory
                          :closure-sha256 closure-sha256
                          :ready ready
                          :input-manifest (dissoc input-manifest :state)
                          :server (dissoc server-artifact :state)}
                         :space-id space-id :port record-port :served-log log
                         :arguments-sha256 (:arguments-sha256 shape)})
       :authority {:pid pid :pid-birth birth :process-start-millis start
                   :controller {:kind "systemd" :unit controller-unit
                                :main-pid controller-pid}
                   :record {:path (:path sealed)
                            :bytes (alength ^bytes (:bytes sealed))
                            :sha256 (:sha256 sealed) :state (:state sealed)}}})))

(defn assert-current!
  "Re-attest and require the same exact process and sealed runtime identity."
  [attestation]
  (let [current
        (try
          (attest-active-runtime! (:request attestation))
          (catch Throwable error
            (fail! "selected frozen Fram authority changed"
                   :runtime-authority-lost
                   {:cause (.getMessage error) :cause-data (ex-data error)})))]
    (when-not (= attestation current)
      (fail! "selected frozen Fram authority changed"
             :runtime-authority-lost
             {:expected attestation :actual current}))
    true))

(defn -main [& [port served-log space-id record-path controller-unit]]
  (prn
   (attest-active-runtime!
    {:port (parse-positive-long! "port" port)
     :served-log served-log
     :space-id space-id
     :record-path record-path
     :controller-unit controller-unit})))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
