(ns north.harness-state
  "Provider-neutral key/value posture state. The Claude-era file is a
   read-only migration fallback: the first North write seeds the canonical
   file from it, after which only the canonical file participates."
  (:require [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.nio ByteBuffer]
           [java.nio.channels FileChannel]
           [java.nio.charset StandardCharsets]
           [java.nio.file FileAlreadyExistsException Files LinkOption OpenOption
            StandardCopyOption StandardOpenOption]
           [java.nio.file.attribute FileAttribute PosixFilePermissions]))

(defonce ^:private in-process-lock (Object.))

(def ^:private file-permissions
  (PosixFilePermissions/fromString "rw-------"))

(def ^:private directory-permissions
  (PosixFilePermissions/fromString "rwx------"))

(defn canonical-path [home]
  (or (System/getenv "NORTH_HARNESS_STATE")
      (str home "/.local/state/north/harness.conf")))

(defn legacy-path [home]
  (or (System/getenv "NORTH_LEGACY_HARNESS_STATE")
      (str home "/.claude/my-config.state")))

(defn lock-path [home]
  (str (canonical-path home) ".lock"))

(defn- slurp-safe [path]
  (try (slurp path) (catch Exception _ nil)))

(defn source-path
  "Canonical state wins. Legacy is consulted only while canonical is absent."
  [home]
  (let [canonical (canonical-path home)
        legacy (legacy-path home)]
    (cond
      (.isFile (io/file canonical)) canonical
      (.isFile (io/file legacy)) legacy
      :else canonical)))

(defn content [home]
  (or (slurp-safe (source-path home)) ""))

(defn- content-for-write [home]
  (let [path (source-path home)]
    (if (.isFile (io/file path))
      ;; A write must never reinterpret an unreadable current state as empty;
      ;; that would turn a read failure into silent key loss.
      (slurp path)
      "")))

(defn get-value [home key default]
  (let [prefix (str key "=")]
    (or (some->> (str/split-lines (content home))
                 (filter #(str/starts-with? % prefix))
                 last
                 (#(subs % (count prefix))))
        default)))

(defn- permission-attribute [permissions]
  (into-array FileAttribute
              [(PosixFilePermissions/asFileAttribute permissions)]))

(defn- owner-only! [path permissions directory?]
  (try
    (Files/setPosixFilePermissions path permissions)
    (catch UnsupportedOperationException _
      (let [file (.toFile path)
            changed? (and (.setReadable file false false)
                          (.setWritable file false false)
                          (.setExecutable file false false)
                          (.setReadable file true true)
                          (.setWritable file true true)
                          (or (not directory?) (.setExecutable file true true)))]
        (when-not changed?
          (throw (ex-info "could not secure harness state path"
                          {:path (str path)}))))))
  path)

(defn- ensure-private-directory! [dir secure-existing?]
  (let [existed? (Files/exists dir (make-array LinkOption 0))]
    (Files/createDirectories dir (permission-attribute directory-permissions))
    ;; A caller-supplied state file may live in an existing shared directory
    ;; such as /tmp. Secure directories we create and North's canonical state
    ;; directory, but never chmod an arbitrary existing parent.
    (when (or secure-existing? (not existed?))
      (owner-only! dir directory-permissions true)))
  dir)

(defn- ensure-lock-file! [path]
  (try
    (Files/createFile path (permission-attribute file-permissions))
    (catch FileAlreadyExistsException _))
  (when (or (Files/isSymbolicLink path)
            (not (Files/isRegularFile path (into-array LinkOption [LinkOption/NOFOLLOW_LINKS]))))
    (throw (ex-info "harness state lock must be a regular file"
                    {:path (str path)})))
  (owner-only! path file-permissions false))

(defn- write-durable! [path value]
  (with-open [channel (FileChannel/open
                       path
                       (into-array OpenOption
                                   [StandardOpenOption/WRITE
                                    StandardOpenOption/TRUNCATE_EXISTING]))]
    (let [buffer (ByteBuffer/wrap (.getBytes (str value) StandardCharsets/UTF_8))]
      (while (.hasRemaining buffer) (.write channel buffer))
      (.force channel true))))

(defn- atomic-spit! [path value]
  (let [dest (.toAbsolutePath (.normalize (.toPath (io/file path))))
        dir (.getParent dest)
        _ (ensure-private-directory! dir false)
        tmp (Files/createTempFile
             dir ".harness." ".tmp"
             (permission-attribute file-permissions))]
    (try
      (write-durable! tmp value)
      (Files/move
       tmp dest
       (into-array java.nio.file.CopyOption
                   [StandardCopyOption/ATOMIC_MOVE
                    StandardCopyOption/REPLACE_EXISTING]))
      (owner-only! dest file-permissions false)
      (finally (Files/deleteIfExists tmp)))))

(defn- with-state-lock [home f]
  ;; The lock has its own stable inode. Replacing harness.conf therefore never
  ;; releases or bypasses the mutual-exclusion point for a waiting writer.
  (locking in-process-lock
    (let [path (.toAbsolutePath (.normalize (.toPath (io/file (lock-path home)))))
          dir (.getParent path)
          default-dir (.getParent
                       (.toAbsolutePath
                        (.normalize
                         (.toPath (io/file home ".local/state/north/harness.conf")))))]
      (ensure-private-directory! dir (= dir default-dir))
      (ensure-lock-file! path)
      (with-open [channel (FileChannel/open
                           path
                           (into-array OpenOption
                                       [StandardOpenOption/WRITE
                                        LinkOption/NOFOLLOW_LINKS]))]
        ;; Closing the channel releases its lock. Babashka intentionally does
        ;; not expose methods on the JDK's private FileLock implementation.
        (let [_held (.lock channel)]
          (f))))))

(defn put-value!
  "Write canonical state. When only legacy state exists, preserve all of its
   lines in the canonical seed but never mutate the legacy file."
  [home key value]
  (when-not (re-matches #"[A-Za-z0-9_.-]+" (or key ""))
    (throw (ex-info "invalid harness state key" {:key key})))
  (when (or (nil? value) (str/includes? (str value) "\n") (str/includes? (str value) "\r"))
    (throw (ex-info "invalid harness state value" {:key key})))
  (let [canonical (canonical-path home)]
    (with-state-lock
      home
      (fn []
        ;; Read after acquiring the persistent lock. This makes the whole
        ;; read-modify-replace sequence one cross-process transaction.
        (let [source (content-for-write home)
              prefix (str key "=")
              lines (if (str/blank? source) [] (str/split-lines source))
              kept (remove #(str/starts-with? % prefix) lines)
              next-content (str (str/join "\n" (concat kept [(str key "=" value)])) "\n")]
          (atomic-spit! canonical next-content))))
    canonical))
