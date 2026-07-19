;; inbox-peek.clj — bounded, quiet mail delivery for the PostToolUse hook.
;; Usage: bb inbox-peek.clj <port> <agent-id>
;;
;; The local spool is only a scheduling hint: it stores a bounded page of IDs
;; and the exact continuation cursor Fram issued. The graph remains authority.
;; Every cached ID is claimed and re-read before output, and only a complete,
;; flushed rendering is acknowledged.
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-contract.clj"))

(def one north.coord/resolved)
(def spool-schema "north-inbox-spool-v1")
(def spool-page-limit north.message-audience/pending-page-limit)
(def candidate-limit 3)
(def delivery-limit 3)
(def output-byte-limit (* 24 1024))
(def output-time-budget-ms 1600)
(def hook-work-budget-ms 1800)
(def lock-wait-ms 40)
(def actor-byte-limit 512)
(def actor-pattern #"^[A-Za-z0-9._:-]+$")
(def spool-byte-limit (* 192 1024))
(def spool-max-age-ms (* 60 60 1000))
(def state-keys
  #{:schema :actor-key :log-key :snapshot-version :created-at-ms :ids :next})
(def nofollow-links
  (into-array java.nio.file.LinkOption
              [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
(def file-permissions
  (java.nio.file.attribute.PosixFilePermissions/fromString "rw-------"))
(def directory-permissions
  (java.nio.file.attribute.PosixFilePermissions/fromString "rwx------"))

(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn sha256 [domain value]
  (let [digest
        (.digest
         (java.security.MessageDigest/getInstance "SHA-256")
         (.getBytes
          (str domain "\u0000" value)
          java.nio.charset.StandardCharsets/UTF_8))]
    (apply str
           (map #(format "%02x" (bit-and (int %) 0xff)) digest))))

(defn managed-actor-key [value]
  (when-not (and (string? value)
                 (pos? (utf8-bytes value))
                 (<= (utf8-bytes value) actor-byte-limit)
                 (boolean (re-matches actor-pattern value)))
    (throw (ex-info "inbox actor is outside the canonical identity surface"
                    {:type :invalid-inbox-actor})))
  (sha256 "north-actor-key-v1\u0000managed" value))

(defn canonical-log-key []
  (sha256 "north-inbox-spool-log-v1" (north.coord/expected-log)))

(defn valid-message-id? [value]
  (and (string? value)
       (<= (utf8-bytes value)
           north.message-contract/max-message-id-bytes)
       (boolean
        (re-matches #"^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn valid-cursor? [value]
  (north.coord/valid-query-page-cursor? value))

(defn exact-edn [bytes]
  (try
    (let [decoder
          (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
            (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
            (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))
          text (str (.decode decoder (java.nio.ByteBuffer/wrap bytes)))]
      (with-open [reader
                  (java.io.PushbackReader. (java.io.StringReader. text))]
        (let [eof (Object.)
              value (edn/read {:eof eof} reader)
              trailing (edn/read {:eof eof} reader)]
          (when (or (identical? eof value)
                    (not (identical? eof trailing)))
            (throw (ex-info "spool is not exactly one EDN form"
                            {:type :invalid-inbox-spool})))
          value)))
    (catch clojure.lang.ExceptionInfo error
      (throw error))
    (catch Exception error
      (throw (ex-info "spool is not canonical UTF-8 EDN"
                      {:type :invalid-inbox-spool}
                      error)))))

(defn bounded-file-bytes [path]
  (with-open [input
              (java.nio.file.Files/newInputStream
               path
               (make-array java.nio.file.OpenOption 0))
              output (java.io.ByteArrayOutputStream.)]
    (let [buffer (byte-array 4096)]
      (loop [total 0]
        (let [count (.read input buffer)]
          (if (= -1 count)
            (.toByteArray output)
            (let [next-total (+ total count)]
              (when (> next-total spool-byte-limit)
                (throw (ex-info "spool exceeds its byte bound"
                                {:type :invalid-inbox-spool
                                 :max-bytes spool-byte-limit})))
              (.write output buffer 0 count)
              (recur next-total))))))))

(defn path-exists? [path]
  (java.nio.file.Files/exists path nofollow-links))

(defn regular-file? [path]
  (java.nio.file.Files/isRegularFile path nofollow-links))

(defn directory? [path]
  (java.nio.file.Files/isDirectory path nofollow-links))

(defn require-regular-state! [path]
  (when (path-exists? path)
    (when-not (regular-file? path)
      (throw (ex-info "inbox state path is not a regular file"
                      {:type :unsafe-inbox-state
                       :path (str path)})))
    (java.nio.file.Files/setPosixFilePermissions path file-permissions))
  path)

(defn secure-directory! [path]
  (java.nio.file.Files/createDirectories
   path (make-array java.nio.file.attribute.FileAttribute 0))
  (when-not (directory? path)
    (throw (ex-info "inbox state root is not a directory"
                    {:type :unsafe-inbox-state
                     :path (str path)})))
  (java.nio.file.Files/setPosixFilePermissions path directory-permissions)
  path)

(defn fsync-directory! [directory]
  (with-open [channel
              (java.nio.channels.FileChannel/open
               directory
               (into-array java.nio.file.OpenOption
                           [java.nio.file.StandardOpenOption/READ]))]
    (.force channel true)))

(defn atomic-write! [path value]
  (let [payload (.getBytes (str (pr-str value) "\n")
                           java.nio.charset.StandardCharsets/UTF_8)
        _ (when (> (alength payload) spool-byte-limit)
            (throw (ex-info "spool encoding exceeds its byte bound"
                            {:type :invalid-inbox-spool
                             :max-bytes spool-byte-limit})))
        directory (.getParent path)
        temporary
        (.resolve directory
                  (str "." (.getFileName path) "."
                       (java.util.UUID/randomUUID) ".tmp"))
        attribute
        (java.nio.file.attribute.PosixFilePermissions/asFileAttribute
         file-permissions)]
    (try
      (with-open [channel
                  (java.nio.channels.FileChannel/open
                   temporary
                   #{java.nio.file.StandardOpenOption/CREATE_NEW
                     java.nio.file.StandardOpenOption/WRITE}
                   (into-array java.nio.file.attribute.FileAttribute
                               [attribute]))]
        (let [buffer (java.nio.ByteBuffer/wrap payload)]
          (while (.hasRemaining buffer)
            (.write channel buffer)))
        (.force channel true))
      (java.nio.file.Files/move
       temporary
       path
       (into-array
        java.nio.file.CopyOption
        [java.nio.file.StandardCopyOption/ATOMIC_MOVE
         java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (java.nio.file.Files/setPosixFilePermissions path file-permissions)
      (fsync-directory! directory)
      (finally
        (java.nio.file.Files/deleteIfExists temporary)))))

(defn delete-state! [path]
  (when (path-exists? path)
    (require-regular-state! path)
    (java.nio.file.Files/delete path)
    (fsync-directory! (.getParent path))))

(defn valid-spool? [value actor-key log-key current-version now]
  (and
   (map? value)
   (= state-keys (set (keys value)))
   (= spool-schema (:schema value))
   (= actor-key (:actor-key value))
   (= log-key (:log-key value))
   (integer? (:snapshot-version value))
   (not (neg? (:snapshot-version value)))
   (<= (:snapshot-version value) current-version)
   (integer? (:created-at-ms value))
   (<= 0 (- now (:created-at-ms value)) spool-max-age-ms)
   (vector? (:ids value))
   (<= (count (:ids value)) spool-page-limit)
   (= (count (:ids value)) (count (distinct (:ids value))))
   (every? valid-message-id? (:ids value))
   (or (nil? (:next value)) (valid-cursor? (:next value)))))

(defn read-spool [port path actor-key log-key now]
  (when (path-exists? path)
    (require-regular-state! path)
    (let [value
          (try
            (exact-edn (bounded-file-bytes path))
            (catch Exception _ ::invalid))
          current-version
          (when-not (= ::invalid value)
            (north.coord/cur-ver port))]
      (if (and (not= ::invalid value)
               (valid-spool? value actor-key log-key current-version now))
        value
        (do (delete-state! path) nil)))))

(defn page-spool [page actor-key log-key now]
  (let [ids (->> (:messages page)
                 (filter valid-message-id?)
                 vec)]
    {:schema spool-schema
     :actor-key actor-key
     :log-key log-key
     :snapshot-version (:version page)
     :created-at-ms now
     :ids ids
     ;; This is copied verbatim from Fram. Never derive a cursor from an ID.
     :next (when (:more page) (:next page))}))

(defn persist-spool! [path spool]
  (if (or (seq (:ids spool)) (:next spool))
    (atomic-write! path spool)
    (delete-state! path)))

(defn before-deadline? [deadline]
  (< (System/nanoTime) deadline))

(defn rendered-message [from subject body]
  (when-not (or (north.message-contract/sender-problem from)
                (north.message-contract/subject-problem subject)
                (north.message-contract/body-problem body))
    (str "✉ from " from " — " subject "\n"
         "  " body "\n")))

(defn exact-delivery! [port recipient message output-bytes deadline]
  (cond
    (not (valid-message-id? message))
    {:consumed true :emitted false :output-bytes output-bytes}

    (not (before-deadline? deadline))
    {:consumed false :emitted false :output-bytes output-bytes}

    :else
    (if-let [claim
             (north.message-audience/claim-delivery! port message recipient)]
      (try
        (let [to (one port message "to")
              from (one port message "from")
              subject (one port message "subject")
              body (one port message "body")
              deliverable
              (and
               (string? to)
               (north.message-audience/deliverable?
                port message to recipient #{recipient}))
              rendered (when deliverable
                         (rendered-message from subject body))
              rendered-bytes (when rendered (utf8-bytes rendered))]
          (cond
            (not (before-deadline? deadline))
            (do
              (north.message-audience/release-delivery-claim! port claim)
              {:consumed false :emitted false :output-bytes output-bytes})

            (or (nil? rendered)
                (> rendered-bytes output-byte-limit)
                (> (+ output-bytes rendered-bytes) output-byte-limit))
            (do
              (north.message-audience/release-delivery-claim! port claim)
              {:consumed true :emitted false :output-bytes output-bytes})

            :else
            (do
              (print rendered)
              ;; Output is the irreversible side effect. Complete and flush the
              ;; whole bounded rendering before the graph acknowledgement.
              (flush)
              (north.message-audience/complete-delivery!
               port message recipient claim)
              {:consumed true
               :emitted true
               :output-bytes (+ output-bytes rendered-bytes)})))
        (catch Exception error
          (north.message-audience/release-delivery-claim! port claim)
          (throw error)))
      ;; Another healthy consumer owns the exact graph claim (or the ID has
      ;; already settled). Drop only this local hint; the graph retains it if the
      ;; foreign claimant dies before acknowledgement.
      {:consumed true :emitted false :output-bytes output-bytes})))

(defn consume-spool [port recipient spool deadline]
  (loop [remaining (:ids spool)
         inspected 0
         emitted 0
         output-bytes 0]
    (if (or (empty? remaining)
            (>= inspected candidate-limit)
            (>= emitted delivery-limit)
            (not (before-deadline? deadline)))
      [(assoc spool :ids (vec remaining)) emitted]
      (let [result
            (exact-delivery!
             port recipient (first remaining) output-bytes deadline)]
        (if (:consumed result)
          (recur (next remaining)
                 (inc inspected)
                 (if (:emitted result) (inc emitted) emitted)
                 (:output-bytes result))
          [(assoc spool :ids (vec remaining)) emitted])))))

(defn try-lock! [channel deadline]
  (loop []
    (let [lock
          (try
            (.tryLock channel)
            ;; Babashka does not expose OverlappingFileLockException as a
            ;; resolvable catch class, so discriminate it without swallowing
            ;; unrelated I/O failures.
            (catch Exception error
              (if (= "OverlappingFileLockException"
                     (.getSimpleName (class error)))
                nil
                (throw error))))]
      (cond
        lock lock
        (before-deadline? deadline)
        (do (Thread/sleep 2) (recur))
        :else nil))))

(defn with-state-lock [lock-path deadline f]
  (require-regular-state! lock-path)
  (when-not (path-exists? lock-path)
    (try
      (java.nio.file.Files/createFile
       lock-path
       (into-array
        java.nio.file.attribute.FileAttribute
        [(java.nio.file.attribute.PosixFilePermissions/asFileAttribute
          file-permissions)]))
      (catch java.nio.file.FileAlreadyExistsException _ nil)))
  (require-regular-state! lock-path)
  (with-open [channel
              (java.nio.channels.FileChannel/open
               lock-path
               (into-array java.nio.file.OpenOption
                           [java.nio.file.StandardOpenOption/WRITE]))]
    (when-let [_lock (try-lock! channel deadline)]
      ;; Closing CHANNEL at the with-open boundary releases the FileLock. This
      ;; also covers exceptions and process death, and avoids depending on the
      ;; concrete sun.nio FileLock implementation in Babashka's SCI allowlist.
      (f))))

(defn run-peek! [port me]
  (let [started (System/nanoTime)
        hard-deadline (+ started (* hook-work-budget-ms 1000000))
        recipient (north.message-audience/bare-handle me)
        actor-key (managed-actor-key me)
        log-key (canonical-log-key)
        state-root
        (.toPath
         (io/file (or (System/getenv "XDG_RUNTIME_DIR") "/tmp")
                  "north-inbox-peek"))
        _ (secure-directory! state-root)
        spool-path (.resolve state-root actor-key)
        lock-path (.resolve state-root (str actor-key ".lock"))
        lock-deadline
        (min hard-deadline
             (+ (System/nanoTime) (* lock-wait-ms 1000000)))]
    (with-state-lock
      lock-path lock-deadline
      (fn []
        (let [now (System/currentTimeMillis)
              cached (read-spool port spool-path actor-key log-key now)
              spool
              (if (seq (:ids cached))
                cached
                (let [page
                      (north.message-audience/pending-message-page
                       port recipient #{recipient} spool-page-limit (:next cached))
                      fresh (page-spool page actor-key log-key now)]
                  ;; A fresh page is still only a hint. Deferring its single
                  ;; durable write until after delivery leaves more of the
                  ;; 900ms foreground budget for first output; a crash before
                  ;; that write loses no authority because the graph was never
                  ;; replaced by the spool.
                  fresh))
              delivery-deadline
              (min hard-deadline
                   (+ (System/nanoTime)
                      (* output-time-budget-ms 1000000)))
              [remaining _]
              (consume-spool port recipient spool delivery-deadline)]
          (persist-spool! spool-path remaining))))))

(when-not (= "1" (System/getProperty "north.inbox-peek.lib"))
  (let [[port me] *command-line-args*]
    (run-peek! (Integer/parseInt port) me)))
