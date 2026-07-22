(ns north.corpus-transaction
  "Journaled, all-old/all-new replacement for North's split fact corpus.

  Callers seal exact live and candidate file records plus bounded tagged
  artifact provenance into a plan, then provide coordinator lifecycle
  callbacks. The core owns the state singleton, writer fences, durable
  preimage, journal, replacement, and crash recovery."
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [fram.rt :as rt]))

(def plan-format "north-corpus-transaction-plan/v2")
(def journal-format "north-corpus-transaction-journal/v2")
(def receipt-format "north-corpus-transaction-receipt/v2")
(def preimage-format "north-corpus-preimage/v2")
(def roles [:coordination :telemetry])
(def plan-id-pattern #"plan-[0-9a-f]{64}")
(def provenance-max-bytes 1024)
(def schema-candidate-provenance-format
  "north-corpus-provenance/schema-candidate-v1")
(def snapshot-restore-provenance-format
  "north-corpus-provenance/snapshot-restore-v1")
(def sha256-pattern #"[0-9a-f]{64}")
(def snapshot-id-pattern #"snapshot-[0-9a-f]{64}")
(def finalized-candidate-id-pattern #"schema-candidate-[0-9a-f]{64}")
(def restore-candidate-id-pattern #"candidate-[0-9a-f]{64}")
(def schema-receipt-id-pattern #"schema-converged-[0-9a-f]{64}\.edn")

(defn fail!
  ([message] (fail! message {}))
  ([message data]
   (throw (ex-info message (merge {:north.corpus-transaction/error true} data)))))

(defn now-iso [] (.toString (java.time.Instant/now)))

(defn- home []
  (or (System/getenv "HOME") (System/getProperty "user.home")))

(defn state-root []
  (let [base (or (System/getenv "XDG_STATE_HOME") (str (home) "/.local/state"))]
    (.getCanonicalPath
     (io/file (or (System/getenv "NORTH_CORPUS_TRANSACTION_DIR")
                  (str base "/north/corpus-transactions"))))))

(defn journal-path [] (str (state-root) "/active.edn"))
(defn maintenance-lock-path [] (str (state-root) "/maintenance.lock"))

(defn ensure-dir! [path]
  (let [f (io/file path)]
    (when-not (or (.isDirectory f) (.mkdirs f))
      (fail! (str "cannot create directory: " path)))
    (.getCanonicalPath f)))

(defn fsync-dir! [path]
  (with-open [channel (java.nio.channels.FileChannel/open
                       (.toPath (io/file path))
                       (into-array java.nio.file.OpenOption
                                   [java.nio.file.StandardOpenOption/READ]))]
    (.force channel true))
  nil)

(defn fsync-file! [path]
  (with-open [channel (java.nio.channels.FileChannel/open
                       (.toPath (io/file path))
                       (into-array java.nio.file.OpenOption
                                   [java.nio.file.StandardOpenOption/WRITE]))]
    (.force channel true))
  nil)

(defn- sha256-bytes [^bytes payload]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") payload)]
    (apply str (map #(format "%02x" %) digest))))

(defn sha256-text [value]
  (sha256-bytes (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn sha256-file [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")
        buffer (byte-array 65536)]
    (with-open [input (java.io.FileInputStream. (str path))]
      (loop []
        (let [n (.read input buffer)]
          (when (pos? n)
            (.update digest buffer 0 n)
            (recur)))))
    (apply str (map #(format "%02x" %) (.digest digest)))))

(defn prefix-sha256 [path byte-count]
  (let [f (io/file path)]
    (when (< (.length f) (long byte-count))
      (fail! (str "file is shorter than sealed prefix: " path)))
    (let [digest (java.security.MessageDigest/getInstance "SHA-256")
          buffer (byte-array 65536)]
      (with-open [input (java.io.FileInputStream. f)]
        (loop [remaining (long byte-count)]
          (when (pos? remaining)
            (let [want (int (min remaining (alength buffer)))
                  n (.read input buffer 0 want)]
              (when-not (pos? n)
                (fail! (str "unexpected EOF reading sealed prefix: " path)))
              (.update digest buffer 0 n)
              (recur (- remaining n))))))
      (apply str (map #(format "%02x" %) (.digest digest))))))

(defn- posix-permissions [path]
  (try
    (->> (java.nio.file.Files/getPosixFilePermissions
          (.toPath (io/file path))
          (make-array java.nio.file.LinkOption 0))
         (map str)
         sort
         vec)
    (catch UnsupportedOperationException _ nil)))

(defn- set-posix-permissions! [path permissions]
  (when (seq permissions)
    (java.nio.file.Files/setPosixFilePermissions
     (.toPath (io/file path))
     (java.util.HashSet.
      ^java.util.Collection
      (mapv java.nio.file.attribute.PosixFilePermission/valueOf permissions))))
  nil)

(defn canonical-regular-file! [label path]
  (when (str/blank? (str path))
    (fail! (str label " path is blank")))
  (let [f (io/file (str path))
        nio (.toPath f)]
    (when (java.nio.file.Files/isSymbolicLink nio)
      (fail! (str label " must not be a symbolic link: " path)))
    (when-not (.isFile f)
      (fail! (str label " is not a regular file: " path)))
    (.getCanonicalPath f)))

(defn file-record [label path]
  (let [canonical (canonical-regular-file! label path)
        f (io/file canonical)]
    (cond-> {:path canonical
             :bytes (.length f)
             :sha256 (sha256-file canonical)}
      (posix-permissions canonical)
      (assoc :permissions (posix-permissions canonical)))))

(def append-boundary "terminal-lf-or-empty")

(defn append-boundary-valid? [path]
  (let [f (io/file path)
        length (.length f)]
    (or (zero? length)
        (with-open [raf (java.io.RandomAccessFile. f "r")]
          (.seek raf (dec length))
          (= 10 (.read raf))))))

(defn verify-append-boundary! [label path]
  (when-not (append-boundary-valid? path)
    (fail! (str label " is nonempty but lacks a terminal LF: " path)
           {:path (.getCanonicalPath (io/file path))
            :append-boundary append-boundary}))
  path)

(defn corpus-file-record [label path]
  (let [record (file-record label path)]
    (verify-append-boundary! label (:path record))
    (assoc record :append-boundary append-boundary)))

(defn record-matches? [record]
  (try
    (let [actual (file-record "sealed corpus file" (:path record))]
      (and (= (:path record) (:path actual))
           (= (:bytes record) (:bytes actual))
           (= (:sha256 record) (:sha256 actual))))
    (catch Throwable _ false)))

(defn record-prefix-matches? [record live-path]
  (try
    (let [f (io/file live-path)]
      (and (.isFile f)
           (not (java.nio.file.Files/isSymbolicLink (.toPath f)))
           (>= (.length f) (long (:bytes record)))
           (= (:sha256 record) (prefix-sha256 live-path (:bytes record)))))
    (catch Throwable _ false)))

(defn verify-record! [label record]
  (when-not (and (map? record)
                 (string? (:path record))
                 (integer? (:bytes record))
                 (not (neg? (:bytes record)))
                 (re-matches #"[0-9a-f]{64}" (str (:sha256 record))))
    (fail! (str label " is not a complete sealed file record")))
  (when-not (record-matches? record)
    (fail! (str label " changed after it was sealed: " (:path record))
           {:record record}))
  record)

(defn verify-corpus-record! [label record]
  (verify-record! label record)
  (when-not (= append-boundary (:append-boundary record))
    (fail! (str label " lacks a sealed append-boundary invariant")))
  (verify-append-boundary! label (:path record))
  record)

(defn canonical-form [value]
  (cond
    (map? value)
    (into (sorted-map-by #(compare (str %1) (str %2)))
          (map (fn [[k v]] [k (canonical-form v)]) value))
    (set? value) (mapv canonical-form (sort-by str value))
    (sequential? value) (mapv canonical-form value)
    :else value))

(defn canonical-edn-bytes [value]
  (.getBytes (pr-str (canonical-form value))
             java.nio.charset.StandardCharsets/UTF_8))

(defn exact-keys? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn- valid-source-snapshot? [source]
  (and (exact-keys? source #{:snapshot_id :manifest_sha256})
       (string? (:snapshot_id source))
       (re-matches snapshot-id-pattern (:snapshot_id source))
       (string? (:manifest_sha256 source))
       (re-matches sha256-pattern (:manifest_sha256 source))))

(defn- valid-candidate? [candidate id-pattern]
  (and (exact-keys? candidate #{:candidate_id :manifest_sha256})
       (string? (:candidate_id candidate))
       (re-matches id-pattern (:candidate_id candidate))
       (string? (:manifest_sha256 candidate))
       (re-matches sha256-pattern (:manifest_sha256 candidate))))

(defn validate-provenance! [provenance]
  (let [size (alength ^bytes (canonical-edn-bytes provenance))]
    (when (> size provenance-max-bytes)
      (fail! "corpus transaction provenance exceeds its canonical size limit"
             {:type :provenance-too-large
              :bytes size :limit provenance-max-bytes})))
  (let [valid?
        (cond
          (= schema-candidate-provenance-format (:format provenance))
          (and (exact-keys? provenance
                            #{:format :source_snapshot :finalized_candidate
                              :schema_receipt_id})
               (valid-source-snapshot? (:source_snapshot provenance))
               (valid-candidate? (:finalized_candidate provenance)
                                 finalized-candidate-id-pattern)
               (string? (:schema_receipt_id provenance))
               (re-matches schema-receipt-id-pattern
                           (:schema_receipt_id provenance)))

          (= snapshot-restore-provenance-format (:format provenance))
          (and (exact-keys? provenance
                            #{:format :source_snapshot :restore_candidate})
               (valid-source-snapshot? (:source_snapshot provenance))
               (valid-candidate? (:restore_candidate provenance)
                                 restore-candidate-id-pattern))

          :else false)]
    (when-not valid?
      (fail! "corpus transaction provenance is missing, malformed, or contains unknown fields"
             {:type :provenance-invalid :format (:format provenance)})))
  (canonical-form provenance))

(defn plan-payload [plan]
  (dissoc plan :plan-id))

(defn plan-digest [plan]
  (sha256-text (pr-str (canonical-form (plan-payload plan)))))

(defn seal-plan [plan]
  (let [candidate (assoc plan :format plan-format)]
    (assoc candidate :plan-id (str "plan-" (plan-digest candidate)))))

(defn live-pair! [live]
  (doseq [role roles]
    (when-not (map? (get live role))
      (fail! (str "plan is missing live " (name role) " record"))))
  (let [coord (canonical-regular-file! "live coordination corpus"
                                       (get-in live [:coordination :path]))
        telem (canonical-regular-file! "live telemetry corpus"
                                       (get-in live [:telemetry :path]))
        coord-parent (.getCanonicalPath (.getParentFile (io/file coord)))
        telem-parent (.getCanonicalPath (.getParentFile (io/file telem)))]
    (when (= coord telem)
      (fail! "coordination and telemetry paths must be distinct"))
    (when-not (= coord-parent telem-parent)
      (fail! "coordination and telemetry corpora must share one state directory"))
    {:coordination coord :telemetry telem :directory coord-parent}))

(defn verify-plan!
  ([plan] (verify-plan! plan nil))
  ([plan expected-live]
   (when-not (= plan-format (:format plan))
     (fail! (str "unsupported corpus transaction plan format: " (:format plan))))
   (let [provenance (validate-provenance! (:provenance plan))]
     (when-not (and (string? (:plan-id plan))
                    (re-matches plan-id-pattern (:plan-id plan))
                    (= (:plan-id plan) (str "plan-" (plan-digest plan))))
       (fail! "corpus transaction plan seal is invalid"))
     (doseq [role roles]
       (verify-corpus-record! (str "live " (name role))
                              (get-in plan [:live role]))
       (verify-corpus-record! (str "candidate " (name role))
                              (get-in plan [:candidate role])))
     (let [pair (live-pair! (:live plan))]
       (when expected-live
         (doseq [role roles]
           (when-not (= (get pair role) (get expected-live role))
             (fail! (str "plan live " (name role)
                         " path does not match this North instance")))))
       (assoc plan :provenance provenance :verified-live pair)))))

(defn make-plan
  [{:keys [purpose live candidate target runtime metadata provenance]}]
  (let [provenance (validate-provenance! provenance)]
    (seal-plan
     {:purpose (or purpose "unspecified-maintenance")
      :created-at (now-iso)
      :provenance provenance
      :live (into {} (map (fn [role]
                            [role (corpus-file-record (str "live " (name role))
                                                      (get live role))]) roles))
      :candidate (into {} (map (fn [role]
                                 [role (corpus-file-record
                                        (str "candidate " (name role))
                                        (get candidate role))]) roles))
      :target (or target {})
      :runtime (or runtime {})
      :metadata (or metadata {})})))

(defn write-bytes-durable! [path ^bytes payload]
  (let [target (io/file path)
        parent (.getParentFile target)]
    (ensure-dir! (.getPath parent))
    (with-open [output (java.io.FileOutputStream. target)]
      (.write output payload)
      (.flush output)
      (.force (.getChannel output) true))
    (fsync-dir! (.getCanonicalPath parent)))
  path)

(defn move-atomic! [source target]
  (java.nio.file.Files/move
   (.toPath (io/file source))
   (.toPath (io/file target))
   (into-array java.nio.file.CopyOption
               [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
  (fsync-dir! (.getCanonicalPath (.getParentFile (io/file target))))
  target)

(defn move-atomic-new! [source target]
  (java.nio.file.Files/move
   (.toPath (io/file source))
   (.toPath (io/file target))
   (into-array java.nio.file.CopyOption
               [java.nio.file.StandardCopyOption/ATOMIC_MOVE]))
  (fsync-dir! (.getCanonicalPath (.getParentFile (io/file target))))
  target)

(defn write-edn-atomic! [path value]
  (let [target (io/file path)
        parent (ensure-dir! (.getPath (.getParentFile target)))
        tmp (str parent "/." (.getName target) "." (java.util.UUID/randomUUID) ".tmp")]
    (write-bytes-durable!
     tmp (.getBytes (str (pr-str (canonical-form value)) "\n")
                    java.nio.charset.StandardCharsets/UTF_8))
    (move-atomic! tmp (.getCanonicalPath target))))

(defn artifact-record [path]
  (select-keys (file-record "content-addressed artifact" path)
               [:path :bytes :sha256]))

(defn write-content-addressed-edn!
  "Write an immutable canonical EDN artifact. An existing object at the same
  digest is accepted only when its bytes verify; it is never overwritten."
  [directory prefix value]
  (let [payload (.getBytes (str (pr-str (canonical-form value)) "\n")
                           java.nio.charset.StandardCharsets/UTF_8)
        digest (sha256-bytes payload)
        parent (ensure-dir! directory)
        target (str parent "/" prefix "-" digest ".edn")
        tmp (str parent "/." prefix "." (java.util.UUID/randomUUID) ".tmp")]
    (if (.exists (io/file target))
      (do
        (when-not (= digest (sha256-file target))
          (fail! (str "content-addressed artifact digest collision: " target)))
        (artifact-record target))
      (do
        (write-bytes-durable! tmp payload)
        (try
          (move-atomic-new! tmp target)
          (catch java.nio.file.FileAlreadyExistsException _
            (java.nio.file.Files/deleteIfExists (.toPath (io/file tmp)))
            (fsync-dir! parent)
            (when-not (= digest (sha256-file target))
              (fail! (str "content-addressed artifact race mismatch: " target)))))
        (artifact-record target)))))

(defn read-edn-file! [label path]
  (let [f (io/file path)]
    (when-not (.isFile f)
      (fail! (str label " is missing: " path)))
    (when (> (.length f) (* 2 1024 1024))
      (fail! (str label " exceeds the 2 MiB control-file limit")))
    (try
      (edn/read-string (slurp f))
      (catch Exception error
        (fail! (str label " is not valid EDN: " path)
               {:cause (.getMessage error)})))))

(defn delete-file-durable! [path]
  (let [f (io/file path)]
    (when (.exists f)
      (when (java.nio.file.Files/isSymbolicLink (.toPath f))
        (fail! (str "refusing to delete symbolic-link control file: " path)))
      (java.nio.file.Files/delete (.toPath f))
      (fsync-dir! (.getCanonicalPath (.getParentFile f)))))
  nil)

(defn copy-durable! [source target expected-record permissions]
  (verify-record! "copy source" expected-record)
  (let [src (io/file source)
        dst (io/file target)]
    (ensure-dir! (.getPath (.getParentFile dst)))
    (with-open [input (java.io.FileInputStream. src)
                output (java.io.FileOutputStream. dst)]
      (let [buffer (byte-array 65536)]
        (loop []
          (let [n (.read input buffer)]
            (when (pos? n)
              (.write output buffer 0 n)
              (recur)))))
      (.flush output)
      (.force (.getChannel output) true))
    (set-posix-permissions! (.getPath dst) permissions)
    ;; chmod is metadata too. Force the file only after its final mode is in
    ;; place, then force the containing directory before claiming durability.
    (fsync-file! (.getPath dst))
    (when-not (= (select-keys expected-record [:bytes :sha256])
                 (select-keys (file-record "durable copy" (.getPath dst))
                              [:bytes :sha256]))
      (fail! (str "durable copy verification failed: " target)))
    (fsync-dir! (.getCanonicalPath (.getParentFile dst))))
  target)

(defn copy-prefix-durable! [source target expected-record permissions]
  (let [expected-bytes (long (:bytes expected-record))]
    (when-not (= (:sha256 expected-record)
                 (prefix-sha256 source expected-bytes))
      (fail! (str "live corpus prefix no longer matches the pre-lease plan: " source)))
    (let [dst (io/file target)
          buffer (byte-array 65536)]
      (ensure-dir! (.getPath (.getParentFile dst)))
      (with-open [input (java.io.FileInputStream. (str source))
                  output (java.io.FileOutputStream. dst)]
        (loop [remaining expected-bytes]
          (when (pos? remaining)
            (let [want (int (min remaining (alength buffer)))
                  n (.read input buffer 0 want)]
              (when-not (pos? n)
                (fail! (str "unexpected EOF copying pre-lease prefix: " source)))
              (.write output buffer 0 n)
              (recur (- remaining n)))))
        (.flush output)
        (.force (.getChannel output) true))
      (set-posix-permissions! (.getPath dst) permissions)
      (fsync-file! (.getPath dst))
      (verify-record! "durable pre-lease prefix"
                      (assoc expected-record :path (.getPath dst)))
      (fsync-dir! (.getCanonicalPath (.getParentFile dst))))
    target))

(defn acquire-maintenance-lock!
  ([] (acquire-maintenance-lock! false))
  ([blocking?]
  (ensure-dir! (state-root))
  (let [raf (java.io.RandomAccessFile. (maintenance-lock-path) "rw")
        channel (.getChannel raf)]
    (try
      (if-let [lock (if blocking? (.lock channel) (.tryLock channel))]
        {:channel channel :lock lock}
        (do (.close channel)
            (fail! "another North corpus maintenance transaction is active")))
      (catch IllegalStateException error
        (.close channel)
        (fail! "another North corpus maintenance transaction is active"
               {:cause (.getMessage error)}))))))

(defn close-lock! [handle]
  (when-let [channel (:channel handle)]
    (.close ^java.nio.channels.FileChannel channel))
  nil)

(defmacro with-maintenance-lock [& body]
  `(let [handle# (acquire-maintenance-lock!)]
     (try ~@body (finally (close-lock! handle#)))))

(defmacro with-maintenance-lock-wait [& body]
  `(let [handle# (acquire-maintenance-lock! true)]
     (try ~@body (finally (close-lock! handle#)))))

(defn acquire-file-locks! [paths]
  (loop [remaining (sort paths) handles []]
    (if (empty? remaining)
      handles
      (let [path (first remaining)
            raf (java.io.RandomAccessFile. path "rw")
            channel (.getChannel raf)
            handle
            (try
              (.lock channel)
              {:path path :channel channel}
              (catch Throwable error
                (.close channel)
                (doseq [prior handles] (close-lock! prior))
                (throw error)))]
        (recur (rest remaining) (conj handles handle))))))

(defn acquire-offline-fences! [pair]
  ;; Fram's rewrite lock is directory-scoped, so one exclusive grant fences
  ;; ordinary coordination and telemetry appenders together. Direct file locks
  ;; additionally exclude North's exceptional offline repair writer.
  (let [rewrite (rt/acquire-rewrite-lock! (:coordination pair) false true)]
    (try
      {:rewrite rewrite
       :files (acquire-file-locks! [(:coordination pair) (:telemetry pair)])}
      (catch Throwable error
        (rt/close-rewrite-lock! rewrite)
        (throw error)))))

(defn close-offline-fences! [handles]
  (doseq [handle (reverse (:files handles))] (close-lock! handle))
  (rt/close-rewrite-lock! (:rewrite handles))
  nil)

(defmacro with-offline-fences [pair & body]
  `(let [handles# (acquire-offline-fences! ~pair)]
     (try ~@body (finally (close-offline-fences! handles#)))))

(defn transaction-id [plan]
  (str "txn-"
       (sha256-text (str (:plan-id plan) "|" (now-iso) "|"
                         (java.util.UUID/randomUUID)))))

(declare maybe-inject-crash!)

(def pre-rename-phases #{:pre-stop :stopped :preimage-prepared})
(def replacement-phases
  #{:prepared :coordination-replaced :both-replaced :recovering
    :recovered-coordination :recovered-telemetry})
(def settled-data-phases
  #{:data-resolved :runtime-unverified :runtime-verified :lease-settled
    :settlement-recorded})

(defn record-content= [left right]
  (= (select-keys left [:path :bytes :sha256])
     (select-keys right [:path :bytes :sha256])))

(defn pair-content= [left right]
  (every? true? (map #(record-content= (get left %) (get right %)) roles)))

(defn current-live-records! [live]
  (into {}
        (map (fn [role]
               [role (corpus-file-record (str "current live " (name role))
                                         (get-in live [role :path]))])
             roles)))

(defn execution-seal [journal]
  (sha256-text
   (pr-str
    (canonical-form
     (select-keys journal [:plan-id :purpose :provenance :original-live :live
                           :candidate :target :runtime :checkpoint :lease])))))

(defn validate-journal! [journal]
  (when-not (= journal-format (:format journal))
    (fail! (str "unsupported active corpus journal format: " (:format journal))))
  (let [provenance (validate-provenance! (:provenance journal))]
    (when-not (and (string? (:transaction-id journal))
                   (str/starts-with? (:transaction-id journal) "txn-")
                   (= (:execution-seal journal) (execution-seal journal)))
      (fail! "active corpus journal execution seal is invalid"))
    (live-pair! (:live journal))
    (assoc journal :provenance provenance)))

(defn update-journal! [journal phase]
  (let [next (assoc journal :phase phase :updated-at (now-iso))]
    (write-edn-atomic! (journal-path) next)
    next))

(defn clear-journal! []
  (delete-file-durable! (journal-path)))

(defn prevalidate-active-journal! []
  (when (.isFile (io/file (journal-path)))
    (validate-journal!
     (read-edn-file! "active corpus transaction journal" (journal-path)))))

(defn persist-content-object! [role source-record]
  (let [directory (ensure-dir! (str (state-root) "/objects/sha256"))
        target (str directory "/" (:sha256 source-record) ".log")
        tmp (str directory "/." (name role) "." (java.util.UUID/randomUUID) ".tmp")]
    (if (.exists (io/file target))
      (let [existing (file-record "content-addressed corpus object" target)]
        (when-not (= (select-keys source-record [:bytes :sha256])
                     (select-keys existing [:bytes :sha256]))
          (fail! (str "content-addressed corpus object mismatch: " target)))
        (assoc (select-keys existing [:path :bytes :sha256])
               :permissions (:permissions source-record)
               :append-boundary append-boundary))
      (do
        (copy-durable! (:path source-record) tmp source-record
                       (:permissions source-record))
        (try
          (move-atomic-new! tmp target)
          (catch java.nio.file.FileAlreadyExistsException _
            (delete-file-durable! tmp)))
        (let [stored (file-record "content-addressed corpus object" target)]
          (when-not (= (select-keys source-record [:bytes :sha256])
                       (select-keys stored [:bytes :sha256]))
            (fail! (str "persisted corpus object mismatch: " target)))
          (assoc (select-keys stored [:path :bytes :sha256])
                 :permissions (:permissions source-record)
                 :append-boundary append-boundary))))))

(defn persist-preimage! [journal current]
  (let [preimage (into {}
                       (map (fn [role]
                              [role (persist-content-object! role (get current role))])
                            roles))
        manifest {:format preimage-format
                  :transaction-id (:transaction-id journal)
                  :plan-id (:plan-id journal)
                  :execution-seal (:execution-seal journal)
                  :provenance (:provenance journal)
                  :created-at (now-iso)
                  :files preimage}
        manifest-record
        (write-content-addressed-edn!
         (str (state-root) "/manifests") "preimage" manifest)
        next (assoc journal
                    :phase :preimage-prepared
                    :stopped-live current
                    :preimage preimage
                    :preimage-manifest manifest-record
                    :updated-at (now-iso))]
    ;; The journal points at the immutable objects only after every object and
    ;; the sealed manifest are durable. Before this point no rename is allowed.
    (write-edn-atomic! (journal-path) next)
    (maybe-inject-crash! "preimage-prepared")
    next))

(defn verify-preimage! [journal]
  (let [artifact (:preimage-manifest journal)]
    (verify-record! "preimage manifest" artifact)
    (let [manifest (read-edn-file! "preimage manifest" (:path artifact))]
      (when-not (= {:format preimage-format
                    :transaction-id (:transaction-id journal)
                    :plan-id (:plan-id journal)
                    :execution-seal (:execution-seal journal)
                    :provenance (:provenance journal)
                    :files (:preimage journal)}
                   (dissoc manifest :created-at))
        (fail! "preimage manifest does not match its active transaction"))
      (doseq [role roles]
        (verify-corpus-record! (str "preimage " (name role))
                               (get-in journal [:preimage role])))))
  journal)

(defn create-intent-journal! [execution]
  (let [base {:format journal-format
              :transaction-id (transaction-id execution)
              :plan-id (:plan-id execution)
              :purpose (:purpose execution)
              :provenance (:provenance execution)
              :created-at (now-iso)
              :phase :pre-stop
              :original-live (:original-live execution)
              :live (:live execution)
              :candidate (:candidate execution)
              :target (:target execution)
              :runtime (:runtime execution)
              :checkpoint (:checkpoint execution)
              :lease (:lease execution)
              :candidate-omits-maintenance-lease true}
        journal (assoc base :execution-seal (execution-seal base))]
    ;; This intent and exact lease epoch are durable before supervisor stop.
    (write-edn-atomic! (journal-path) journal)
    journal))

(defn prepare-transaction! [journal]
  (verify-preimage! journal)
  (let [pair (live-pair! (:live journal))
        txid (:transaction-id journal)
        stage {:coordination (str (:directory pair) "/.north-corpus-" txid
                                  ".coordination.next")
               :telemetry (str (:directory pair) "/.north-corpus-" txid
                               ".telemetry.next")}]
    (doseq [role roles]
      (let [candidate (get-in journal [:candidate role])]
        (copy-durable! (:path candidate) (get stage role) candidate
                       (get-in journal [:stopped-live role :permissions]))))
    (maybe-inject-crash! "stages-prepared")
    (let [prepared (assoc journal :phase :prepared :stage stage
                          :updated-at (now-iso))]
      (write-edn-atomic! (journal-path) prepared)
      (maybe-inject-crash! "prepared")
      (maybe-inject-crash! "journal")
      prepared)))

(defn receipt-payload [journal result]
  {:format receipt-format
   :transaction-id (:transaction-id journal)
   :plan-id (:plan-id journal)
   :execution-seal (:execution-seal journal)
   :purpose (:purpose journal)
   :provenance (:provenance journal)
   :result result
   ;; These instants are durable journal facts written once before publication;
   ;; retrying the same receipt therefore reproduces byte-identical content.
   :data-resolved-at (:data-resolved-at journal)
   :runtime-verified-at (:runtime-verified-at journal)
   :settled-at (:settled-at journal)
   :preimage-manifest (:preimage-manifest journal)
   :candidate (:candidate journal)
   :target (:target journal)
   :runtime (:runtime journal)
   :checkpoint (:checkpoint journal)
   :lease (:lease journal)
   :runtime-verification (:runtime-verification journal)
   :runtime-error (:runtime-error journal)
   :runtime-unverified-at (:runtime-unverified-at journal)
   :pre-settlement-corpus (:pre-settlement-corpus journal)
   :lease-settlement (:lease-settlement journal)
   :post-settlement-corpus (:post-settlement-corpus journal)})

(defn write-receipt! [journal result]
  (let [receipt (receipt-payload journal result)
        artifact (write-content-addressed-edn!
                  (str (state-root) "/receipts") "receipt" receipt)]
    {:receipt receipt :artifact artifact :path (:path artifact)}))

(defn verify-receipt! [journal]
  (let [artifact (:receipt-artifact journal)]
    (verify-record! "settlement receipt" artifact)
    (let [receipt (read-edn-file! "settlement receipt" (:path artifact))]
      (when-not (= (receipt-payload journal (:data-result journal)) receipt)
        (fail! "settlement receipt does not match its active transaction"))
      {:receipt receipt :artifact artifact :path (:path artifact)})))

(defn replace-stage! [journal role]
  (let [stage (get-in journal [:stage role])
        target (get-in journal [:live role :path])]
    (verify-corpus-record! (str "staged " (name role))
                           (assoc (get-in journal [:candidate role]) :path stage))
    (move-atomic! stage target)
    (verify-corpus-record! (str "replaced live " (name role))
                           (assoc (get-in journal [:candidate role]) :path target))))

(defn prepare-recovery-stage! [journal role]
  (let [pair (live-pair! (:live journal))
        stage (str (:directory pair) "/.north-corpus-"
                   (:transaction-id journal) "." (name role) ".recover")
        preimage (get-in journal [:preimage role])]
    (copy-durable! (:path preimage) stage preimage
                   (:permissions preimage))
    stage))

(defn mark-data-resolved!
  ([journal result] (mark-data-resolved! journal result nil))
  ([journal result reason]
   (let [resolved (cond-> (assoc journal
                                 :phase :data-resolved
                                 :data-result result
                                 :data-resolved-at (now-iso)
                                 :updated-at (now-iso))
                    reason (assoc :resolution-reason reason))]
     (write-edn-atomic! (journal-path) resolved)
     resolved)))

(defn live-exactly-matches? [records live]
  (every? true?
          (for [role roles]
            (let [path (get-in live [role :path])]
              (and (append-boundary-valid? path)
                   (record-matches? (assoc (get records role) :path path)))))))

(defn recover-journal-under-fence! [raw-journal]
  (let [journal (validate-journal! raw-journal)
        phase (:phase journal)]
    (doseq [role roles]
      (verify-append-boundary! (str "recovery live " (name role))
                               (get-in journal [:live role :path])))
    (cond
      ;; No live rename can occur before :prepared is durable. Recovery in this
      ;; region snapshots the exact stopped bytes and preserves them verbatim;
      ;; original-live is OCC/provenance only and is never a rollback image.
      (#{:pre-stop :stopped} phase)
      (-> journal
          (persist-preimage! (current-live-records! (:live journal)))
          (mark-data-resolved! "rolled-back" "interrupted-before-preimage"))

      (= :preimage-prepared phase)
      (do
        (verify-preimage! journal)
        (when-not (live-exactly-matches? (:preimage journal) (:live journal))
          (fail! "live corpus drifted after the stopped preimage was sealed"))
        (mark-data-resolved! journal "rolled-back" "interrupted-before-rename"))

      (replacement-phases phase)
      (do
        (verify-preimage! journal)
        ;; Candidate source paths are staging conveniences, not recovery
        ;; authority. Sealed candidate digests plus immutable preimage objects
        ;; are sufficient after a builder cleans its staging directory.
        (let [target? (into {}
                            (map (fn [role]
                                   [role (record-matches?
                                          (assoc (get-in journal [:candidate role])
                                                 :path (get-in journal [:live role :path])))])
                                 roles))
              old? (into {}
                         (map (fn [role]
                                [role (record-matches?
                                       (assoc (get-in journal [:preimage role])
                                              :path (get-in journal [:live role :path])))])
                              roles))]
          (cond
            (every? true? (vals target?))
            (mark-data-resolved! journal "committed")

            (every? true? (vals old?))
            (mark-data-resolved! journal "rolled-back")

            :else
            (let [stages (into {}
                               (map (fn [role]
                                      [role (prepare-recovery-stage! journal role)])
                                    roles))
                  recovering (update-journal! journal :recovering)
                  recovered
                  (reduce
                   (fn [current role]
                     (move-atomic! (get stages role)
                                   (get-in journal [:live role :path]))
                     (verify-corpus-record!
                      (str "recovered live " (name role))
                      (assoc (get-in journal [:preimage role])
                             :path (get-in journal [:live role :path])))
                     (update-journal!
                      current (keyword (str "recovered-" (name role)))))
                   recovering roles)]
              (mark-data-resolved! recovered "rolled-back" "mixed-live-pair")))))

      (settled-data-phases phase)
      (do (verify-preimage! journal) journal)

      :else
      (fail! (str "unsupported active corpus journal phase: " phase)))))

(defn assert-expected-live! [journal expected-live]
  (let [pair (live-pair! (:live journal))]
    (when expected-live
      (doseq [role roles]
        (when-not (= (get pair role) (get expected-live role))
          (fail! (str "active journal belongs to a different "
                      (name role) " corpus")))))
    pair))

(defn recover-active! [{:keys [expected-live assert-offline!]}]
  ;; A malformed or provenance-tampered journal is rejected before opening the
  ;; maintenance lock file. The journal is read and validated again under the
  ;; lock so this early gate grants no race authority.
  (prevalidate-active-journal!)
  (with-maintenance-lock
    (if-not (.isFile (io/file (journal-path)))
      {:result "clean"}
      (let [journal (validate-journal!
                     (read-edn-file! "active corpus transaction journal"
                                     (journal-path)))
            pair (assert-expected-live! journal expected-live)]
        (when-not assert-offline!
          (fail! "offline assertion callback is required for corpus recovery"))
        (assert-offline!)
        (with-offline-fences pair
          (recover-journal-under-fence! journal))))))

(defn live-matches-resolution? [journal]
  (let [records (case (:data-result journal)
                  "committed" (:candidate journal)
                  "rolled-back" (:preimage journal)
                  nil)]
    (and records
         (every? #(append-boundary-valid?
                   (get-in journal [:live % :path])) roles)
         (every? true?
                 (for [role roles]
                   (record-prefix-matches?
                    (get records role) (get-in journal [:live role :path])))))))

(defn resolution-evidence! [journal]
  (let [expected (case (:data-result journal)
                   "committed" (:candidate journal)
                   "rolled-back" (:preimage journal)
                   (fail! (str "unknown data resolution: " (:data-result journal))))
        observed
        (into {}
              (map (fn [role]
                     [role (corpus-file-record
                            (str "settlement live " (name role))
                            (get-in journal [:live role :path]))])
                   roles))]
    (doseq [role roles]
      (when-not (record-prefix-matches? (get expected role)
                                        (get-in observed [role :path]))
        (fail! (str "settlement live " (name role)
                    " lost its sealed resolved prefix"))))
    {:resolution (:data-result journal)
     :expected-prefix expected
     :observed observed
     :prefix-match true}))

(defn settle-journal-online! [raw-journal {:keys [verify-restart! settle-lease!]}]
  (let [journal (validate-journal! raw-journal)]
    (if (= :settlement-recorded (:phase journal))
      (let [receipt (verify-receipt! journal)]
        (clear-journal!)
        receipt)
      (do
        (when-not (#{:data-resolved :runtime-unverified :runtime-verified
                     :lease-settled} (:phase journal))
          (fail! (str "corpus journal data is not resolved: " (:phase journal))))
        (verify-preimage! journal)
        (when-not (live-matches-resolution? journal)
          (fail! "live corpus no longer contains the sealed resolved prefix"))
        (when-not (and verify-restart! settle-lease!)
          (fail! "runtime verification and exact lease settlement callbacks are required"))
        (let [runtime-verified
              (if (#{:runtime-verified :lease-settled} (:phase journal))
                journal
                (let [verdict (verify-restart! journal)]
                  (when-not (= true (:ok verdict))
                    (fail! (str "coordinator runtime verification was not positive: "
                                (pr-str verdict))
                           {:runtime-verification verdict}))
                  (let [verified (assoc journal
                                        :phase :runtime-verified
                                        :runtime-verification verdict
                                        :pre-settlement-corpus
                                        (resolution-evidence! journal)
                                        :runtime-verified-at (now-iso)
                                        :updated-at (now-iso))]
                    (write-edn-atomic! (journal-path) verified)
                    verified)))
              _ (maybe-inject-crash! "settlement-verified")
              lease-settled
              (if (= :lease-settled (:phase runtime-verified))
                runtime-verified
                (let [lease-result
                      (settle-lease! (:lease runtime-verified) runtime-verified)]
                  (when-not (= true (:ok lease-result))
                    (fail! (str "exact maintenance lease settlement failed: "
                                (pr-str lease-result))))
                  ;; Type=simple can accept legitimate writes before ExecStartPost
                  ;; runs. Preserve and attest the valid suffix rather than
                  ;; demanding whole-file equality.
                  (let [settled (assoc runtime-verified
                                       :phase :lease-settled
                                       :lease-settlement lease-result
                                       :post-settlement-corpus
                                       (resolution-evidence! runtime-verified)
                                       :settled-at (now-iso)
                                       :updated-at (now-iso))]
                    (write-edn-atomic! (journal-path) settled)
                    settled)))
              _ (maybe-inject-crash! "lease-settled")
              receipt (write-receipt! lease-settled
                                      (:data-result lease-settled))
              _ (maybe-inject-crash! "receipt-published")
              recorded (assoc lease-settled
                              :phase :settlement-recorded
                              :receipt-artifact (:artifact receipt)
                              :updated-at (now-iso))]
          (write-edn-atomic! (journal-path) recorded)
          (maybe-inject-crash! "settlement-receipt")
          (clear-journal!)
          (maybe-inject-crash! "settlement-cleared")
          receipt)))))

(defn settle-active! [{:keys [wait? expected-live] :as callbacks}]
  (prevalidate-active-journal!)
  (let [run (fn []
              (if-not (.isFile (io/file (journal-path)))
                {:result "clean"}
                (let [journal (validate-journal!
                               (read-edn-file!
                                "active corpus transaction journal"
                                (journal-path)))]
                  (assert-expected-live! journal expected-live)
                  (settle-journal-online! journal callbacks))))]
    (if wait?
      (with-maintenance-lock-wait (run))
      (with-maintenance-lock (run)))))

(defn maybe-inject-crash! [phase]
  (when (= phase (System/getenv "NORTH_CORPUS_TRANSACTION_FAIL_AFTER"))
    (throw (ex-info (str "simulated corpus transaction crash after " phase)
                    {:north.corpus-transaction/simulated-crash true
                     :phase phase}))))

(defn validate-lease! [lease]
  (when-not (and (:ok lease)
                 (string? (:resource lease))
                 (not (str/blank? (:resource lease)))
                 (string? (:holder lease))
                 (not (str/blank? (:holder lease)))
                 (integer? (:epoch lease)))
    (fail! (str "singleton graph maintenance lease unavailable or malformed: "
                (pr-str lease))))
  lease)

(defn finalize-execution! [verified lease checkpoint-source!]
  (let [checkpoint (checkpoint-source! verified lease)
        execution-live (:live checkpoint)
        candidate (:candidate checkpoint)
        target (:target checkpoint)
        version (:version checkpoint)]
    (doseq [role roles]
      (verify-corpus-record! (str "post-lease live " (name role))
                             (get execution-live role))
      (verify-corpus-record! (str "checkpoint-finalized candidate " (name role))
                             (get candidate role)))
    (let [execution-pair (live-pair! execution-live)]
      (doseq [role roles]
        (when-not (= (get execution-pair role)
                     (get (:verified-live verified) role))
          (fail! (str "lease checkpoint changed the live " (name role) " path"))))
      (when (some #{(:coordination execution-pair) (:telemetry execution-pair)}
                  (map #(get-in candidate [% :path]) roles))
        (fail! "checkpoint-finalized candidates must not alias live corpus paths"))
      (when-not (and (integer? version) (not (neg? version))
                     (integer? (:corpus-max-tx target))
                     (>= (:corpus-max-tx target) version))
        (fail! (str "candidate watermark would regress the checkpoint version: "
                    (pr-str {:checkpoint-version version
                             :candidate-max-tx (:corpus-max-tx target)}))))
      (assoc verified
             :original-live (:live verified)
             :live execution-live
             :candidate candidate
             :target target
             :runtime (or (:runtime checkpoint) (:runtime verified))
             :verified-live execution-pair
             :checkpoint (dissoc checkpoint :live :candidate :target :runtime
                                 :provenance)
             :lease lease))))

(defn mark-runtime-unverified! [error]
  (when (.isFile (io/file (journal-path)))
    (let [journal (validate-journal!
                   (read-edn-file! "active corpus transaction journal"
                                   (journal-path)))
          pending (assoc journal
                         :phase :runtime-unverified
                         :runtime-error (.getMessage error)
                         :runtime-verification
                         (or (:runtime-verification (ex-data error))
                             (:runtime-verification journal)
                             {:ok false :error (.getMessage error)})
                         :runtime-unverified-at (now-iso)
                         :updated-at (now-iso))
          _ (write-edn-atomic! (journal-path) pending)
          receipt (write-receipt!
                   pending (str (:data-result pending) "-runtime-unverified"))]
      {:journal pending :receipt receipt})))

(defn apply-plan!
  "Apply one sealed plan through a durable pre-stop intent, exact stopped
  preimage, and all-old/all-new swap. checkpoint-source! must finalize and seal
  candidate records plus a non-regressing corpus watermark under the acquired
  maintenance lease."
  [plan {:keys [expected-live acquire-lease! release-lease! checkpoint-source!
                stop! start! assert-offline! verify-restart! settle-lease!]
         :as callbacks}]
  (doseq [[label callback]
          [[:acquire-lease! acquire-lease!] [:release-lease! release-lease!]
           [:checkpoint-source! checkpoint-source!] [:stop! stop!]
           [:start! start!] [:assert-offline! assert-offline!]
           [:verify-restart! verify-restart!] [:settle-lease! settle-lease!]]]
    (when-not (fn? callback)
      (fail! (str "missing corpus transaction callback " label))))
  ;; The complete plan, including structured provenance and its content seal,
  ;; is verified before creating the state directory or maintenance lock.
  ;; Reverification inside the lock retains the existing OCC behavior.
  (verify-plan! plan expected-live)
  (with-maintenance-lock
    (when (.isFile (io/file (journal-path)))
      (fail! (str "an active corpus transaction journal already exists at "
                  (journal-path) "; recover it before applying another plan")))
    (let [lease* (atom nil)
          stop-invoked? (atom false)
          stopped? (atom false)
          data-resolved? (atom false)
          start-attempted? (atom false)]
      (try
        (let [verified (verify-plan! plan expected-live)
              _ (doseq [role roles]
                  (verify-corpus-record! (str "preflight live " (name role))
                                         (get-in verified [:live role])))
              lease (validate-lease! (acquire-lease!))
              _ (reset! lease* lease)
              execution (finalize-execution! verified lease checkpoint-source!)
              intent (create-intent-journal! execution)
              _ (maybe-inject-crash! "pre-stop-journal")
              _ (reset! stop-invoked? true)
              _ (stop!)
              _ (reset! stopped? true)
              stopped-journal (update-journal! intent :stopped)
              _ (maybe-inject-crash! "stopped")
              _ (assert-offline!)
              outcome
              (with-offline-fences (:verified-live execution)
                (let [current (current-live-records! (:live execution))
                      preimaged (persist-preimage! stopped-journal current)]
                  (if-not (pair-content= current (:live execution))
                    {:drift true
                     :resolved (mark-data-resolved!
                                preimaged "rolled-back"
                                "source-drift-between-checkpoint-and-stop")}
                    (let [prepared (prepare-transaction! preimaged)
                          _ (replace-stage! prepared :coordination)
                          _ (maybe-inject-crash! "coordination-renamed")
                          coordination
                          (update-journal! prepared :coordination-replaced)
                          _ (maybe-inject-crash! "coordination-journaled")
                          _ (replace-stage! coordination :telemetry)
                          _ (maybe-inject-crash! "telemetry-renamed")
                          both (update-journal! coordination :both-replaced)
                          _ (maybe-inject-crash! "telemetry-journaled")]
                      (doseq [role roles]
                        (verify-corpus-record!
                         (str "committed live " (name role))
                         (assoc (get-in execution [:candidate role])
                                :path (get-in execution [:live role :path]))))
                      {:drift false
                       :resolved (mark-data-resolved! both "committed")}))))
              _ (reset! data-resolved? true)
              _ (reset! start-attempted? true)
              _ (start!)
              receipt (settle-journal-online! (:resolved outcome) callbacks)]
          (if (:drift outcome)
            {:ok false
             :aborted :source-drift
             :plan-id (:plan-id verified)
             :receipt receipt}
            {:ok true :plan-id (:plan-id verified) :receipt receipt}))
        (catch Throwable error
          (cond
            (:north.corpus-transaction/simulated-crash (ex-data error))
            ;; A simulated process death performs no catch cleanup. The durable
            ;; journal is the only authority consumed by recovery/settlement.
            (throw error)

            (nil? @lease*)
            (throw error)

            (not @stop-invoked?)
            (let [released (release-lease! @lease*)]
              (when (:ok released)
                (when (.isFile (io/file (journal-path))) (clear-journal!)))
              (throw error))

            (not @stopped?)
            ;; stop! was invoked but did not prove a terminal state. Preserve
            ;; the pre-stop intent and exact lease for an authoritative recovery.
            (throw (ex-info
                    (str "coordinator stop outcome is unknown; durable recovery is required: "
                         (.getMessage error))
                    {:north.corpus-transaction/recovery-required true}
                    error))

            @data-resolved?
            (let [pending (mark-runtime-unverified! error)]
              (throw (ex-info
                      (str "corpus data is resolved, but runtime settlement failed: "
                           (.getMessage error))
                      {:north.corpus-transaction/data-resolved true
                       :north.corpus-transaction/runtime-verified false
                       :receipt (:receipt pending)}
                      error)))

            :else
            ;; Ordinary preparation/replacement failures are repaired before
            ;; returning: exact stopped preimage -> restart once -> settle the
            ;; exact durable lease. A failed restart remains retry-settleable.
            (try
              (assert-offline!)
              (let [journal (validate-journal!
                             (read-edn-file!
                              "active corpus transaction journal" (journal-path)))
                    pair (assert-expected-live! journal expected-live)
                    resolved (with-offline-fences pair
                               (recover-journal-under-fence! journal))]
                (reset! data-resolved? true)
                (reset! start-attempted? true)
                (start!)
                (let [receipt (settle-journal-online! resolved callbacks)]
                  (throw (ex-info
                          (str "corpus transaction failed and was rolled back safely: "
                               (.getMessage error))
                          {:north.corpus-transaction/recovered true
                           :receipt receipt}
                          error))))
              (catch Throwable recovery-error
                (if (:north.corpus-transaction/recovered (ex-data recovery-error))
                  (throw recovery-error)
                  (let [pending (when @data-resolved?
                                  (mark-runtime-unverified! recovery-error))]
                    (throw (ex-info
                            (str "corpus transaction recovery remains pending: "
                                 (.getMessage recovery-error))
                            {:north.corpus-transaction/recovery-required true
                             :runtime-unverified (boolean pending)
                             :receipt (:receipt pending)}
                            recovery-error))))))))))))
