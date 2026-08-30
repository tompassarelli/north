(ns north.runtime-attestation
  "Bind one canonical Store RPC listener to the selected typed JVM/Native
  generation, Store log, SpaceId, process image, and systemd owner."
  (:require [babashka.process :as proc]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [north.store-runtime-manifest :as manifest]))

(def attestation-format "north-store-runtime-attestation/v1")
(def active-runtime-record-format "north-store-runtime/v1")
(def managed-native-runtime-record-format "north-store-native-runtime/v2")
(def jvm-runtime-record-format "north-store-jvm-runtime/v1")
(def release-receipt-format "north-store-release/v1")
(def release-receipt-name "RELEASE")
(def release-receipt-read-limit 4096)
;; The receipt's own field order, which is what a sealed release publishes.
(def release-receipt-order
  ["format" "source" "revision" "tree" "native_artifact_dir"
   "native_closure_sha256" "server_artifact_sha256" "created"])
(def runtime-record-order
  ["FORMAT" "BEAGLE_STORE_SOURCE" "BEAGLE_STORE_REVISION" "BEAGLE_STORE_TREE"
   "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" "BEAGLE_STORE_NATIVE_CLOSURE_SHA256"
   "BEAGLE_STORE_SERVER_ARTIFACT" "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"
   "BEAGLE_STORE_SPACE_ID" "BEAGLE_STORE_PORT" "BEAGLE_STORE_LOG" "PID" "PID_BIRTH"
   "CONTROLLER_UNIT" "CONTROLLER_MAIN_PID"])
(def managed-native-runtime-record-order
  ["FORMAT" "GENERATION_TARGET" "GENERATION_SHA256" "RUNTIME_KIND"
   "BEAGLE_STORE_SOURCE" "BEAGLE_STORE_REVISION" "BEAGLE_STORE_TREE"
   "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" "BEAGLE_STORE_NATIVE_CLOSURE_SHA256"
   "BEAGLE_STORE_SERVER_ARTIFACT" "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"
   "BEAGLE_STORE_SPACE_ID" "BEAGLE_STORE_PORT" "BEAGLE_STORE_LOG" "PID" "PID_BIRTH"
   "CONTROLLER_UNIT" "CONTROLLER_MAIN_PID"])
(def jvm-runtime-record-order
  ["FORMAT" "GENERATION_TARGET" "GENERATION_SHA256" "RUNTIME_KIND"
   "BEAGLE_STORE_OUTPUT" "BEAGLE_STORE_REVISION" "BEAGLE_STORE_TREE"
   "BEAGLE_STORE_PACKAGE_NAR_SHA256" "BEAGLE_STORE_MANIFEST"
   "BEAGLE_STORE_MANIFEST_BYTES" "BEAGLE_STORE_MANIFEST_SHA256"
   "BEAGLE_STORE_JAVA" "BEAGLE_STORE_SERVER_CLASSPATH_FILE"
   "BEAGLE_STORE_SERVER_CLASSPATH_SHA256" "BEAGLE_STORE_SPACE_ID"
   "BEAGLE_STORE_PORT" "BEAGLE_STORE_LOG" "PID" "PID_BIRTH"
   "CONTROLLER_UNIT" "CONTROLLER_MAIN_PID"])
(def proc-read-limit (* 16 1024 1024))
(def record-read-limit (* 1024 1024))
(def wrapper-read-limit (* 1024 1024))

(def ^:dynamic *store-runtime-state-root* nil)
(def ^:dynamic *allow-controller-starting?* false)

(def ^:private generation-readers
  {'north.store_runtime_manifest.StoreRuntimeManifest
   manifest/map->StoreRuntimeManifest
   'north.store_runtime_manifest.JVM manifest/map->JVM
   'north.store_runtime_manifest.Native manifest/map->Native
   'north.store_runtime_manifest.StoreRuntimeGeneration
   manifest/map->StoreRuntimeGeneration})

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
          (.getBytes (str "beagle-store-native-build/v1 " closure-sha256 "\n")
                     java.nio.charset.StandardCharsets/UTF_8)]
      (when-not (and (= before after)
                     (= (:size before) (alength payload))
                     (java.util.Arrays/equals ^bytes expected ^bytes payload))
        (fail! "Native READY receipt differs from the exact closure"
               :runtime-record-invalid
               {:path canonical :closure-sha256 closure-sha256}))
      {:path canonical :bytes (alength payload)
       :sha256 (sha256-bytes payload)})))

(defn runtime-record-order-for! [format]
  (case format
    "north-store-runtime/v1" runtime-record-order
    "north-store-native-runtime/v2" managed-native-runtime-record-order
    "north-store-jvm-runtime/v1" jvm-runtime-record-order
    (fail! "Store RPC runtime identity has an unsupported format"
           :runtime-record-invalid {:format format})))

