(ns north.concern-spool
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.io PushbackReader StringReader]
           [java.nio ByteBuffer]
           [java.nio.channels FileChannel]
           [java.nio.charset CodingErrorAction StandardCharsets]
           [java.nio.file CopyOption Files LinkOption OpenOption Path
            StandardCopyOption StandardOpenOption]
           [java.nio.file.attribute FileAttribute PosixFilePermission
            PosixFilePermissions]
           [java.security MessageDigest]
           [java.time Instant]))

(def schema-version "north-concern-operation-v1")
(def operation-type "concern-declare")
(def commit-marker-value "north-local-operation-committed-v1")
(def default-max-record-bytes (* 64 1024))
(def default-max-files 256)
(def default-max-total-bytes (* 8 1024 1024))
(def ^:private maximum-configured-record-bytes (* 1024 1024))
(def ^:private maximum-configured-files 4096)
(def ^:private maximum-configured-total-bytes (* 64 1024 1024))
(def ^:private final-suffix ".op.edn")
(def ^:private temp-suffix ".tmp")
(def ^:private lock-name ".publish.lock")
(def ^:private directory-permissions
  #{PosixFilePermission/OWNER_READ
    PosixFilePermission/OWNER_WRITE
    PosixFilePermission/OWNER_EXECUTE})
(def ^:private file-permissions
  #{PosixFilePermission/OWNER_READ
    PosixFilePermission/OWNER_WRITE})
(def ^:private no-follow
  (into-array LinkOption [LinkOption/NOFOLLOW_LINKS]))

(def ^:dynamic *limits-override* nil)
(def ^:dynamic *publish-stage!*
  "Test seam for deterministic crash-point injection. Production is a no-op."
  (fn [_stage _path] nil))

(defn- fail! [message data]
  (throw (ex-info message data)))

