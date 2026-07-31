(ns north.concern-spool-reconcile
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.io BufferedInputStream PushbackReader StringReader]
           [java.nio ByteBuffer]
           [java.nio.channels FileChannel]
           [java.nio.charset CodingErrorAction StandardCharsets]
           [java.nio.file CopyOption Files LinkOption OpenOption Path
            StandardCopyOption StandardOpenOption]
           [java.nio.file.attribute FileAttribute PosixFilePermission
            PosixFilePermissions]
           [java.security MessageDigest]
           [java.time Instant]))

(def reconciliation-schema "north-concern-reconciliation-v1")
(def reconciliation-commit "north-concern-reconciliation-committed-v1")
(def default-max-items 32)
(def default-max-bytes (* 2 1024 1024))
(def default-max-millis 5000)
(def ^:private maximum-items 4096)
(def ^:private maximum-bytes (* 64 1024 1024))
(def ^:private maximum-millis 60000)
(def ^:private marker-max-bytes (* 64 1024))
(def ^:private settled-suffix ".settled.edn")
(def ^:private conflict-suffix ".conflict.edn")
(def ^:private temporary-suffix ".tmp")
(def ^:private lock-name ".reconcile.lock")
(def ^:private cursor-name ".cursor.edn")
(def ^:private cursor-schema "north-concern-reconcile-cursor-v1")
(def ^:private cursor-commit "north-concern-reconcile-cursor-committed-v1")
(def ^:private maximum-spool-entries 8192)
(def ^:private operation-name-pattern
  #"^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.op\.edn$")
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
(def ^:dynamic *reconcile-stage!*
  "Test seam for deterministic crash and lock-contention injection."
  (fn [_stage _context] nil))
(def ^:dynamic *transition-plan!*
  "Concern CLI callback that rebuilds the exact transition batch after the
   reconciler has captured its global base. Nil defers transition operations."
  nil)

(defn- fail! [message data]
  (throw (ex-info message data)))

(defn- conflict! [reason data]
  (fail!
   (str "offline concern reconciliation conflict: " reason)
   (assoc data :type :concern-reconciliation-conflict :reason reason)))

(defn- deferred! [message data]
  (fail! message (assoc data :type :concern-reconciliation-deferred)))

(defn- ->path [value]
  (if (instance? Path value)
    value
    (.toPath (io/file value))))

(defn- configured-positive-integer [env-name override-key default maximum]
  (let [raw (or (some-> *limits-override* override-key str)
                (System/getenv env-name)
                (str default))
        value (when (re-matches #"[1-9][0-9]*" raw) (parse-long raw))]
    (when-not (and value (<= value maximum))
      (fail!
       (str env-name " must be an integer from 1 through " maximum)
       {:type :invalid-concern-reconciliation-limit
        :name env-name
        :value raw
        :maximum maximum}))
    value))

(defn limits []
  {:max-items
   (configured-positive-integer
    "NORTH_CONCERN_RECONCILE_MAX_ITEMS"
    :max-items
    default-max-items
    maximum-items)
   :max-bytes
   (configured-positive-integer
    "NORTH_CONCERN_RECONCILE_MAX_BYTES"
    :max-bytes
    default-max-bytes
    maximum-bytes)
   :max-millis
   (configured-positive-integer
    "NORTH_CONCERN_RECONCILE_MAX_MILLIS"
    :max-millis
    default-max-millis
    maximum-millis)})

(defn- validate-pass-limits! [configured]
  (let [{:keys [max-items max-bytes max-millis]} configured]
    (doseq [[field value maximum]
            [[:max-items max-items maximum-items]
             [:max-bytes max-bytes maximum-bytes]
             [:max-millis max-millis maximum-millis]]]
      (when-not (and (integer? value) (<= 1 value maximum))
        (fail! "concern reconciliation pass limit is invalid"
               {:type :invalid-concern-reconciliation-limit
                :field field
                :value value
                :maximum maximum})))
    configured))

(defn state-directory []
  (let [override (System/getenv "NORTH_CONCERN_RECONCILE_DIR")
        home (or (System/getenv "HOME") (System/getProperty "user.home"))
        state-home (or (System/getenv "XDG_STATE_HOME")
                       (str home "/.local/state"))
        file (io/file
              (or override
                  (str state-home "/north/concern-reconciliation")))]
    (when-not (.isAbsolute file)
      (fail! "concern reconciliation directory must be absolute"
             {:type :invalid-concern-reconciliation-directory
              :path (.getPath file)}))
    (.toPath (.getCanonicalFile file))))

(defn- set-permissions! [^Path path permissions]
  (Files/setPosixFilePermissions path permissions)
  (when-not (= permissions (Files/getPosixFilePermissions path no-follow))
    (fail! "concern reconciliation permissions could not be enforced"
           {:type :invalid-concern-reconciliation-permissions
            :path (str path)})))

(defn- ensure-directory! [^Path path]
  (when (Files/isSymbolicLink path)
    (fail! "concern reconciliation directory may not be a symlink"
           {:type :invalid-concern-reconciliation-directory
            :path (str path)}))
  (Files/createDirectories
   path
   (into-array
    FileAttribute
    [(PosixFilePermissions/asFileAttribute directory-permissions)]))
  (when-not (and (Files/isDirectory path no-follow)
                 (not (Files/isSymbolicLink path)))
    (fail! "concern reconciliation path is not a directory"
           {:type :invalid-concern-reconciliation-directory
            :path (str path)}))
  (set-permissions! path directory-permissions)
  path)

(defn- ensure-spool-directory! [^Path path]
  (cond
    (not (Files/exists path no-follow)) nil
    (or (Files/isSymbolicLink path)
        (not (Files/isDirectory path no-follow)))
    (fail! "concern spool path is not a safe directory"
           {:type :invalid-concern-spool-directory
            :path (str path)})
    :else path))

(defn- directory-entries [^Path directory]
  (with-open [stream (Files/newDirectoryStream directory)]
    (vec (seq stream))))

(defn- fsync-directory! [^Path directory]
  (with-open [channel
              (FileChannel/open
               directory
               (into-array OpenOption [StandardOpenOption/READ]))]
    (.force channel true)))

(defn- open-lock-channel [^Path path]
  (when (Files/isSymbolicLink path)
    (fail! "concern reconciliation lock may not be a symlink"
           {:type :invalid-concern-reconciliation-entry
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

(defn- try-lock [^FileChannel channel]
  (try
    (.tryLock channel)
    (catch Exception error
      (if (= "java.nio.channels.OverlappingFileLockException"
             (.getName (class error)))
        nil
        (throw error)))))

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

(defn- decode-utf8! [bytes]
  (let [decoder
        (doto (.newDecoder StandardCharsets/UTF_8)
          (.onMalformedInput CodingErrorAction/REPORT)
          (.onUnmappableCharacter CodingErrorAction/REPORT))]
    (try
      (str (.decode decoder (ByteBuffer/wrap bytes)))
      (catch Exception error
        (fail! "concern reconciliation record is not valid UTF-8"
               {:type :invalid-concern-reconciliation-record
                :cause (.getMessage error)})))))

(defn- read-one-edn! [text]
  (try
    (with-open [reader (PushbackReader. (StringReader. text))]
      (let [eof (Object.)
            value (edn/read {:eof eof} reader)
            trailing (edn/read {:eof eof} reader)]
        (when (or (identical? eof value)
                  (not (identical? eof trailing)))
          (fail! "concern reconciliation record is not exactly one EDN form"
                 {:type :invalid-concern-reconciliation-record}))
        value))
    (catch clojure.lang.ExceptionInfo error
      (throw error))
    (catch Exception error
      (fail! "concern reconciliation record is malformed EDN"
             {:type :invalid-concern-reconciliation-record
              :cause (.getMessage error)}))))

(defn- raw-file-sha256 [^Path path]
  (let [digest (MessageDigest/getInstance "SHA-256")
        buffer (byte-array 8192)]
    (with-open [input (BufferedInputStream. (io/input-stream (.toFile path)))]
      (loop []
        (let [read-count (.read input buffer)]
          (when (pos? read-count)
            (.update digest buffer 0 read-count)
            (recur)))))
    (apply str
           (map #(format "%02x" (bit-and 0xff %)) (.digest digest)))))

(defn- projection-sha256 [rows]
  (north.concern-spool/sha256
   (north.concern-spool/canonical-edn
    (vec (sort-by pr-str rows)))))

(defn- build-record
  [{:keys [record-type operation-id concern-id target-log operation-sha256
           reason observed-version observed-projection-sha256 created-at]}]
  (let [created-at (or created-at (str (Instant/now)))
        _ (try
            (Instant/parse created-at)
            (catch Exception _
              (fail! "concern reconciliation record instant is invalid"
                     {:type :invalid-concern-reconciliation-record
                      :field :created-at})))
        unsigned
        (sorted-map
         :schema-version reconciliation-schema
         :record-type record-type
         :operation-id operation-id
         :concern-id (or concern-id "")
         :target-log (or target-log "")
         :operation-sha256 operation-sha256
         :reason reason
         :observed-version (long (or observed-version -1))
         :observed-projection-sha256
         (or observed-projection-sha256 "")
         :created-at created-at
         :commit reconciliation-commit)]
    (when-not (#{"settled" "conflict"} record-type)
      (fail! "concern reconciliation record type is invalid"
             {:type :invalid-concern-reconciliation-record
              :field :record-type}))
    (when-not (and (string? operation-id)
                   (re-matches
                    #"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
                    operation-id))
      (fail! "concern reconciliation operation id is invalid"
             {:type :invalid-concern-reconciliation-record
              :field :operation-id}))
    (when-not (and (string? operation-sha256)
                   (re-matches #"[0-9a-f]{64}" operation-sha256))
      (fail! "concern reconciliation operation digest is invalid"
             {:type :invalid-concern-reconciliation-record
              :field :operation-sha256}))
    (when-not (and (string? reason) (not (str/blank? reason)))
      (fail! "concern reconciliation reason is invalid"
             {:type :invalid-concern-reconciliation-record
              :field :reason}))
    (assoc
     unsigned
     :sha256
     (north.concern-spool/sha256
      (north.concern-spool/canonical-edn unsigned)))))

(defn- validate-record! [record]
  (let [expected-keys
        #{:schema-version :record-type :operation-id :concern-id :target-log
          :operation-sha256 :reason :observed-version
          :observed-projection-sha256 :created-at :commit :sha256}]
    (when-not (= expected-keys (set (keys record)))
      (fail! "concern reconciliation record has unknown or missing fields"
             {:type :invalid-concern-reconciliation-record
              :field :top-level}))
    (let [rebuilt (build-record (dissoc record :schema-version :commit :sha256))]
      (when-not (= reconciliation-schema (:schema-version record))
        (fail! "concern reconciliation schema is unsupported"
               {:type :invalid-concern-reconciliation-record
                :field :schema-version}))
      (when-not (= reconciliation-commit (:commit record))
        (fail! "concern reconciliation record lacks its commit marker"
               {:type :invalid-concern-reconciliation-record
                :field :commit}))
      (when-not (= rebuilt record)
        (fail! "concern reconciliation record digest does not match"
               {:type :invalid-concern-reconciliation-record
                :field :sha256}))
      rebuilt)))

(defn- read-record! [^Path path]
  (let [size (Files/size path)]
    (when (> size marker-max-bytes)
      (fail! "concern reconciliation record exceeds its size bound"
             {:type :invalid-concern-reconciliation-record
              :bytes size
              :max-bytes marker-max-bytes}))
    (let [text (decode-utf8! (Files/readAllBytes path))
          record (validate-record! (read-one-edn! text))]
      (when-not (= (str (north.concern-spool/canonical-edn record) "\n") text)
        (fail! "concern reconciliation record is not canonical EDN"
               {:type :invalid-concern-reconciliation-record
                :field :encoding}))
      record)))

(defn- build-cursor [last-file]
  (when-not
   (and (string? last-file)
        (re-matches operation-name-pattern last-file))
    (fail! "concern reconciliation cursor has an invalid operation filename"
           {:type :invalid-concern-reconciliation-cursor
            :last-file last-file}))
  (let [unsigned
        (sorted-map
         :schema-version cursor-schema
         :last-file last-file
         :commit cursor-commit)]
    (assoc
     unsigned
     :sha256
     (north.concern-spool/sha256
      (north.concern-spool/canonical-edn unsigned)))))

(defn- read-cursor! [^Path directory]
  (let [path (.resolve directory cursor-name)]
    (when (Files/exists path no-follow)
      (when (or (Files/isSymbolicLink path)
                (not (Files/isRegularFile path no-follow)))
        (fail! "concern reconciliation cursor is unsafe"
               {:type :invalid-concern-reconciliation-cursor
                :path (str path)}))
      (let [size (Files/size path)]
        (when (> size marker-max-bytes)
          (fail! "concern reconciliation cursor exceeds its size bound"
                 {:type :invalid-concern-reconciliation-cursor
                  :bytes size
                  :max-bytes marker-max-bytes}))
        (let [text (decode-utf8! (Files/readAllBytes path))
              cursor (read-one-edn! text)
              expected
              #{:schema-version :last-file :commit :sha256}
              rebuilt (when (and (map? cursor)
                                 (= expected (set (keys cursor))))
                        (build-cursor (:last-file cursor)))]
          (when-not (and (= cursor-schema (:schema-version cursor))
                         (= cursor-commit (:commit cursor))
                         (= rebuilt cursor)
                         (= (str
                             (north.concern-spool/canonical-edn cursor)
                             "\n")
                            text))
            (fail! "concern reconciliation cursor is malformed"
                   {:type :invalid-concern-reconciliation-cursor
                    :path (str path)}))
          (set-permissions! path file-permissions)
          (fsync-directory! directory)
          cursor)))))

(defn- write-cursor! [^Path directory last-file]
  (let [cursor (build-cursor last-file)
        final-path (.resolve directory cursor-name)
        temp-path
        (.resolve
         directory
         (str "." cursor-name "." (java.util.UUID/randomUUID)
              temporary-suffix))
        payload
        (.getBytes
         (str (north.concern-spool/canonical-edn cursor) "\n")
         StandardCharsets/UTF_8)
        renamed? (atom false)]
    (try
      (write-file-fsynced! temp-path payload)
      (Files/move
       temp-path
       final-path
       (into-array
        CopyOption
        [StandardCopyOption/ATOMIC_MOVE
         StandardCopyOption/REPLACE_EXISTING]))
      (reset! renamed? true)
      (set-permissions! final-path file-permissions)
      (fsync-directory! directory)
      cursor
      (catch Throwable error
        (when-not @renamed?
          (try
            (when (Files/deleteIfExists temp-path)
              (fsync-directory! directory))
            (catch Throwable _ nil)))
        (throw error)))))

(defn- record-path [^Path directory operation-id record-type]
  (.resolve
   directory
   (str operation-id
        (if (= "settled" record-type) settled-suffix conflict-suffix))))

(defn- record-identity [operation]
  {:operation-id (:operation-id operation)
   :concern-id (or (:concern-id operation) "")
   :target-log (or (:target-log operation) "")
   :operation-sha256 (:sha256 operation)})

(defn- record-matches-operation? [record operation]
  (= (record-identity operation)
     (select-keys
      record
      [:operation-id :concern-id :target-log :operation-sha256])))

(defn- existing-record [^Path directory operation]
  (let [settled (record-path directory (:operation-id operation) "settled")
        conflict (record-path directory (:operation-id operation) "conflict")
        settled? (Files/exists settled no-follow)
        conflict? (Files/exists conflict no-follow)]
    (when (and settled? conflict?)
      (fail! "operation has both settled and conflict records"
             {:type :invalid-concern-reconciliation-state
              :operation-id (:operation-id operation)}))
    (when (or settled? conflict?)
      (let [path (if settled? settled conflict)
            _ (when (or (Files/isSymbolicLink path)
                        (not (Files/isRegularFile path no-follow)))
                (fail! "concern reconciliation marker is unsafe"
                       {:type :invalid-concern-reconciliation-entry
                        :path (str path)}))
            record (read-record! path)]
        (when-not (record-matches-operation? record operation)
          (fail! "concern reconciliation marker does not bind the operation"
                 {:type :invalid-concern-reconciliation-state
                  :operation-id (:operation-id operation)
                  :path (str path)}))
        ;; A process may have died after the atomic marker rename and before its
        ;; directory fsync. Revalidating the exact record and syncing the
        ;; directory here safely completes that durability step before skip.
        (set-permissions! path file-permissions)
        (fsync-directory! directory)
        record))))

(defn- publish-record! [^Path directory record]
  (let [record (validate-record! record)
        final-path
        (record-path directory (:operation-id record) (:record-type record))
        other-path
        (record-path
         directory
         (:operation-id record)
         (if (= "settled" (:record-type record)) "conflict" "settled"))
        temp-path
        (.resolve
         directory
         (str "." (.getFileName final-path) "."
              (java.util.UUID/randomUUID) temporary-suffix))
        payload
        (.getBytes
         (str (north.concern-spool/canonical-edn record) "\n")
         StandardCharsets/UTF_8)
        renamed? (atom false)]
    (when (Files/exists other-path no-follow)
      (fail! "operation already has the opposite immutable reconciliation result"
             {:type :invalid-concern-reconciliation-state
              :operation-id (:operation-id record)
              :path (str other-path)}))
    (if (Files/exists final-path no-follow)
      (let [existing (read-record! final-path)]
        (when-not (= existing record)
          (fail! "immutable concern reconciliation record already differs"
                 {:type :invalid-concern-reconciliation-state
                  :operation-id (:operation-id record)
                  :path (str final-path)}))
        existing)
      (try
        (write-file-fsynced! temp-path payload)
        (*reconcile-stage!* :record-file-fsynced {:record record :path temp-path})
        (try
          (Files/move
           temp-path
           final-path
           (into-array CopyOption [StandardCopyOption/ATOMIC_MOVE]))
          (catch Exception error
            (fail! "concern reconciliation atomic rename failed"
                   {:type :concern-reconciliation-atomic-move-failed
                    :cause (.getMessage error)})))
        (reset! renamed? true)
        (set-permissions! final-path file-permissions)
        (*reconcile-stage!* :record-renamed
                            {:record record :path final-path})
        (fsync-directory! directory)
        (*reconcile-stage!* :record-directory-fsynced
                            {:record record :path final-path})
        record
        (catch Throwable error
          (when-not @renamed?
            (try
              (when (Files/deleteIfExists temp-path)
                (fsync-directory! directory))
              (catch Throwable _ nil)))
          (throw error))))))

(defn- operation-order-key [^Path path]
  (try
    (let [operation (north.concern-spool/read-operation-file! path)]
      [0 (Instant/parse (:created-at operation)) (:operation-id operation)])
    (catch Throwable _
      [1 "" (str (.getFileName path))])))

(defn- operation-files [^Path spool-directory deadline-ns]
  (if-not (ensure-spool-directory! spool-directory)
    []
    (with-open [stream (Files/newDirectoryStream spool-directory)]
      (loop [remaining (seq stream)
             entry-count 0
             operations []]
        (when-not (< (System/nanoTime) deadline-ns)
          (deferred!
           "concern reconciliation spool scan deadline exceeded"
           {:entries entry-count}))
        (if-not remaining
          (->> operations
               (mapv
                (fn [^Path path]
                  (when-not (< (System/nanoTime) deadline-ns)
                    (deferred!
                     "concern reconciliation spool ordering deadline exceeded"
                     {:entries entry-count}))
                  [path (operation-order-key path)]))
               (sort-by second)
               (mapv first))
          (let [path ^Path (first remaining)
                next-count (inc entry-count)
                operation?
                (re-matches
                 operation-name-pattern
                 (str (.getFileName path)))]
            (when (> next-count maximum-spool-entries)
              (fail! "concern reconciliation spool entry bound exceeded"
                     {:type :concern-reconciliation-spool-entry-bound
                      :entries next-count
                      :maximum maximum-spool-entries}))
            (when
             (and operation?
                  (or (Files/isSymbolicLink path)
                      (not (Files/isRegularFile path no-follow))))
              (fail! "concern spool contains an unsafe operation entry"
                     {:type :invalid-concern-spool-entry
                      :path (str path)}))
            (recur
             (next remaining)
             next-count
             (cond-> operations operation? (conj path)))))))))

(defn- rotate-after [files last-file]
  (if-not last-file
    files
    (let [index
          (first
           (keep-indexed
            (fn [position ^Path path]
              (when (= last-file (str (.getFileName path))) position))
            files))]
      (if (nil? index)
        files
        (vec
         (concat (subvec files (inc index))
                 (subvec files 0 (inc index))))))))

(defn- operation-id-from-path [^Path path]
  (second
   (re-matches operation-name-pattern (str (.getFileName path)))))

(defn- wrong-log-response? [response]
  (and (map? response) (= :log-mismatch (:code response))))

(defn- explicit-rejection? [response]
  (and (map? response)
       (or (contains? response :reject)
           (contains? response :error))))

(defn- response-conflict! [response]
  (if (wrong-log-response? response)
    (conflict!
     "wrong-log"
     {:response response
      :observed-version (:version response)})
    (conflict!
     "coordinator-rejected-read"
     {:response response
      :observed-version (:version response)})))

(defn- exact-version! [port target-log]
  (let [response
        (north.coord/send-op-for-log port target-log {:op :version})]
    (cond
      (and (map? response)
           (= #{:version} (set (keys response)))
           (integer? (:version response))
           (not (neg? (:version response))))
      (:version response)

      (explicit-rejection? response)
      (response-conflict! response)

      :else
      (deferred!
       "coordinator returned a malformed version response"
       {:response response}))))

(defn- exact-show! [port target-log subject]
  (let [response
        (north.coord/send-op-for-log
         port target-log {:op :show :te subject})]
    (cond
      (and (map? response)
           (= #{:version :rows} (set (keys response)))
           (integer? (:version response))
           (not (neg? (:version response)))
           (vector? (:rows response))
           (every?
            (fn [row]
              (and (vector? row)
                   (= 2 (count row))
                   (every? string? row)))
            (:rows response)))
      response

      (explicit-rejection? response)
      (response-conflict! response)

      :else
      (deferred!
       "coordinator returned a malformed show response"
       {:subject subject
        :response response}))))

(defn- about-shape-valid? [rows]
  (let [kinds (mapv second (filter #(= "kind" (first %)) rows))
        titles (mapv second (filter #(= "title" (first %)) rows))]
    (and (= ["thread"] kinds)
         (= 1 (count titles))
         (not (str/blank? (first titles))))))

(defn- exact-about-binding!
  [port target-log subject binding-cid]
  (if-not binding-cid
    {:binding-valid false
     :binding-unproven true}
    (let [response
          (north.coord/send-op-for-log
           port
           target-log
           {:op :claim-read
            :cid binding-cid
            :te subject
            :p "kind"})]
      (cond
        (and (map? response)
             (true? (:ok response))
             (= binding-cid (:claim-cid response))
             (= "thread" (:claim response))
             (integer? (:version response)))
        {:binding-valid true
         :version (:version response)}

        (wrong-log-response? response)
        (response-conflict! response)

        (and (explicit-rejection? response)
             (integer? (:version response)))
        {:binding-valid false
         :version (:version response)
         :response response}

        :else
        (deferred!
         "coordinator returned a malformed about-binding response"
         {:subject subject
          :binding-cid binding-cid
          :response response})))))

(defn- expected-rows [operation]
  (mapv
   (fn [{:keys [predicate object]}] [predicate object])
   (:facts operation)))

(def ^:private concern-fact-cardinalities
  {"title" "single"
   "agent" "single"
   "driver" "single"
   "repo" "single"
   "intent" "single"
   "about" "single"
   "code_port" "single"
   "code_log" "single"
   "touches" "multi"
   "reached" "multi"
   "kind" "single"})

(def ^:private required-single-concern-predicates
  #{"title" "agent" "driver" "repo" "intent" "kind"})

(defn- exact-ref? [value]
  (and (string? value)
       (re-matches #"@[A-Za-z0-9][A-Za-z0-9._:-]*" value)))

(defn- validate-declaration-operation! [operation]
  (let [facts (:facts operation)
        grouped (group-by :predicate facts)
        values (fn [predicate] (mapv :object (get grouped predicate [])))
        fact-pairs (mapv (juxt :predicate :object) facts)
        about (get-in operation [:precondition :about :subject])
        agent (first (values "agent"))
        driver (first (values "driver"))
        repo (first (values "repo"))
        intent (first (values "intent"))
        code-port (first (values "code_port"))
        code-log (first (values "code_log"))]
    (when-not
     (every?
      (fn [{:keys [predicate cardinality]}]
        (= cardinality (get concern-fact-cardinalities predicate)))
      facts)
      (fail! "offline concern operation has an unsupported fact or cardinality"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (= (count fact-pairs) (count (set fact-pairs)))
      (fail! "offline concern operation repeats an identical fact"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (every? #(not (str/blank? (:object %))) facts)
      (fail! "offline concern operation contains a blank fact value"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not
     (every?
      #(= 1 (count (get grouped % [])))
      required-single-concern-predicates)
      (fail! "offline concern operation lacks one complete concern spine"
             {:type :invalid-concern-operation
              :field :facts}))
    (when
     (some
      (fn [[predicate entries]]
        (and (= "single" (get concern-fact-cardinalities predicate))
             (> (count entries) 1)))
      grouped)
      (fail! "offline concern operation conflicts within a single-valued field"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (= ["building"] (values "reached"))
      (fail! "offline concern declaration must begin at reached=building"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (= ["concern"] (values "kind"))
      (fail! "offline concern operation has an invalid terminal kind"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (and (exact-ref? agent) (= agent driver))
      (fail! "offline concern agent and driver binding is invalid"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (= [(str "[" repo "] " intent)] (values "title"))
      (fail! "offline concern title does not bind its repo and intent"
             {:type :invalid-concern-operation
              :field :facts}))
    (when-not (= (if about [about] []) (values "about"))
      (fail! "offline concern about fact and precondition differ"
             {:type :invalid-concern-operation
              :field :precondition}))
    (when-not (= (boolean (seq (values "code_port")))
                 (boolean (seq (values "code_log"))))
      (fail! "offline concern code target must bind both port and log"
             {:type :invalid-concern-operation
              :field :facts}))
    (when code-port
      (let [port (when (re-matches #"[0-9]+" code-port)
                   (parse-long code-port))]
        (when-not (and port (<= 1 port 65535))
          (fail! "offline concern code target port is invalid"
                 {:type :invalid-concern-operation
                  :field :facts}))))
    (when code-log
      (when-not
       (try
         (= code-log (north.coord/canonical-log-path code-log))
         (catch Exception _ false))
        (fail! "offline concern code target log is not canonical"
               {:type :invalid-concern-operation
                :field :facts})))
    operation))

(defn- validate-transition-operation! [operation]
  (let [facts (:facts operation)
        fact (first facts)]
    (when-not
     (and (= north.concern-spool/transition-operation-type
             (:operation-type operation))
          (= 1 (count facts))
          (= "reached" (:predicate fact))
          (= "multi" (:cardinality fact))
          (contains? north.concern-spool/transition-statuses (:object fact))
          (= "concern-transition-or-exact"
             (get-in operation [:precondition :mode]))
          (= (:concern-id operation)
             (get-in operation [:precondition :subject]))
          (= (:facts-sha256 operation)
             (get-in operation [:precondition :projection-sha256]))
          (nil? (get-in operation [:precondition :about])))
      (fail! "offline concern transition has an unsupported shape"
             {:type :invalid-concern-operation
              :field :facts}))
    operation))

(defn- validate-concern-operation! [operation]
  (case (:operation-type operation)
    "concern-declare" (validate-declaration-operation! operation)
    "concern-transition" (validate-transition-operation! operation)
    (fail! "offline concern operation type is unsupported"
           {:type :invalid-concern-operation
            :field :operation-type})))

(defn- exact-projection? [operation rows]
  (= (frequencies (expected-rows operation))
     (frequencies rows)))

(defn- read-snapshot-at-base [port operation]
  (let [target-log (:target-log operation)
        base (exact-version! port target-log)
        subject-view
        (exact-show! port target-log (:concern-id operation))
        about-subject (get-in operation [:precondition :about :subject])
        about-view
        (when about-subject
          (exact-show! port target-log about-subject))
        about-binding
        (when about-subject
          (exact-about-binding!
           port
           target-log
           about-subject
           (get-in operation
                   [:precondition :about :binding-cid])))]
    (if (or (not= base (:version subject-view))
            (and about-view (not= base (:version about-view)))
            (and (:version about-binding)
                 (not= base (:version about-binding))))
      {:reject :conflict}
      {:base base
       :rows (:rows subject-view)
       :about-subject about-subject
       :about-rows (:rows about-view)
       :about-binding about-binding})))

(defn- precondition-conflict [snapshot]
  (when (:about-subject snapshot)
    (cond
      (get-in snapshot [:about-binding :binding-unproven])
      {:local-conflict true
       :reason "about-binding-unproven"
       :observed-version (:base snapshot)
       :rows (:rows snapshot)}

      (or (not (get-in snapshot [:about-binding :binding-valid]))
          (not (about-shape-valid? (:about-rows snapshot))))
      {:local-conflict true
       :reason "about-binding-changed"
       :observed-version (:base snapshot)
       :rows (:rows snapshot)})))

(defn- valid-commit-ack? [response fact-count]
  (and (map? response)
       (= #{:ok :written :idempotent :batch} (set (keys response)))
       (integer? (:ok response))
       (not (neg? (:ok response)))
       (vector? (:written response))
       (every? string? (:written response))
       (vector? (:idempotent response))
       (every? string? (:idempotent response))
       (= fact-count
          (+ (count (:written response))
             (count (:idempotent response))))
       (true? (:batch response))))

(defn- declaration-commit-attempt! [port operation]
  (let [snapshot (read-snapshot-at-base port operation)]
    (if (= :conflict (:reject snapshot))
      snapshot
      (or
       (precondition-conflict snapshot)
       (cond
         (exact-projection? operation (:rows snapshot))
         {:done :identical
          :observed-version (:base snapshot)
          :rows (:rows snapshot)}

         (seq (:rows snapshot))
         {:local-conflict true
          :reason "projection-differs"
          :observed-version (:base snapshot)
          :rows (:rows snapshot)}

         :else
         (do
           (*reconcile-stage!* :pre-commit
                               {:operation operation
                                :base (:base snapshot)})
           (let [response
                 (north.coord/send-op-for-log
                  port
                  (:target-log operation)
                  {:op :assert-batch-at-version
                   :te (:concern-id operation)
                   :facts
                   (mapv
                    (fn [{:keys [predicate object]}]
                      {:p predicate :r object})
                    (:facts operation))
                   :base (:base snapshot)})]
             (cond
               (= :conflict (:reject response))
               response

               (valid-commit-ack? response (count (:facts operation)))
               (do
                 (*reconcile-stage!* :post-commit-pre-ack
                                     {:operation operation
                                      :response response})
                 {:done :committed
                  :observed-version (:ok response)
                  :ack response})

               (wrong-log-response? response)
               (response-conflict! response)

               (explicit-rejection? response)
               {:local-conflict true
                :reason "atomic-commit-rejected"
                :observed-version (or (:version response) (:base snapshot))
                :rows (:rows snapshot)}

               :else
               (deferred!
                "coordinator acknowledgement for reconciled concern is ambiguous"
                {:response response})))))))))

(defn- transition-snapshot-at-base [port operation]
  (let [base (exact-version! port (:target-log operation))
        subject-view
        (exact-show! port (:target-log operation) (:concern-id operation))]
    (if (not= base (:version subject-view))
      {:reject :conflict}
      {:base base :rows (:rows subject-view)})))

(defn- transition-present? [operation rows]
  (contains?
   (set rows)
   ["reached" (get-in operation [:facts 0 :object])]))

(defn- transition-commit-attempt! [port operation]
  (let [snapshot (transition-snapshot-at-base port operation)]
    (if (= :conflict (:reject snapshot))
      snapshot
      (if (transition-present? operation (:rows snapshot))
        {:done :identical
         :observed-version (:base snapshot)
         :rows (:rows snapshot)}
        (if-not *transition-plan!*
          (deferred!
           "concern transition reconciliation requires its CLI planner"
           {:operation-id (:operation-id operation)})
          (let [plan (*transition-plan!* port operation snapshot)]
            (cond
              (:local-conflict plan) plan

              (:done plan)
              {:done (:done plan)
               :observed-version (:base snapshot)
               :rows (:rows snapshot)}

              :else
              (let [facts (:facts plan)]
                (when-not (and (vector? facts) (seq facts))
                  (deferred!
                   "concern transition planner returned no canonical batch"
                   {:operation-id (:operation-id operation)}))
                (*reconcile-stage!* :pre-commit
                                    {:operation operation
                                     :base (:base snapshot)})
                (let [response
                      (north.coord/send-op-for-log
                       port
                       (:target-log operation)
                       {:op :assert-batch-at-version
                        :te (:concern-id operation)
                        :facts facts
                        :base (:base snapshot)})]
                  (cond
                    (= :conflict (:reject response)) response

                    (valid-commit-ack? response (count facts))
                    (do
                      (*reconcile-stage!*
                       :post-commit-pre-ack
                       {:operation operation :response response})
                      {:done :committed
                       :observed-version (:ok response)
                       :ack response})

                    (wrong-log-response? response)
                    (response-conflict! response)

                    (explicit-rejection? response)
                    {:local-conflict true
                     :reason "atomic-transition-rejected"
                     :observed-version
                     (or (:version response) (:base snapshot))
                     :rows (:rows snapshot)}

                    :else
                    (deferred!
                     "coordinator acknowledgement for reconciled concern transition is ambiguous"
                     {:response response})))))))))))

(defn- commit-attempt! [port operation]
  (case (:operation-type operation)
    "concern-declare" (declaration-commit-attempt! port operation)
    "concern-transition" (transition-commit-attempt! port operation)))

(defn- declaration-readback! [port operation deadline-ns]
  (let [snapshot
        (north.coord/retry-conflicts-until!
         deadline-ns
         16
         #(read-snapshot-at-base port operation))]
    (cond
      (= :deadline (:reject snapshot))
      (deferred!
       "concern reconciliation readback deadline exceeded"
       {:operation-id (:operation-id operation)})

      (= :conflict (:reject snapshot))
      (deferred!
       "concern reconciliation readback did not stabilize"
       {:operation-id (:operation-id operation)})

      (precondition-conflict snapshot)
      (precondition-conflict snapshot)

      (not (exact-projection? operation (:rows snapshot)))
      {:local-conflict true
       :reason "readback-projection-differs"
       :observed-version (:base snapshot)
       :rows (:rows snapshot)}

      :else snapshot)))

(defn- transition-readback! [port operation deadline-ns]
  (let [snapshot
        (north.coord/retry-conflicts-until!
         deadline-ns
         16
         #(transition-snapshot-at-base port operation))]
    (cond
      (= :deadline (:reject snapshot))
      (deferred!
       "concern transition reconciliation readback deadline exceeded"
       {:operation-id (:operation-id operation)})

      (= :conflict (:reject snapshot))
      (deferred!
       "concern transition reconciliation readback did not stabilize"
       {:operation-id (:operation-id operation)})

      (not (transition-present? operation (:rows snapshot)))
      {:local-conflict true
       :reason "readback-transition-missing"
       :observed-version (:base snapshot)
       :rows (:rows snapshot)}

      :else snapshot)))

(defn- exact-readback! [port operation deadline-ns]
  (case (:operation-type operation)
    "concern-declare" (declaration-readback! port operation deadline-ns)
    "concern-transition" (transition-readback! port operation deadline-ns)))

(defn- reconcile-operation! [port ^Path state-dir operation deadline-ns]
  (if-let [record (existing-record state-dir operation)]
    {:status (if (= "settled" (:record-type record))
               :already-settled
               :already-conflict)
     :record record}
    (let [attempt
          (north.coord/retry-conflicts-until!
           deadline-ns
           16
           #(commit-attempt! port operation))]
      (cond
        (or (= :deadline (:reject attempt))
            (= :conflict (:reject attempt)))
        {:status :deferred
         :reason "commit-conflict-deadline"}

        (:local-conflict attempt)
        (let [record
              (build-record
               (merge
                (record-identity operation)
                {:record-type "conflict"
                 :reason (:reason attempt)
                 :observed-version (:observed-version attempt)
                 :observed-projection-sha256
                 (projection-sha256 (:rows attempt))}))]
          (publish-record! state-dir record)
          {:status :conflict :record record})

        (:done attempt)
        (let [readback (exact-readback! port operation deadline-ns)]
          (if (:local-conflict readback)
            (let [record
                  (build-record
                   (merge
                    (record-identity operation)
                    {:record-type "conflict"
                     :reason (:reason readback)
                     :observed-version (:observed-version readback)
                     :observed-projection-sha256
                     (projection-sha256 (:rows readback))}))]
              (publish-record! state-dir record)
              {:status :conflict :record record})
            (do
              (*reconcile-stage!* :post-readback-pre-settlement
                                  {:operation operation
                                   :snapshot readback})
              (let [record
                    (build-record
                     (merge
                      (record-identity operation)
                      {:record-type "settled"
                       :reason
                       (if (= :identical (:done attempt))
                         "identical-preexisting"
                         "committed")
                       :observed-version (:base readback)
                       :observed-projection-sha256
                       (projection-sha256 (:rows readback))}))]
                (publish-record! state-dir record)
                {:status :settled :record record}))))

        :else
        (deferred!
         "concern reconciliation reached an ambiguous local state"
         {:operation-id (:operation-id operation)
          :attempt attempt})))))

(defn- invalid-operation! [^Path state-dir ^Path operation-path error]
  (let [operation
        {:operation-id (operation-id-from-path operation-path)
         :concern-id ""
         :target-log ""
         :sha256 (raw-file-sha256 operation-path)}]
    (if-let [record (existing-record state-dir operation)]
      {:status (if (= "conflict" (:record-type record))
                 :already-conflict
                 :already-settled)
       :record record}
      (let [record
            (build-record
             (merge
              (record-identity operation)
              {:record-type "conflict"
               :reason "invalid-operation"
               :observed-version -1
               :observed-projection-sha256 ""}))]
        (publish-record! state-dir record)
        {:status :conflict
         :record record
         :error (.getMessage error)}))))

(defn- process-operation! [port state-dir operation-path deadline-ns]
  (try
    (let [operation
          (validate-concern-operation!
           (north.concern-spool/read-operation-file! operation-path))]
      (if-not (= (:operation-id operation)
                 (operation-id-from-path operation-path))
        (invalid-operation!
         state-dir
         operation-path
         (ex-info
          "operation filename does not match its canonical operation id"
          {:type :invalid-concern-operation
           :field :operation-id}))
        (reconcile-operation! port state-dir operation deadline-ns)))
    (catch clojure.lang.ExceptionInfo error
      (let [{:keys [type reason observed-version]} (ex-data error)]
        (cond
          (= :concern-reconciliation-conflict type)
          (let [operation
                (try
                  (north.concern-spool/read-operation-file! operation-path)
                  (catch Throwable _ nil))]
            (if operation
              (let [record
                    (build-record
                     (merge
                      (record-identity operation)
                      {:record-type "conflict"
                       :reason reason
                       :observed-version observed-version
                       :observed-projection-sha256 ""}))]
                (publish-record! state-dir record)
                {:status :conflict :record record})
              (invalid-operation! state-dir operation-path error)))

          (#{:invalid-concern-operation
             :concern-spool-record-too-large}
           type)
          (invalid-operation! state-dir operation-path error)

          :else
          {:status :deferred
           :reason (or (some-> type name) "transport-or-local-failure")
           :error (.getMessage error)})))
    (catch java.io.IOException error
      {:status :deferred
       :reason "transport-or-filesystem-failure"
       :error (.getMessage error)})))

(defn- summarize-outcomes [outcomes selected-count total-count bytes elapsed-ms]
  (let [statuses (frequencies (map :status outcomes))]
    {:status :complete
     :selected selected-count
     :processed (count outcomes)
     :bytes bytes
     :settled (get statuses :settled 0)
     :conflicts (get statuses :conflict 0)
     :already-settled (get statuses :already-settled 0)
     :already-conflict (get statuses :already-conflict 0)
     :deferred (get statuses :deferred 0)
     :remaining (- total-count (count outcomes))
     :elapsed-ms elapsed-ms
     :outcomes outcomes}))

(defn reconcile-pass!
  "Run one deterministic, single-process, bounded reconciliation pass.

   The pending operation is never removed. A valid immutable settlement or
   conflict marker makes later passes idempotent without weakening the spool's
   sole durable copy."
  ([port]
   (reconcile-pass!
    port
    {:spool-directory (north.concern-spool/state-directory)
     :state-directory (state-directory)
     :limits (limits)}))
  ([port {:keys [spool-directory state-directory limits]}]
   (let [spool-directory (->path spool-directory)
         state-directory (ensure-directory! (->path state-directory))
         {:keys [max-items max-bytes max-millis]}
         (validate-pass-limits! limits)
         start-ns (System/nanoTime)
         deadline-ns (+ start-ns (* 1000000 (long max-millis)))
         lock-path (.resolve state-directory lock-name)]
     (with-open [channel (open-lock-channel lock-path)]
       (if-not (try-lock channel)
         {:status :busy
          :selected 0
          :processed 0
          :bytes 0
          :settled 0
          :conflicts 0
          :deferred 0
          :remaining 0
          :elapsed-ms (quot (- (System/nanoTime) start-ns) 1000000)
          :outcomes []}
         (do
           (*reconcile-stage!* :lock-acquired
                               {:spool-directory spool-directory
                                :state-directory state-directory})
           (let [all-files (operation-files spool-directory deadline-ns)
                 cursor (read-cursor! state-directory)
                 ordered-files (rotate-after all-files (:last-file cursor))
                 selected
                 (loop [remaining ordered-files
                        picked []
                        bytes 0]
                   (if (or (empty? remaining)
                           (>= (count picked) max-items)
                           (not (< (System/nanoTime) deadline-ns)))
                     {:files picked :bytes bytes}
                     (let [path (first remaining)
                           size (Files/size path)
                           fits? (<= (+ bytes size) max-bytes)]
                       (recur
                        (rest remaining)
                        (conj
                         picked
                         {:path path
                          :size size
                          :over-byte-budget (not fits?)})
                        (if fits? (+ bytes size) bytes)))))
                 files (:files selected)
                 outcomes
                 (binding [north.coord/*request-deadline-ns* deadline-ns]
                   (loop [remaining files
                          results []]
                     (if (or (empty? remaining)
                             (not (< (System/nanoTime) deadline-ns)))
                       results
                       (let [{:keys [path size over-byte-budget]}
                             (first remaining)
                             result
                             (if over-byte-budget
                               {:status :deferred
                                :reason
                                "operation-exceeds-remaining-pass-byte-budget"
                                :bytes size}
                               (process-operation!
                                port state-directory path deadline-ns))]
                         (recur
                          (rest remaining)
                          (conj
                           results
                           (assoc
                            result
                            :file (str (.getFileName ^Path path)))))))))]
             (when-let [last-file (:file (peek outcomes))]
               (write-cursor! state-directory last-file))
             (summarize-outcomes
              outcomes
              (count files)
              (count all-files)
              (:bytes selected)
              (quot (- (System/nanoTime) start-ns) 1000000)))))))))