(defn- read-record! [path]
  (let [{canonical :path before :state}
        (canonical-regular-file! "Store RPC runtime identity" path)]
    (when-not (and (= 384 (:mode before)) (= 1 (:nlink before)))
      (fail! "Store RPC runtime identity must be one 0600 regular file"
             :runtime-record-invalid {:path canonical :state before}))
    (when (> (:size before) record-read-limit)
      (fail! "Store RPC runtime identity exceeds its read bound"
             :runtime-record-invalid
             {:path canonical :bytes (:size before) :limit record-read-limit}))
    (let [payload (java.nio.file.Files/readAllBytes (.toPath (io/file canonical)))
          after (unix-file-state! "Store RPC runtime identity" canonical)]
      (when-not (and (= before after) (= (:size before) (alength payload)))
        (fail! "Store RPC runtime identity changed while it was read"
               :runtime-record-raced {:path canonical}))
      (when (or (zero? (alength payload))
                (not= 10 (bit-and 255 (aget payload (dec (alength payload)))))
                (some #(= 13 (bit-and 255 %)) payload))
        (fail! "Store RPC runtime identity must be canonical LF text"
               :runtime-record-invalid {:path canonical}))
      (let [lines (str/split-lines
                   (String. ^bytes payload
                            java.nio.charset.StandardCharsets/UTF_8))
            pairs
            (mapv
             (fn [line]
               (let [index (str/index-of line "=")]
                 (when-not (and index (pos? index))
                   (fail! "Store RPC runtime identity has a malformed line"
                          :runtime-record-invalid {:path canonical :line line}))
                 [(subs line 0 index) (subs line (inc index))]))
             lines)
            values
            (reduce
             (fn [result [key value]]
               (when (or (str/blank? value) (contains? result key))
                 (fail! "Store RPC runtime identity has a blank or duplicate field"
                        :runtime-record-invalid {:path canonical :key key}))
               (assoc result key value))
             {} pairs)]
        (let [order (runtime-record-order-for! (get values "FORMAT"))]
          (when-not (and (= order (mapv first pairs))
                         (= (set order) (set (keys values))))
          (fail! "Store RPC runtime identity has the wrong exact field set"
                 :runtime-record-invalid
                 {:path canonical :fields (mapv first pairs)})))
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

(defn runtime-state-root []
  (or *store-runtime-state-root*
      (System/getenv "NORTH_STORE_RUNTIME_STATE")
      manifest/canonical-store-runtime-root))

(defn default-runtime-record-path []
  (.getCanonicalPath
   (io/file (or (System/getenv "XDG_STATE_HOME")
                (str (System/getProperty "user.home") "/.local/state"))
            "north" "store-runtime" "north-store.runtime")))

(defn read-selection!
  "Read only the simple export assignments in the existing North Store selection.
   Member identity is ignored; the selected typed generation owns that choice."
  [path]
  (let [{canonical :path before :state}
        (canonical-regular-file! "North Store runtime selection" path)]
    (when (> (:size before) record-read-limit)
      (fail! "North Store runtime selection exceeds its read bound"
             :runtime-selection-invalid {:path canonical :bytes (:size before)}))
    (let [text (slurp canonical)
          after (unix-file-state! "North Store runtime selection" canonical)]
      (when-not (= before after)
        (fail! "North Store runtime selection changed while it was read"
               :runtime-selection-raced {:path canonical}))
      (reduce
       (fn [values line]
         (cond
           (or (str/blank? line) (str/starts-with? line "#")) values
           :else
           (let [[_ key value]
                 (re-matches #"export ([A-Z][A-Z0-9_]*)='([^']*)'" line)]
             (when-not key
               (fail! "North Store runtime selection has a noncanonical line"
                      :runtime-selection-invalid {:path canonical :line line}))
             (when (or (str/blank? value) (contains? values key))
               (fail! "North Store runtime selection has a blank or duplicate field"
                      :runtime-selection-invalid {:path canonical :key key}))
             (assoc values key value))))
       {} (str/split-lines text)))))

(defn- common-runtime-selection! [selection]
  (let [space-id (get selection "BEAGLE_STORE_SPACE_ID")
        port-text (get selection "BEAGLE_STORE_SERVER_PORT")
        port (parse-long (str port-text))
        log (get selection "BEAGLE_STORE_LOG")]
    (when-not (and (not (str/blank? space-id))
                   port (pos? port)
                   (not (str/blank? log))
                   (.isAbsolute (io/file log)))
      (fail! "North Store runtime selection lacks canonical SpaceId, port, or log"
             :runtime-selection-invalid
             {:space-id space-id :port port-text :log log}))
    {:space-id space-id
     :port port
     :log (.getCanonicalPath (io/file log))}))

(defn- jvm-wrapper-values! [output]
  (let [wrapper (manifest/jvm-dispatcher-path-for output)
        {canonical :path before :state}
        (canonical-regular-file! "Store JVM dispatcher" wrapper)]
    (when (> (:size before) wrapper-read-limit)
      (fail! "Store JVM dispatcher exceeds its read bound"
             :runtime-package-invalid {:path canonical :bytes (:size before)}))
    (let [text (slurp canonical)
          after (unix-file-state! "Store JVM dispatcher" canonical)
          values
          (reduce
           (fn [result line]
             (if-let [[_ key value]
                      (re-matches #"export ([A-Z][A-Z0-9_]*)='([^']*)'" line)]
               (if (contains? #{"BEAGLE_STORE_HOME" "BEAGLE_STORE_BIN"
                                "BEAGLE_STORE_OUT"
                                "BEAGLE_STORE_SERVER_CLASSPATH_FILE"
                                "BEAGLE_STORE_PACKAGED"
                                "BEAGLE_STORE_SERVER_RUNTIME"
                                "BEAGLE_STORE_JAVA"}
                              key)
                 (do
                   (when (contains? result key)
                     (fail! "Store JVM dispatcher repeats a runtime binding"
                            :runtime-package-invalid {:path canonical :key key}))
                   (assoc result key value))
                 result)
               result))
           {} (str/split-lines text))
          expected
          {"BEAGLE_STORE_HOME" (manifest/jvm-store-home-for output)
           "BEAGLE_STORE_BIN" (manifest/jvm-store-bin-for output)
           "BEAGLE_STORE_OUT" (manifest/jvm-store-out-for output)
           "BEAGLE_STORE_SERVER_CLASSPATH_FILE"
           (manifest/jvm-server-classpath-file-for output)
           "BEAGLE_STORE_PACKAGED" "1"
           "BEAGLE_STORE_SERVER_RUNTIME" "jvm"}
          java (get values "BEAGLE_STORE_JAVA")]
      (when-not (= before after)
        (fail! "Store JVM dispatcher changed while it was read"
               :runtime-package-raced {:path canonical}))
      (when-not (and (= expected (dissoc values "BEAGLE_STORE_JAVA"))
                     (re-matches
                      #"/nix/store/[0-9abcdfghijklmnpqrsvwxyz]{32}-[^/]+/bin/java"
                      (str java))
                     (.canExecute (io/file java)))
        (fail! "Store JVM dispatcher lacks the exact package runtime bindings"
               :runtime-package-invalid
               {:path canonical :expected expected :actual values}))
      values)))

(defn jvm-runtime-facts! [member]
  (let [checked (manifest/validate-runtime-member! member)
        output (:output checked)
        wrapper-values (jvm-wrapper-values! output)
        classpath-file (get wrapper-values "BEAGLE_STORE_SERVER_CLASSPATH_FILE")
        classpath-record (artifact-record "Store JVM server classpath" classpath-file)
        classpath (str/trim-newline (slurp (:path classpath-record)))
        launcher (manifest/jvm-server-launcher-for output)]
    (when-not (and (= "jvm" (manifest/runtime-member-kind checked))
                   (not (str/blank? classpath))
                   (.canExecute (io/file launcher)))
      (fail! "Store JVM package lacks its exact server launcher or classpath"
             :runtime-package-invalid {:output output}))
    {:output output
     :home (get wrapper-values "BEAGLE_STORE_HOME")
     :bin (get wrapper-values "BEAGLE_STORE_BIN")
     :out (get wrapper-values "BEAGLE_STORE_OUT")
     :launcher launcher
     :java (get wrapper-values "BEAGLE_STORE_JAVA")
     :java-executable (.getCanonicalPath
                       (io/file (get wrapper-values "BEAGLE_STORE_JAVA")))
     :classpath-file classpath-file
     :classpath classpath
     :classpath-sha256 (:sha256 classpath-record)}))

(defn launch-spec! [member selection]
  (let [checked (manifest/validate-runtime-member! member)
        {:keys [space-id port log]} (common-runtime-selection! selection)
        common {"BEAGLE_STORE_SPACE_ID" space-id
                "BEAGLE_STORE_SERVER_PORT" (str port)
                "BEAGLE_STORE_LOG" log
                "BEAGLE_STORE_SERVER_LOG" log}
        inherited (select-keys selection
                               ["BEAGLE_STORE_MAX_ACTIVE_CLIENTS"
                                "BEAGLE_STORE_CLIENT_IO_TIMEOUT_MS"])]
    (case (manifest/runtime-member-kind checked)
      "native"
      (let [launcher (str (:release-root checked) "/bin/beagle-store-server")]
        (when-not (.canExecute (io/file launcher))
          (fail! "Store Native release lacks its server launcher"
                 :runtime-package-invalid {:path launcher}))
        {:executable launcher
         :arguments [(str port)]
         :environment
         (merge inherited common
                {"BEAGLE_STORE_HOME" (:release-root checked)
                 "BEAGLE_STORE_BIN" (str (:release-root checked) "/bin")
                 "BEAGLE_STORE_OUT" (str (:release-root checked) "/out")
                 "BEAGLE_STORE_SERVER_RUNTIME" "native"
                 "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" (:artifact-root checked)
                 "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" (:closure-sha256 checked)
                 "BEAGLE_STORE_SERVER_ARTIFACT" (:server-artifact checked)
                 "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" (:server-sha256 checked)})})

      "jvm"
      (let [facts (jvm-runtime-facts! checked)]
        {:executable (:launcher facts)
         :arguments [(str port)]
         :environment
         (merge inherited common
                {"BEAGLE_STORE_HOME" (:home facts)
                 "BEAGLE_STORE_BIN" (:bin facts)
                 "BEAGLE_STORE_OUT" (:out facts)
                 "BEAGLE_STORE_SERVER_RUNTIME" "jvm"
                 "BEAGLE_STORE_PACKAGED" "1"
                 "BEAGLE_STORE_JAVA" (:java facts)
                 "BEAGLE_STORE_SERVER_CLASSPATH_FILE" (:classpath-file facts)})}))))

(defn active-generation-evidence! [state-root]
  (let [root (.toAbsolutePath (.normalize (.toPath (io/file state-root))))
        generations (.resolve root "generations")
        selector (.resolve root "active")]
    (when-not (java.nio.file.Files/isSymbolicLink selector)
      (fail! "Store runtime generation selector is absent or not a symbolic link"
             :runtime-generation-unselected {:selector (str selector)}))
    (let [target (java.nio.file.Files/readSymbolicLink selector)
          generation-root (.normalize (.resolve root target))
          record (.resolve generation-root "generation.edn")]
      (when-not (and (not (.isAbsolute target))
                     (.startsWith generation-root generations)
                     (= (.getParent generation-root) generations)
                     (java.nio.file.Files/isRegularFile record (no-follow-options)))
        (fail! "Store runtime generation selector escaped its exact record"
               :runtime-generation-invalid {:target (str target)}))
      (let [before (unix-file-state! "Store runtime generation record" record)
            bytes (java.nio.file.Files/readAllBytes record)
            after (unix-file-state! "Store runtime generation record" record)
            generation
            (try
              (manifest/validate-runtime-generation!
               (edn/read-string
                {:readers generation-readers}
                (String. bytes java.nio.charset.StandardCharsets/UTF_8)))
              (catch Throwable error
                (fail! "Store runtime generation record is not a valid typed generation"
                       :runtime-generation-invalid
                       {:path (str record) :cause (.getMessage error)})))]
        (when-not (= before after)
          (fail! "Store runtime generation record changed while it was read"
                 :runtime-generation-raced {:path (str record)}))
        {:state-root (str root)
         :target (str target)
         :root (str generation-root)
         :record (str record)
         :sha256 (sha256-bytes bytes)
         :generation generation}))))

(defn- validate-generation-evidence! [values]
  (let [current (active-generation-evidence! (runtime-state-root))]
    (when-not (and (= (:target current) (get values "GENERATION_TARGET"))
                   (= (:sha256 current) (get values "GENERATION_SHA256")))
      (fail! "Store runtime record does not name the selected generation"
             :runtime-generation-mismatch
             {:expected current
              :actual {:target (get values "GENERATION_TARGET")
                       :sha256 (get values "GENERATION_SHA256")}}))
    current))

(defn sealed-release-home
  "The sealed Beagle Store release the live selection names. A pointer names an
   immutable snapshot: nothing here may name a checkout or a worktree."
  []
  (let [home (System/getenv "BEAGLE_STORE_HOME")]
    (when (str/blank? (str home))
      (fail! "BEAGLE_STORE_HOME must name the sealed Beagle Store release"
             :runtime-release-unselected))
    home))

(defn sealed-release-identity!
  "Read the sealed release's own receipt. The receipt is the immutable record
   of which Beagle Store revision and tree were sealed into this release, so expected
   identity is derived from the selected snapshot instead of a literal any
   later engine generation would silently invalidate. `source` names the
   checkout the release was CUT FROM; it is provenance only and is never
   resolved — that checkout is mutable and may not exist."
  [home]
  (let [directory (canonical-directory! "sealed Beagle Store release" home)
        receipt-path (.getPath (io/file directory release-receipt-name))
        {canonical :path before :state}
        (canonical-regular-file! "sealed Beagle Store release receipt" receipt-path)]
    (when (> (:size before) release-receipt-read-limit)
      (fail! "sealed Beagle Store release receipt exceeds its read bound"
             :runtime-release-invalid
             {:path canonical :bytes (:size before)
              :limit release-receipt-read-limit}))
    (let [payload (java.nio.file.Files/readAllBytes (.toPath (io/file canonical)))
          after (unix-file-state! "sealed Beagle Store release receipt" canonical)]
      (when-not (and (= before after) (= (:size before) (alength payload)))
        (fail! "sealed Beagle Store release receipt changed while it was read"
               :runtime-release-raced {:path canonical}))
      (when (or (zero? (alength payload))
                (not= 10 (bit-and 255 (aget payload (dec (alength payload)))))
                (some #(= 13 (bit-and 255 %)) payload))
        (fail! "sealed Beagle Store release receipt must be canonical LF text"
               :runtime-release-invalid {:path canonical}))
      (let [pairs
            (mapv
             (fn [line]
               (let [index (str/index-of line "=")]
                 (when-not (and index (pos? index))
                   (fail! "sealed Beagle Store release receipt has a malformed line"
                          :runtime-release-invalid {:path canonical :line line}))
                 [(subs line 0 index) (subs line (inc index))]))
             (str/split-lines
              (String. ^bytes payload
                       java.nio.charset.StandardCharsets/UTF_8)))
            values (into {} pairs)]
        (when-not (and (= release-receipt-order (mapv first pairs))
                       (= (count release-receipt-order) (count values))
                       (not-any? str/blank? (vals values))
                       (= release-receipt-format (get values "format")))
          (fail! "sealed Beagle Store release receipt has the wrong exact field set"
                 :runtime-release-invalid
                 {:path canonical :fields (mapv first pairs)}))
        (let [revision (get values "revision")
              tree (get values "tree")]
          (when-not (and (re-matches #"[0-9a-f]{40,64}" revision)
                         (re-matches #"[0-9a-f]{40,64}" tree)
                         (re-matches #"[0-9a-f]{64}"
                                     (get values "native_closure_sha256"))
                         (re-matches #"[0-9a-f]{64}"
                                     (get values "server_artifact_sha256"))
                         (str/starts-with? (get values "native_artifact_dir") "/")
                         (str/starts-with? (get values "source") "/"))
            (fail! "sealed Beagle Store release receipt lacks exact identity"
                   :runtime-release-invalid {:path canonical}))
          {:source directory
           :revision revision
           :tree tree
           :cut-from (get values "source")
           :native-artifact-dir (get values "native_artifact_dir")
           :native-closure-sha256 (get values "native_closure_sha256")
           :server-artifact-sha256 (get values "server_artifact_sha256")
           :receipt {:path canonical :bytes (alength payload)
                     :sha256 (sha256-bytes payload)}})))))

(defn- parse-positive-long! [label value]
  (let [parsed (parse-long (str value))]
    (when-not (and parsed (pos? parsed))
      (fail! (str label " is not a positive integer")
             :runtime-record-invalid {:label label :value value}))
    parsed))

;; The controller is a user unit: the coordination engine runs in the login
;; session's manager, so the system manager cannot see it at all and answers
;; LoadState=not-found for the unit that is in fact running.
(defn- systemd-properties [unit]
  (let [result
        (proc/shell {:out :string :err :string :continue true}
                    "systemctl" "--user" "show" unit "--no-pager"
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
    (fail! "Store RPC controller unit is unsafe"
           :runtime-controller-invalid {:unit unit}))
  (let [{:keys [exit error values]} (systemd-properties unit)
        pid (parse-long (get values "MainPID"))]
    (when-not
     (and (zero? exit) (= unit (get values "Id"))
          (= "loaded" (get values "LoadState"))
          (or (and (= "active" (get values "ActiveState"))
                   (= "running" (get values "SubState")))
              (and *allow-controller-starting?*
                   (= "activating" (get values "ActiveState"))
                   (contains? #{"start" "start-post"}
                              (get values "SubState"))))
          pid (pos? pid))
      (fail! "Store RPC controller is not one loaded running systemd unit"
             :runtime-controller-invalid
             {:unit unit :properties values :error error}))
    pid))

(defn- exact-process-shape!
  [pid expected]
  (let [actual {:cwd (process-path pid "cwd")
                :executable (process-path pid "exe")
                :arguments (process-cmdline pid)}]
    (when-not (= expected actual)
      (fail! "Store RPC listener process shape differs from the selected runtime"
             :runtime-process-attestation-failed
             {:reason :process-shape-mismatch :pid pid
              :expected expected :actual actual}))
    {:cwd (:cwd actual)
     :arguments-sha256
     (sha256-bytes
      (.getBytes (pr-str (:arguments actual))
                 java.nio.charset.StandardCharsets/UTF_8))}))

(defn- runtime-identity-environment [environment]
  (into {}
        (filter (fn [[key _]]
                  (or (str/starts-with? key "BEAGLE_STORE_")
                      (str/starts-with? key "NORTH_"))))
        environment))

;; A unit that loads the selection file carries more BEAGLE_STORE_/NORTH_ variables
;; than the seven that name identity, so exact-set equality would reject the
;; supported launch shape. What must never disagree is any variable that can
;; select a different engine, artifact, Beagle Store log, port, or SpaceId: required
;; names must be present and exact, constrained names must be exact when
;; present, and the names that would route the launcher away from the sealed
;; Native artifact must be absent.
(defn- sealed-environment-expectation
  [{:keys [source native-artifact-dir native-closure-sha256
           server-artifact-sha256]}
   {:keys [space-id port log controller-unit server-artifact]}]
  {:required
   {"BEAGLE_STORE_HOME" source
    "BEAGLE_STORE_SERVER_RUNTIME" "native"
    "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" native-artifact-dir
    "BEAGLE_STORE_SPACE_ID" space-id
    "BEAGLE_STORE_SERVER_PORT" (str port)
    "BEAGLE_STORE_LOG" log
    "NORTH_COORD_SYSTEMD_UNIT" controller-unit}
   :constrained
   {"BEAGLE_STORE_BIN" (.getPath (io/file source "bin"))
    "BEAGLE_STORE_OUT" (.getPath (io/file source "out"))
    "BEAGLE_STORE_SERVER_ARTIFACT" server-artifact
    "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" server-artifact-sha256
    "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" native-closure-sha256
    "NORTH_PORT" (str port)}
   :forbidden
   #{"BEAGLE_STORE_LISTEN_FD" "BEAGLE_STORE_PACKAGED"
     "BEAGLE_STORE_JAVA" "BEAGLE_STORE_SERVER_CLASSPATH_FILE"}})

(defn- jvm-environment-expectation
  [facts {:keys [space-id port log controller-unit]}]
  {:required
   {"BEAGLE_STORE_HOME" (:home facts)
    "BEAGLE_STORE_BIN" (:bin facts)
    "BEAGLE_STORE_OUT" (:out facts)
    "BEAGLE_STORE_SERVER_RUNTIME" "jvm"
    "BEAGLE_STORE_PACKAGED" "1"
    "BEAGLE_STORE_JAVA" (:java facts)
    "BEAGLE_STORE_SERVER_CLASSPATH_FILE" (:classpath-file facts)
    "BEAGLE_STORE_SPACE_ID" space-id
    "BEAGLE_STORE_SERVER_PORT" (str port)
    "BEAGLE_STORE_LOG" log
    "BEAGLE_STORE_SERVER_LOG" log
    "NORTH_COORD_SYSTEMD_UNIT" controller-unit}
   :constrained {"NORTH_PORT" (str port)}
   :forbidden
   #{"BEAGLE_STORE_LISTEN_FD" "BEAGLE_STORE_NATIVE_ARTIFACT_DIR"
     "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" "BEAGLE_STORE_SERVER_ARTIFACT"
     "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" "BEAGLE_STORE_SERVER_G1_REGION"
     "BEAGLE_STORE_SERVER_NO_OOM_EXIT" "JAVA_TOOL_OPTIONS" "_JAVA_OPTIONS"
     "JDK_JAVA_OPTIONS" "CLASSPATH"}})

(defn- environment-disagreements [expectation environment]
  (let [{:keys [required constrained forbidden]} expectation]
    (vec
     (sort
      (concat
       (keep (fn [[key value]]
               (when-not (= value (get environment key)) key))
             required)
       (keep (fn [[key value]]
               (when (and (contains? environment key)
                          (not= value (get environment key)))
                 key))
             constrained)
       (filter #(contains? environment %) forbidden))))))

(defn- common-runtime-authority!
  [{:keys [port served-log space-id controller-unit]} sealed values shape
   expectation]
  (let [record-port (parse-positive-long! "BEAGLE_STORE_PORT"
                                          (get values "BEAGLE_STORE_PORT"))
        pid (parse-positive-long! "PID" (get values "PID"))
        main-pid (parse-positive-long! "CONTROLLER_MAIN_PID"
                                       (get values "CONTROLLER_MAIN_PID"))
        log (.getCanonicalPath (io/file (get values "BEAGLE_STORE_LOG")))
        requested-log (.getCanonicalPath (io/file served-log))
        listener-owners (listener-pids record-port)
        birth (process-birth-token pid)
        start (process-start-millis pid)
        controller-pid (systemd-main-pid! controller-unit)
        process-shape (exact-process-shape! pid shape)
        environment (process-environment pid)
        disagreements (environment-disagreements expectation environment)]
    (when-not (and (= (long port) record-port)
                   (= requested-log log)
                   (= space-id (get values "BEAGLE_STORE_SPACE_ID"))
                   (= controller-unit (get values "CONTROLLER_UNIT"))
                   (= pid main-pid)
                   (= [pid] listener-owners)
                   (= (get values "PID_BIRTH") birth)
                   (integer? start)
                   (= pid controller-pid)
                   (empty? disagreements))
      (fail! "Store RPC runtime record, listener, environment, and systemd owner disagree"
             :runtime-process-attestation-failed
             {:pid pid :listener-pids listener-owners
              :expected-birth (get values "PID_BIRTH") :actual-birth birth
              :process-start-millis start :controller-pid controller-pid
              :environment-disagreements disagreements
              :environment-keys
              (sort (keys (runtime-identity-environment environment)))}))
    {:pid pid :pid-birth birth :process-start-millis start
     :port record-port :log log :arguments-sha256 (:arguments-sha256 process-shape)
     :controller {:kind "systemd" :unit controller-unit :main-pid controller-pid}
     :record {:path (:path sealed)
              :bytes (alength ^bytes (:bytes sealed))
              :sha256 (:sha256 sealed) :state (:state sealed)}}))

(defn- old-native-record-with-selection! []
  (let [selector (.toPath (io/file (runtime-state-root) "active"))]
    (when (or (java.nio.file.Files/isSymbolicLink selector)
              (java.nio.file.Files/exists selector (no-follow-options)))
      (fail! "Selected generations require a North-produced runtime record"
             :runtime-generation-mismatch {:selector (str selector)}))))

(defn- attest-native-runtime!
  [request sealed]
  (let [values (:values sealed)
        managed? (= managed-native-runtime-record-format (get values "FORMAT"))
        generation (if managed?
                     (validate-generation-evidence! values)
                     (do (old-native-record-with-selection!) nil))
        release-home (if managed?
                       (get values "BEAGLE_STORE_SOURCE")
                       (sealed-release-home))
        release (sealed-release-identity! release-home)
        record-port (parse-positive-long! "BEAGLE_STORE_PORT" (get values "BEAGLE_STORE_PORT"))
        pid (parse-positive-long! "PID" (get values "PID"))
        source-field (get values "BEAGLE_STORE_SOURCE")
        source (.getCanonicalPath (io/file source-field))
        closure-sha256 (get values "BEAGLE_STORE_NATIVE_CLOSURE_SHA256")
        _ (when-not (re-matches #"[0-9a-f]{64}" closure-sha256)
            (fail! "Native closure SHA-256 is not canonical"
                   :runtime-record-invalid {:sha256 closure-sha256}))
        artifact-directory
        (canonical-directory! "Native READY artifact directory"
                              (get values "BEAGLE_STORE_NATIVE_ARTIFACT_DIR"))
        ready-path (.getPath (io/file artifact-directory "READY"))
        manifest-path (.getPath (io/file artifact-directory "input.manifest"))
        expected-server-path
        (.getPath (io/file artifact-directory "bin" "beagle-store-server-native"))
        ready (ready-record! ready-path closure-sha256)
        input-manifest (artifact-record "Native input manifest" manifest-path)
        server-artifact
        (artifact-record "sealed Native Beagle Store server" expected-server-path)
        log (.getCanonicalPath (io/file (get values "BEAGLE_STORE_LOG")))
        typed manifest/accepted-native-runtime]
    (when-not (and (= (:source release) source-field source)
                   (= (:revision release) (get values "BEAGLE_STORE_REVISION"))
                   (= (:tree release) (get values "BEAGLE_STORE_TREE"))
                   (= (:native-artifact-dir release) artifact-directory
                      (get values "BEAGLE_STORE_NATIVE_ARTIFACT_DIR"))
                   (= (:native-closure-sha256 release) closure-sha256)
                   (= (:server-artifact-sha256 release)
                      (get values "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"))
                   (= expected-server-path
                      (get values "BEAGLE_STORE_SERVER_ARTIFACT")
                      (:path server-artifact))
                   (or (not managed?)
                       (and (= (:release-root typed) source)
                            (= (:beagle-revision typed)
                               (get values "BEAGLE_STORE_REVISION"))
                            (= (:beagle-tree typed)
                               (get values "BEAGLE_STORE_TREE"))
                            (= (:artifact-root typed) artifact-directory)
                            (= (:closure-sha256 typed) closure-sha256)
                            (= (:server-artifact typed) expected-server-path)
                            (= (:server-sha256 typed)
                               (get values
                                "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"))
                            (= typed (:current (:generation generation)))))
                   (= closure-sha256 (:sha256 input-manifest))
                   (= (:sha256 server-artifact)
                      (get values "BEAGLE_STORE_SERVER_ARTIFACT_SHA256"))
                   (.canExecute (io/file expected-server-path)))
      (fail! "Store RPC runtime record does not bind the requested canonical writer"
             :runtime-record-invalid
             {:request request
              :release (dissoc release :receipt)
              :record (dissoc values "PID_BIRTH")}))
    (let [authority
          (common-runtime-authority!
           request sealed values
           {:cwd source :executable expected-server-path :arguments [expected-server-path]}
           (sealed-environment-expectation
            release
            {:space-id (:space-id request) :port record-port :log log
             :controller-unit (:controller-unit request)
             :server-artifact expected-server-path}))]
      {:format attestation-format
       :request {:port record-port :served-log log
                 :space-id (:space-id request)
                 :record-path (:path sealed)
                 :controller-unit (:controller-unit request)}
       :identity (merge (select-keys release
                                     [:source :revision :tree :cut-from :receipt])
                        {:runtime-kind "native"
                         :generation generation
                         :native-artifact
                         {:directory artifact-directory
                          :closure-sha256 closure-sha256
                          :ready ready
                          :input-manifest (dissoc input-manifest :state)
                          :server (dissoc server-artifact :state)}
                         :space-id (:space-id request) :port record-port
                         :served-log log
                         :arguments-sha256 (:arguments-sha256 authority)})
       :authority (dissoc authority :port :log :arguments-sha256)})))

(defn- jvm-process-arguments [facts port log space-id]
  [(:java facts)
   "-Xmx2g"
   "-XX:+UseG1GC"
   "-XX:G1HeapRegionSize=32m"
   "-XX:+ExitOnOutOfMemoryError"
   "-XX:+HeapDumpOnOutOfMemoryError"
   (str "-XX:HeapDumpPath=" log ".heap.hprof")
   "-cp" (:classpath facts)
   "clojure.main" "server.clj" "serve" (str port) log space-id])

(defn- attest-jvm-runtime! [request sealed]
  (let [values (:values sealed)
        generation (validate-generation-evidence! values)
        output (get values "BEAGLE_STORE_OUTPUT")
        manifest-path (get values "BEAGLE_STORE_MANIFEST")
        manifest-record (artifact-record "Store JVM runtime manifest" manifest-path)
        manifest-text (slurp (:path manifest-record))
        member (manifest/accepted-jvm-runtime!
                output
                (get values "BEAGLE_STORE_PACKAGE_NAR_SHA256")
                (:sha256 manifest-record)
                manifest-text)
        facts (jvm-runtime-facts! member)
        record-port (parse-positive-long! "BEAGLE_STORE_PORT"
                                          (get values "BEAGLE_STORE_PORT"))
        log (.getCanonicalPath (io/file (get values "BEAGLE_STORE_LOG")))
        pid (parse-positive-long! "PID" (get values "PID"))]
    (when-not
     (and (= "jvm" (get values "RUNTIME_KIND"))
          (= member (:current (:generation generation)))
          (= (:beagle-revision member) (get values "BEAGLE_STORE_REVISION"))
          (= (:beagle-tree member) (get values "BEAGLE_STORE_TREE"))
          (= (:manifest-path member) manifest-path (:path manifest-record))
          (= (:manifest-bytes member)
             (parse-positive-long! "BEAGLE_STORE_MANIFEST_BYTES"
                                   (get values "BEAGLE_STORE_MANIFEST_BYTES")))
          (= (:manifest-sha256 member)
             (get values "BEAGLE_STORE_MANIFEST_SHA256"))
          (= (:java facts) (get values "BEAGLE_STORE_JAVA"))
          (= (:classpath-file facts)
             (get values "BEAGLE_STORE_SERVER_CLASSPATH_FILE"))
          (= (:classpath-sha256 facts)
             (get values "BEAGLE_STORE_SERVER_CLASSPATH_SHA256")))
      (fail! "Store JVM runtime record differs from the accepted package"
             :runtime-record-invalid {:record (dissoc values "PID_BIRTH")}))
    (let [authority
          (common-runtime-authority!
           request sealed values
           {:cwd (:home facts)
            :executable (:java-executable facts)
            :arguments (jvm-process-arguments
                        facts record-port log (:space-id request))}
           (jvm-environment-expectation
            facts {:space-id (:space-id request) :port record-port :log log
                   :controller-unit (:controller-unit request)}))]
      {:format attestation-format
       :request {:port record-port :served-log log
                 :space-id (:space-id request)
                 :record-path (:path sealed)
                 :controller-unit (:controller-unit request)}
       :identity {:runtime-kind "jvm"
                  :generation generation
                  :output output
                  :package-nar-sha256 (:package-nar-sha256 member)
                  :revision (:beagle-revision member)
                  :tree (:beagle-tree member)
                  :manifest (dissoc manifest-record :state)
                  :java (:java facts)
                  :classpath-file (:classpath-file facts)
                  :classpath-sha256 (:classpath-sha256 facts)
                  :space-id (:space-id request) :port record-port
                  :served-log log
                  :arguments-sha256 (:arguments-sha256 authority)}
       :authority (dissoc authority :port :log :arguments-sha256)})))

(defn attest-active-runtime!
  "Attest one active canonical writer from its launcher-owned 0600 record."
  [{:keys [port served-log space-id record-path controller-unit] :as request}]
  (when-not (and port served-log space-id record-path controller-unit)
    (fail! "Store RPC runtime attestation requires port, log, SpaceId, record, and unit"
           :runtime-attestation-request-invalid))
  (let [sealed (read-record! record-path)
        format (get-in sealed [:values "FORMAT"])]
    (case format
      "north-store-runtime/v1" (attest-native-runtime! request sealed)
      "north-store-native-runtime/v2" (attest-native-runtime! request sealed)
      "north-store-jvm-runtime/v1" (attest-jvm-runtime! request sealed)
      (fail! "Store RPC runtime identity has an unsupported format"
             :runtime-record-invalid {:format format}))))

(defn attest-runtime-record! [record-path]
  (let [sealed (read-record! record-path)
        values (:values sealed)]
    (attest-active-runtime!
     {:port (parse-positive-long! "BEAGLE_STORE_PORT"
                                  (get values "BEAGLE_STORE_PORT"))
      :served-log (get values "BEAGLE_STORE_LOG")
      :space-id (get values "BEAGLE_STORE_SPACE_ID")
      :record-path record-path
      :controller-unit (get values "CONTROLLER_UNIT")})))

(defn- write-runtime-record! [record-path order values]
  (let [record (.toAbsolutePath (.normalize (.toPath (io/file record-path))))
        directory (.getParent record)
        temporary (.resolve directory
                            (str "." (.getFileName record) ".next."
                                 (java.util.UUID/randomUUID)))
        payload (.getBytes
                 (str (str/join "\n" (map #(str % "=" (get values %)) order))
                      "\n")
                 java.nio.charset.StandardCharsets/UTF_8)
        permissions
        (java.util.HashSet.
         ^java.util.Collection
         [java.nio.file.attribute.PosixFilePermission/OWNER_READ
          java.nio.file.attribute.PosixFilePermission/OWNER_WRITE])]
    (java.nio.file.Files/createDirectories
     directory (make-array java.nio.file.attribute.FileAttribute 0))
    (when (or (java.nio.file.Files/isSymbolicLink directory)
              (some #(str/blank? (get values %)) order))
      (fail! "Store RPC runtime record target or field is invalid"
             :runtime-record-invalid {:path (str record)}))
    (try
      (java.nio.file.Files/write
       temporary payload
       (into-array java.nio.file.OpenOption
                   [java.nio.file.StandardOpenOption/CREATE_NEW
                    java.nio.file.StandardOpenOption/WRITE]))
      (java.nio.file.Files/setPosixFilePermissions temporary permissions)
      (java.nio.file.Files/move
       temporary record
       (into-array java.nio.file.CopyOption
                   [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                    java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (finally
        (java.nio.file.Files/deleteIfExists temporary)))
    (str record)))

(defn publish-runtime-record!
  [{:keys [member generation-evidence pid controller-unit record-path selection]}]
  (let [checked (manifest/validate-runtime-member! member)
        pid (parse-positive-long! "listener PID" pid)
        main-pid (binding [*allow-controller-starting?* true]
                   (systemd-main-pid! controller-unit))
        birth (process-birth-token pid)
        spec (launch-spec! checked selection)
        environment (:environment spec)
        common
        {"GENERATION_TARGET" (:target generation-evidence)
         "GENERATION_SHA256" (:sha256 generation-evidence)
         "BEAGLE_STORE_SPACE_ID" (get environment "BEAGLE_STORE_SPACE_ID")
         "BEAGLE_STORE_PORT" (get environment "BEAGLE_STORE_SERVER_PORT")
         "BEAGLE_STORE_LOG" (get environment "BEAGLE_STORE_LOG")
         "PID" (str pid)
         "PID_BIRTH" birth
         "CONTROLLER_UNIT" controller-unit
         "CONTROLLER_MAIN_PID" (str main-pid)}]
    (when-not (and (= pid main-pid)
                   (not (str/blank? birth))
                   (re-matches #"generations/[^/]+" (:target generation-evidence))
                   (re-matches #"[0-9a-f]{64}" (:sha256 generation-evidence))
                   (= checked
                      (:current
                       (manifest/validate-runtime-generation!
                        (:generation generation-evidence)))))
      (fail! "Store RPC listener and selected generation are not publishable"
             :runtime-record-invalid
             {:pid pid :main-pid main-pid :generation generation-evidence}))
    (let [[order values]
          (case (manifest/runtime-member-kind checked)
            "native"
            [managed-native-runtime-record-order
             (merge common
                    {"FORMAT" managed-native-runtime-record-format
                     "RUNTIME_KIND" "native"
                     "BEAGLE_STORE_SOURCE" (:release-root checked)
                     "BEAGLE_STORE_REVISION" (:beagle-revision checked)
                     "BEAGLE_STORE_TREE" (:beagle-tree checked)
                     "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" (:artifact-root checked)
                     "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" (:closure-sha256 checked)
                     "BEAGLE_STORE_SERVER_ARTIFACT" (:server-artifact checked)
                     "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" (:server-sha256 checked)})]

            "jvm"
            (let [facts (jvm-runtime-facts! checked)]
              [jvm-runtime-record-order
               (merge common
                      {"FORMAT" jvm-runtime-record-format
                       "RUNTIME_KIND" "jvm"
                       "BEAGLE_STORE_OUTPUT" (:output checked)
                       "BEAGLE_STORE_REVISION" (:beagle-revision checked)
                       "BEAGLE_STORE_TREE" (:beagle-tree checked)
                       "BEAGLE_STORE_PACKAGE_NAR_SHA256"
                       (:package-nar-sha256 checked)
                       "BEAGLE_STORE_MANIFEST" (:manifest-path checked)
                       "BEAGLE_STORE_MANIFEST_BYTES" (str (:manifest-bytes checked))
                       "BEAGLE_STORE_MANIFEST_SHA256" (:manifest-sha256 checked)
                       "BEAGLE_STORE_JAVA" (:java facts)
                       "BEAGLE_STORE_SERVER_CLASSPATH_FILE" (:classpath-file facts)
                       "BEAGLE_STORE_SERVER_CLASSPATH_SHA256"
                       (:classpath-sha256 facts)})]))
          written (write-runtime-record! record-path order values)]
      (binding [*store-runtime-state-root* (:state-root generation-evidence)]
        (binding [*allow-controller-starting?* true]
          (attest-runtime-record! written))))))

(defn assert-current!
  "Re-attest and require the same exact process and sealed runtime identity."
  [attestation]
  (let [current
        (try
          (attest-active-runtime! (:request attestation))
          (catch Throwable error
            (fail! "selected frozen Beagle Store authority changed"
                   :runtime-authority-lost
                   {:cause (.getMessage error) :cause-data (ex-data error)})))]
    (when-not (= attestation current)
      (fail! "selected frozen Beagle Store authority changed"
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