(defn- configured-positive-integer [name default maximum]
  (let [override-key
        (case name
          "NORTH_CONCERN_SPOOL_MAX_RECORD_BYTES" :max-record-bytes
          "NORTH_CONCERN_SPOOL_MAX_FILES" :max-files
          "NORTH_CONCERN_SPOOL_MAX_TOTAL_BYTES" :max-total-bytes)
        raw (or (some-> *limits-override* override-key str)
                (System/getenv name)
                (str default))
        value (when (re-matches #"[1-9][0-9]*" raw) (parse-long raw))]
    (when-not (and value (<= value maximum))
      (fail!
       (str name " must be an integer from 1 through " maximum)
       {:type :invalid-concern-spool-limit
        :name name
        :value raw
        :maximum maximum}))
    value))

(defn limits []
  {:max-record-bytes
   (configured-positive-integer
    "NORTH_CONCERN_SPOOL_MAX_RECORD_BYTES"
    default-max-record-bytes
    maximum-configured-record-bytes)
   :max-files
   (configured-positive-integer
    "NORTH_CONCERN_SPOOL_MAX_FILES"
    default-max-files
    maximum-configured-files)
   :max-total-bytes
   (configured-positive-integer
    "NORTH_CONCERN_SPOOL_MAX_TOTAL_BYTES"
    default-max-total-bytes
    maximum-configured-total-bytes)})

(defn state-directory []
  (let [override (System/getenv "NORTH_CONCERN_SPOOL_DIR")
        home (or (System/getenv "HOME") (System/getProperty "user.home"))
        state-home (or (System/getenv "XDG_STATE_HOME")
                       (str home "/.local/state"))
        path (io/file (or override
                          (str state-home "/north/concern-operations")))]
    (when-not (.isAbsolute path)
      (fail! "concern spool directory must be an absolute path"
             {:type :invalid-concern-spool-directory
              :path (.getPath path)}))
    (.toPath (.getCanonicalFile path))))

(defn- canonical-key-compare [left right]
  (compare (pr-str left) (pr-str right)))

(defn- canonical-value [value]
  (cond
    (map? value)
    (into (sorted-map-by canonical-key-compare)
          (map (fn [[key item]] [key (canonical-value item)]))
          value)

    (vector? value)
    (mapv canonical-value value)

    (sequential? value)
    (mapv canonical-value value)

    :else value))

(defn canonical-edn [value]
  (pr-str (canonical-value value)))

(defn sha256 [value]
  (let [digest
        (.digest
         (MessageDigest/getInstance "SHA-256")
         (.getBytes ^String value StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" (bit-and 0xff %)) digest))))

(defn- canonical-log-path [value]
  (when-not (and (string? value) (not (str/blank? value)))
    (fail! "offline concern intent requires an exact target log"
           {:type :invalid-concern-operation
            :field :target-log}))
  (let [file (io/file value)]
    (when-not (.isAbsolute file)
      (fail! "offline concern target log must be absolute"
             {:type :invalid-concern-operation
              :field :target-log
              :value value}))
    (.getCanonicalPath file)))

(defn- canonical-facts [facts]
  (when-not (and (vector? facts) (seq facts))
    (fail! "offline concern intent requires a nonempty ordered fact vector"
           {:type :invalid-concern-operation
            :field :facts}))
  (mapv
   (fn [ordinal fact]
     (let [{:keys [predicate object cardinality]} fact]
       (when-not
        (and (= #{:predicate :object :cardinality} (set (keys fact)))
             (string? predicate)
             (not (str/blank? predicate))
             (string? object)
             (#{"single" "multi"} cardinality))
         (fail! "offline concern intent contains a malformed fact"
                {:type :invalid-concern-operation
                 :field :facts
                 :ordinal ordinal}))
       (sorted-map
        :ordinal ordinal
        :predicate predicate
        :object object
        :cardinality cardinality)))
   (range)
   facts))

(defn build-operation
  "Build the complete immutable concern-declare operation before transport."
  [{:keys [operation-id concern-id target-log created-at facts about]}]
  (when-not (and (string? operation-id)
                 (re-matches #"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
                             operation-id))
    (fail! "offline concern operation id must be a lowercase UUID"
           {:type :invalid-concern-operation
            :field :operation-id}))
  (when-not (and (string? concern-id)
                 (re-matches #"@concern-[0-9]{10,}-[0-9a-f]{4}" concern-id))
    (fail! "offline concern id is not canonical"
           {:type :invalid-concern-operation
            :field :concern-id}))
  (try
    (Instant/parse created-at)
    (catch Exception _
      (fail! "offline concern creation instant is not ISO-8601"
             {:type :invalid-concern-operation
              :field :created-at})))
  (when (and about
             (not (and (string? about)
                       (re-matches #"@[A-Za-z0-9][A-Za-z0-9._:-]*" about))))
    (fail! "offline concern about precondition must be an exact ref"
           {:type :invalid-concern-operation
            :field :about}))
  (let [ordered-facts (canonical-facts facts)
        terminal-fact (peek ordered-facts)
        _ (when-not
            (and (= "kind" (:predicate terminal-fact))
                 (= "concern" (:object terminal-fact))
                 (= "single" (:cardinality terminal-fact)))
            (fail! "kind=concern must be the terminal fact"
                   {:type :invalid-concern-operation
                    :field :terminal-commit-marker}))
        facts-digest (sha256 (canonical-edn ordered-facts))
        precondition
        (cond->
         (sorted-map
          :mode "subject-absent-or-exact"
          :subject concern-id
          :projection-sha256 facts-digest)
          about
          (assoc :about
                 (sorted-map
                  :subject about
                  :requires-kind "thread"
                  :requires-title true)))
        precondition-digest (sha256 (canonical-edn precondition))
        commit-marker
        (sorted-map
         :ordinal (:ordinal terminal-fact)
         :predicate "kind"
         :object "concern")
        unsigned
        (sorted-map
         :schema-version schema-version
         :operation-type operation-type
         :operation-id operation-id
         :concern-id concern-id
         :target-log (canonical-log-path target-log)
         :created-at created-at
         :facts ordered-facts
         :facts-sha256 facts-digest
         :precondition precondition
         :precondition-sha256 precondition-digest
         :terminal-commit-marker commit-marker
         :commit commit-marker-value)]
    (assoc unsigned :sha256 (sha256 (canonical-edn unsigned)))))

(defn validate-operation!
  "Validate a parsed operation and return its canonical value."
  [operation]
  (let [expected-keys
        #{:schema-version :operation-type :operation-id :concern-id :target-log
          :created-at :facts :facts-sha256 :precondition :precondition-sha256
          :terminal-commit-marker :commit :sha256}]
    (when-not (= expected-keys (set (keys operation)))
      (fail! "offline concern operation has an unknown or missing field"
             {:type :invalid-concern-operation
              :field :top-level}))
    (let [rebuilt
          (build-operation
           {:operation-id (:operation-id operation)
            :concern-id (:concern-id operation)
            :target-log (:target-log operation)
            :created-at (:created-at operation)
            :about (get-in operation [:precondition :about :subject])
            :facts
            (mapv #(select-keys % [:predicate :object :cardinality])
                  (:facts operation))})]
      (when-not (= schema-version (:schema-version operation))
        (fail! "unsupported offline concern operation schema"
               {:type :invalid-concern-operation
                :field :schema-version}))
      (when-not (= operation-type (:operation-type operation))
        (fail! "unsupported offline concern operation type"
               {:type :invalid-concern-operation
                :field :operation-type}))
      (when-not (= commit-marker-value (:commit operation))
        (fail! "offline concern operation lacks its terminal commit marker"
               {:type :invalid-concern-operation
                :field :commit}))
      (when-not (= rebuilt (canonical-value operation))
        (fail! "offline concern operation digest or projection does not match"
               {:type :invalid-concern-operation
                :field :sha256}))
      rebuilt)))

(defn- decode-utf8! [bytes]
  (let [decoder
        (doto (.newDecoder StandardCharsets/UTF_8)
          (.onMalformedInput CodingErrorAction/REPORT)
          (.onUnmappableCharacter CodingErrorAction/REPORT))]
    (try
      (str (.decode decoder (ByteBuffer/wrap bytes)))
      (catch Exception error
        (fail! "offline concern operation is not valid UTF-8"
               {:type :invalid-concern-operation
                :cause (.getMessage error)})))))

(defn- read-one-edn! [text]
  (try
    (with-open [reader (PushbackReader. (StringReader. text))]
      (let [eof (Object.)
            value (edn/read {:eof eof} reader)
            trailing (edn/read {:eof eof} reader)]
        (when (or (identical? eof value)
                  (not (identical? eof trailing)))
          (fail! "offline concern operation is not exactly one EDN form"
                 {:type :invalid-concern-operation}))
        value))
    (catch clojure.lang.ExceptionInfo error
      (throw error))
    (catch Exception error
      (fail! "offline concern operation is malformed EDN"
             {:type :invalid-concern-operation
              :cause (.getMessage error)}))))

(defn read-operation-file! [path]
  (let [path (if (instance? Path path) path (.toPath (io/file path)))
        {:keys [max-record-bytes]} (limits)
        size (Files/size path)]
    (when (> size max-record-bytes)
      (fail! "offline concern operation exceeds its record bound"
             {:type :concern-spool-record-too-large
              :max-bytes max-record-bytes
              :bytes size}))
    (let [text (decode-utf8! (Files/readAllBytes path))
          operation (validate-operation! (read-one-edn! text))]
      (when-not (= (str (canonical-edn operation) "\n") text)
        (fail! "offline concern operation is not canonical EDN"
               {:type :invalid-concern-operation
                :field :encoding}))
      operation)))

(defn- set-permissions! [path permissions]
  (Files/setPosixFilePermissions path permissions)
  (when-not (= permissions (Files/getPosixFilePermissions path no-follow))
    (fail! "concern spool permissions could not be enforced"
           {:type :invalid-concern-spool-permissions
            :path (str path)})))

(defn- ensure-state-directory! [path]
  (when (Files/isSymbolicLink path)
    (fail! "concern spool directory may not be a symlink"
           {:type :invalid-concern-spool-directory
            :path (str path)}))
  (Files/createDirectories
   path
   (into-array
    FileAttribute
    [(PosixFilePermissions/asFileAttribute directory-permissions)]))
  (when-not (and (Files/isDirectory path no-follow)
                 (not (Files/isSymbolicLink path)))
    (fail! "concern spool path is not a directory"
           {:type :invalid-concern-spool-directory
            :path (str path)}))
  (set-permissions! path directory-permissions)
  path)

(defn- open-lock-channel [path]
  (when (Files/isSymbolicLink path)
    (fail! "concern spool publication lock may not be a symlink"
           {:type :invalid-concern-spool-entry
            :path (str path)}))
  (let [channel
        (FileChannel/open
         path
         (into-array
          OpenOption
          [StandardOpenOption/CREATE
           StandardOpenOption/WRITE
           LinkOption/NOFOLLOW_LINKS]))]
    (set-permissions! path file-permissions)
    channel))

(defn- acquire-lock! [channel deadline-ns]
  (loop []
    (when (and deadline-ns (not (< (System/nanoTime) deadline-ns)))
      (fail! "concern spool publication deadline exceeded while waiting for capacity"
             {:type :concern-spool-busy}))
    (let [lock
          (try
            (.tryLock channel)
            (catch Exception _ nil))]
      (if lock
        lock
        (do
          (Thread/sleep 5)
          (recur))))))

(defn- operation-file? [^Path path]
  (str/ends-with? (str (.getFileName path)) final-suffix))

(defn- temp-file? [^Path path]
  (str/ends-with? (str (.getFileName path)) temp-suffix))

(defn- directory-entries [^Path directory]
  (with-open [stream (Files/newDirectoryStream directory)]
    (vec (seq stream))))

(defn- fsync-directory! [^Path directory]
  (with-open [channel
              (FileChannel/open
               directory
               (into-array OpenOption [StandardOpenOption/READ]))]
    (.force channel true)))

(defn- recover-and-scan! [^Path directory]
  (let [entries (directory-entries directory)
        temps (filter temp-file? entries)]
    (doseq [path temps]
      (when (or (Files/isSymbolicLink path)
                (not (Files/isRegularFile path no-follow)))
        (fail! "concern spool contains an unsafe temporary entry"
               {:type :invalid-concern-spool-entry
                :path (str path)}))
      (Files/delete path))
    (when (seq temps) (fsync-directory! directory))
    (reduce
     (fn [{:keys [file-count total-bytes] :as state} path]
       (let [name (str (.getFileName ^Path path))]
         (cond
           (= lock-name name)
           state

           (operation-file? path)
           (do
             (when (or (Files/isSymbolicLink path)
                       (not (Files/isRegularFile path no-follow)))
               (fail! "concern spool contains an unsafe operation entry"
                      {:type :invalid-concern-spool-entry
                       :path (str path)}))
             (set-permissions! path file-permissions)
             (read-operation-file! path)
             {:file-count (inc file-count)
              :total-bytes (+ total-bytes (Files/size path))})

           :else
           (fail! "concern spool contains an unknown entry"
                  {:type :invalid-concern-spool-entry
                   :path (str path)}))))
     {:file-count 0 :total-bytes 0}
     (directory-entries directory))))

(defn- write-file-fsynced! [^Path path bytes]
  (with-open [channel
              (FileChannel/open
               path
               #{StandardOpenOption/CREATE_NEW StandardOpenOption/WRITE}
               (into-array
                FileAttribute
                [(PosixFilePermissions/asFileAttribute file-permissions)]))]
    (let [buffer (ByteBuffer/wrap bytes)]
      (while (.hasRemaining buffer)
        (.write channel buffer)))
    (.force channel true))
  (set-permissions! path file-permissions))

(defn publish-operation!
  "Publish one immutable operation through exclusive temp + file fsync + atomic
   rename + directory fsync. DEADLINE-NS bounds only lock contention; filesystem
   durability calls remain fail-closed if the platform cannot complete them."
  ([operation]
   (publish-operation!
    operation
    (+ (System/nanoTime) (* 1000000 1000))))
  ([operation deadline-ns]
   (let [operation (validate-operation! operation)
         payload (.getBytes (str (canonical-edn operation) "\n")
                            StandardCharsets/UTF_8)
         payload-bytes (alength payload)
         {:keys [max-record-bytes max-files max-total-bytes]} (limits)
         _ (when (> payload-bytes max-record-bytes)
             (fail! "offline concern operation exceeds its record bound"
                    {:type :concern-spool-record-too-large
                     :max-bytes max-record-bytes
                     :bytes payload-bytes}))
         directory (ensure-state-directory! (state-directory))
         final-name (str (:operation-id operation) final-suffix)
         final-path (.resolve directory final-name)
         temp-path (.resolve
                    directory
                    (str "." final-name "." (java.util.UUID/randomUUID)
                         temp-suffix))
         lock-path (.resolve directory lock-name)
         renamed? (atom false)]
     (with-open [lock-channel (open-lock-channel lock-path)]
       (let [_lock (acquire-lock! lock-channel deadline-ns)
             {:keys [file-count total-bytes]} (recover-and-scan! directory)]
         (when (or (>= file-count max-files)
                   (> (+ total-bytes payload-bytes) max-total-bytes))
           (fail! "concern spool is full; operation was not published"
                  {:type :concern-spool-full
                   :file-count file-count
                   :max-files max-files
                   :total-bytes total-bytes
                   :record-bytes payload-bytes
                   :max-total-bytes max-total-bytes}))
         (when (Files/exists final-path no-follow)
           (fail! "concern spool operation id already exists"
                  {:type :concern-spool-operation-exists
                   :operation-id (:operation-id operation)}))
         (try
           (write-file-fsynced! temp-path payload)
           (*publish-stage!* :file-fsynced temp-path)
           (try
             (Files/move
              temp-path
              final-path
              (into-array CopyOption [StandardCopyOption/ATOMIC_MOVE]))
             (catch Exception error
               (fail! "concern spool atomic rename failed"
                      {:type :concern-spool-atomic-move-failed
                       :cause (.getMessage error)})))
           (reset! renamed? true)
           (*publish-stage!* :renamed final-path)
           (set-permissions! final-path file-permissions)
           (fsync-directory! directory)
           (*publish-stage!* :directory-fsynced final-path)
           {:durability "durable-local"
            :visibility "pending"
            :operation-id (:operation-id operation)
            :concern-id (:concern-id operation)
            :target-log (:target-log operation)
            :path (str final-path)
            :sha256 (:sha256 operation)}
           (catch Throwable error
             (when-not @renamed?
               (try
                 (when (Files/deleteIfExists temp-path)
                   (fsync-directory! directory))
                 (catch Throwable _ nil)))
             (throw error))))))))
