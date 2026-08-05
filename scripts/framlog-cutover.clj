#!/usr/bin/env bb
;; One operation only: seal the fenced North logs, convert them to canonical
;; FRAMLOG, and publish the exact already-built Fram server selection.
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(import '[java.io FileOutputStream]
        '[java.net InetSocketAddress Socket]
        '[java.nio ByteBuffer ByteOrder]
        '[java.nio.channels FileChannel]
        '[java.nio.charset CodingErrorAction StandardCharsets]
        '[java.nio.file Files LinkOption Path StandardCopyOption StandardOpenOption]
        '[java.security MessageDigest]
        '[java.util Arrays])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))
(def state-home (str (System/getProperty "user.home") "/.local/state/north"))
(def exact-fram-revision "1f305c9c9258b2719d45ab187cb223fd5369f87d")
(def exact-fram-tree "7d0786c5ca271b1c76672ab40dea701b3b455ef4")
(def exact-artifact-sha256
  "082798836f7f3ec84e2b7ac083b60ae0ffa78e0b79d123de505798f8d5323b86")
(def exact-artifact
  (str state-home "/fram-artifacts/graal/" exact-fram-revision "/"
       exact-artifact-sha256 "/fram-server-graal"))
(def exact-fram-source "/home/tom/code/fram/main")
(def migration-encoding
  {:encoding :deflate :framlog-version 1 :framlog-flags 1})
(def spaces {:coordination "north-coordination" :telemetry "north-telemetry"})
(def ports {:coordination 7977 :telemetry 7978})
(def production-config
  {:operation-directory (str state-home "/fram-cutovers/" exact-fram-revision)
   :selector-path (str state-home "/framrpc.env")
   :sources {:coordination "/home/tom/code/north-data/coordination.log"
             :telemetry "/home/tom/code/north-data/telemetry.log"}
   :spaces spaces
   :ports ports
   :fram-source exact-fram-source
   :fram-revision exact-fram-revision
   :fram-tree exact-fram-tree
   :artifact exact-artifact
   :artifact-sha256 exact-artifact-sha256})

(defn fail! [message data]
  (throw (ex-info message data)))

(defn sha256-bytes [^bytes bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
    (apply str (map #(format "%02x" (bit-and (int %) 255)) digest))))

(defn sha256-file [path]
  (sha256-bytes (Files/readAllBytes (.toPath (io/file path)))))

(defn required-file! [label path]
  (let [file (.getCanonicalFile (io/file (or path "")))]
    (when-not (.isFile file)
      (fail! (str label " is unavailable") {:path (.getPath file)}))
    (.getPath file)))

(defn required-executable! [label path]
  (let [canonical (required-file! label path)
        file (io/file canonical)]
    (when-not (.canExecute file)
      (fail! (str label " is not executable") {:path canonical}))
    canonical))

(defn fingerprint! [label path]
  (let [canonical (required-file! label path)
        before (.length (io/file canonical))
        digest (sha256-file canonical)
        after (.length (io/file canonical))]
    (when-not (= before after)
      (fail! (str label " changed while it was fingerprinted")
             {:path canonical :before before :after after}))
    {:path canonical :bytes after :sha256 digest}))

(defn fsync-directory! [^Path directory]
  (with-open [channel
              (FileChannel/open
               directory
               (into-array java.nio.file.OpenOption [StandardOpenOption/READ]))]
    (.force channel true)))

(defn path-entry-exists? [^Path path]
  (Files/exists path (into-array LinkOption [LinkOption/NOFOLLOW_LINKS])))

(defn durable-atomic-write! [path content]
  (let [target (.toPath (io/file path))
        parent (.getParent target)]
    (when-not parent
      (fail! "durable publication requires a parent directory" {:path path}))
    (Files/createDirectories parent
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (when (path-entry-exists? target)
      (fail! "durable one-shot publication refuses an existing path" {:path path}))
    (let [tmp
          (Files/createTempFile
           parent (str "." (.getFileName target) "-") ".tmp"
           (make-array java.nio.file.attribute.FileAttribute 0))]
      (try
        (with-open [stream (FileOutputStream. (.toFile tmp))]
          (.write stream (.getBytes (str content) StandardCharsets/UTF_8))
          (.flush stream)
          (.force (.getChannel stream) true))
        (Files/move tmp target
                    (into-array StandardCopyOption
                                [StandardCopyOption/ATOMIC_MOVE]))
        (fsync-directory! parent)
        path
        (finally
          (Files/deleteIfExists tmp))))))

(defn durable-edn! [path value]
  (durable-atomic-write! path (str (pr-str value) "\n")))

(defn process-result [arguments directory environment]
  (let [builder (ProcessBuilder. ^java.util.List (mapv str arguments))
        process-environment (.environment builder)]
    (.directory builder (io/file directory))
    (doseq [[key value] environment]
      (.put process-environment (name key) (str value)))
    (let [process (.start builder)
          stdout (future (slurp (.getInputStream process)))
          stderr (future (slurp (.getErrorStream process)))
          exit (.waitFor process)]
      {:exit exit :stdout @stdout :stderr @stderr})))

(defn process! [label arguments directory environment]
  (let [result (process-result arguments directory environment)]
    (when-not (zero? (:exit result))
      (fail! (str label " failed")
             {:arguments arguments :exit (:exit result)
              :stdout (:stdout result) :stderr (:stderr result)}))
    result))

(defn git-value! [source & arguments]
  (str/trim
   (:stdout
    (process! "read exact Fram source identity"
              (into ["git" "-C" source] arguments) root {}))))

(defn exact-contract! [config]
  (let [source (.getCanonicalPath (io/file (:fram-source config)))
        revision (git-value! source "rev-parse" "--verify" "HEAD")
        tree (git-value! source "rev-parse" "--verify" "HEAD^{tree}")
        published (git-value! source "rev-parse" "--verify" "origin/main")
        dirty (git-value! source "status" "--porcelain" "--untracked-files=all")
        selected-artifact (:artifact config)
        _ (when-not (.isAbsolute (io/file selected-artifact))
            (fail! "exact Graal artifact path must be absolute"
                   {:path selected-artifact}))
        artifact (required-executable! "exact Graal artifact" selected-artifact)
        artifact-sha256 (sha256-file artifact)
        migrator (required-executable!
                  "exact Fram converter" (str source "/bin/fram-migrate-triple-log"))
        launcher (required-executable! "exact Fram server" (str source "/bin/fram-server"))
        out (.getCanonicalFile (io/file source "out"))]
    (when-not (= (:fram-revision config) revision published)
      (fail! "Fram source is not the exact published cutover revision"
             {:expected (:fram-revision config) :head revision :origin-main published}))
    (when-not (= (:fram-tree config) tree)
      (fail! "Fram source tree differs from the exact cutover tree"
             {:expected (:fram-tree config) :actual tree}))
    (when-not (str/blank? dirty)
      (fail! "exact Fram source must be clean"
             {:path source :changes (str/split-lines dirty)}))
    (when-not (= (:artifact-sha256 config) artifact-sha256)
      (fail! "Graal artifact differs from the exact cutover artifact"
             {:path artifact :expected (:artifact-sha256 config)
              :actual artifact-sha256}))
    (when-not (.isDirectory out)
      (fail! "exact Fram generated output is unavailable" {:path (.getPath out)}))
    {:source {:path source :revision revision :tree tree
              :converter (fingerprint! "exact Fram converter" migrator)
              :server (fingerprint! "exact Fram server" launcher)
              :out (.getPath out)}
     :artifact {:path selected-artifact :canonical-path artifact
                :sha256 artifact-sha256
                :bytes (.length (io/file artifact))}}))

(defn port-open? [port]
  (try
    (with-open [socket (Socket.)]
      (.connect socket (InetSocketAddress. "127.0.0.1" port) 150)
      true)
    (catch Throwable _ false)))

(defn require-fenced! [config]
  (doseq [[store port] (:ports config)]
    (when (port-open? port)
      (fail! "Fram writer must already be fenced before the one-shot conversion"
             {:store store :port port})))
  (when (path-entry-exists? (.toPath (io/file (:selector-path config))))
    (fail! "canonical Fram selection already exists; the one-shot operation refuses it"
           {:path (:selector-path config)}))
  (doseq [[store path] (:sources config)]
    (required-file! (str (name store) " source log") path))
  config)

(defn force-file! [path]
  (with-open [channel
              (FileChannel/open
               (.toPath (io/file path))
               (into-array java.nio.file.OpenOption [StandardOpenOption/WRITE]))]
    (.force channel true)))

(defn seal-copy! [label source target]
  (let [before (fingerprint! (str label " source") source)
        target-file (io/file target)]
    (when (.exists target-file)
      (fail! (str label " sealed copy already exists") {:path target}))
    (Files/copy (.toPath (io/file (:path before))) (.toPath target-file)
                (into-array StandardCopyOption []))
    (force-file! target-file)
    (fsync-directory! (.getParent (.toPath target-file)))
    (let [sealed (fingerprint! (str label " sealed copy") target)
          after (fingerprint! (str label " source") source)]
      (when-not (and (= before after)
                     (= (select-keys before [:bytes :sha256])
                        (select-keys sealed [:bytes :sha256])))
        (fail! (str label " source changed while its preimage was sealed")
               {:before before :after after :sealed sealed}))
      {:source before :sealed sealed})))

(defn seal-preimage! [config directory contract]
  (let [preimage-dir (str directory "/preimage")]
    (Files/createDirectories (.toPath (io/file preimage-dir))
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (let [logs
          (into (sorted-map)
                (for [[store source] (:sources config)]
                  [store (seal-copy! (name store) source
                                     (str preimage-dir "/" (name store) ".log"))]))
          coordinate
          {:format :north/fram-cutover-preimage-v1
           :operation (:fram-revision config)
           :selector {:path (.getCanonicalPath (io/file (:selector-path config)))
                      :state :absent}
           :fram contract
           :logs logs}
          path (str directory "/preimage.edn")]
      (durable-edn! path coordinate)
      {:path path :fingerprint (fingerprint! "sealed preimage coordinate" path)
       :value coordinate})))

(defn normalize-one! [directory store sealed]
  (let [migration-dir (str directory "/migrated")
        target (str migration-dir "/" (name store) ".normalized.log")
        bb (or (System/getenv "NORTH_BB") "bb")]
    (Files/createDirectories (.toPath (io/file migration-dir))
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (let [result
          (edn/read-string
           (:stdout
            (process! (str (name store) " normalization")
                      [bb (str root "/scripts/framlog-normalize.clj")
                       (:path sealed) target]
                      root {})))
          receipt (fingerprint! (str (name store) " normalization receipt")
                                (str target ".normalization.edn"))
          output (fingerprint! (str (name store) " normalized log") target)]
      (when-not (and (= sealed (select-keys (:source result) [:path :bytes :sha256]))
                     (= output (select-keys (:output result) [:path :bytes :sha256])))
        (fail! "normalization did not seal its exact input and output"
               {:store store :result result :sealed sealed :output output}))
      {:result result :receipt receipt :output output})))

(defn strict-utf8 [^bytes bytes]
  (str
   (.decode
    (doto (.newDecoder StandardCharsets/UTF_8)
      (.onMalformedInput CodingErrorAction/REPORT)
      (.onUnmappableCharacter CodingErrorAction/REPORT))
    (ByteBuffer/wrap bytes))))

(defn framlog-header! [path]
  (let [bytes (Files/readAllBytes (.toPath (io/file path)))
        magic (.getBytes "FRAMLOG\u0000" StandardCharsets/UTF_8)]
    (when (< (alength bytes) 16)
      (fail! "FRAMLOG header is truncated" {:path path :bytes (alength bytes)}))
    (when-not (Arrays/equals magic (Arrays/copyOfRange bytes 0 8))
      (fail! "FRAMLOG magic is absent" {:path path}))
    (let [buffer (doto (ByteBuffer/wrap bytes) (.order ByteOrder/LITTLE_ENDIAN))
          version (bit-and 0xffff (.getShort buffer 8))
          flags (bit-and 0xffff (.getShort buffer 10))
          space-length (Integer/toUnsignedLong (.getInt buffer 12))
          end (+ 16 space-length)]
      (when (or (zero? space-length) (> space-length 4096) (> end (alength bytes)))
        (fail! "FRAMLOG SpaceId header is invalid"
               {:path path :space-bytes space-length :file-bytes (alength bytes)}))
      {:version version :flags flags
       :space-id (strict-utf8 (Arrays/copyOfRange bytes 16 (int end)))})))

(defn valid-migration?
  [space manifest normalized output header]
  (and (= "fram-triple-log-migration-manifest/v1" (:format manifest))
       (= space (:space-id manifest) (:space-id header))
       (= normalized (select-keys (:source manifest) [:path :bytes :sha256]))
       (= (select-keys output [:bytes :sha256])
          (select-keys (:output manifest) [:bytes :sha256]))
       (= migration-encoding
          (select-keys (:output manifest)
                       [:encoding :framlog-version :framlog-flags]))
       (= {:version 1 :flags 1 :space-id space} header)
       (zero? (get-in manifest [:summary :diagnostic-count] -1))
       (nil? (:torn-tail manifest))))

(defn migrate-one! [config directory contract store sealed]
  (let [normalization (normalize-one! directory store sealed)
        normalized (:output normalization)
        target (str directory "/migrated/" (name store) ".framlog")
        manifest-path (str target ".migration.edn")
        clj-config (str directory "/clj-config")
        converter (get-in contract [:source :converter :path])
        space (get-in config [:spaces store])]
    (Files/createDirectories (.toPath (io/file clj-config))
                             (make-array java.nio.file.attribute.FileAttribute 0))
    (process! (str (name store) " Deflate FRAMLOG conversion")
              [converter "--deflate" (:path normalized) space target]
              (:fram-source config) {:CLJ_CONFIG clj-config})
    (let [manifest-file (fingerprint! (str (name store) " migration manifest")
                                      manifest-path)
          manifest (edn/read-string (slurp manifest-path))
          output (fingerprint! (str (name store) " FRAMLOG") target)
          header (framlog-header! target)]
      (when-not (valid-migration? space manifest normalized output header)
        (fail! "converted FRAMLOG failed flags, hash, SpaceId, or diagnostic verification"
               {:store store :space space :manifest manifest
                :normalized normalized :output output :header header}))
      {:space-id space :normalization normalization :manifest manifest
       :manifest-file manifest-file :output output :header header})))

(defn shell-quote [value]
  (str "'" (str/replace (str value) "'" "'\"'\"'") "'"))

(defn selector-content [config contract migrations]
  (let [source (get-in contract [:source :path])
        out (get-in contract [:source :out])]
    (str "# Exact Fram main selection for the canonical FRAMRPC/FRAMLOG cutover.\n"
         "export FRAM_HOME=" (shell-quote source) "\n"
         "export FRAM_BIN=" (shell-quote (str source "/bin")) "\n"
         "export FRAM_OUT=" (shell-quote out) "\n"
         "export NORTH_FRAMRPC_OUT=" (shell-quote out) "\n"
         "export FRAM_SERVER_RUNTIME='graal'\n"
         "export FRAM_GRAAL_ARTIFACT=" (shell-quote (get-in contract [:artifact :path])) "\n"
         "export FRAM_SPACE_ID='north-coordination'\n"
         "export NORTH_TELEMETRY_SPACE_ID='north-telemetry'\n"
         "export NORTH_TELEMETRY_PARTITION='1'\n"
         "export NORTH_PORT='7977'\n"
         "export FRAM_SERVER_PORT='7977'\n"
         "export NORTH_TELEMETRY_PORT='7978'\n"
         "export FRAM_LOG="
         (shell-quote (get-in migrations [:coordination :output :path])) "\n"
         "export FRAM_TELEMETRY_LOG="
         (shell-quote (get-in migrations [:telemetry :output :path])) "\n")))

(defn publish-selection! [config directory contract preimage migrations]
  (require-fenced! config)
  (let [current-contract (exact-contract! config)
        _ (when-not (= contract current-contract)
            (fail! "exact Fram source or artifact changed during conversion"
                   {:sealed contract :current current-contract}))
        content (selector-content config contract migrations)
        selector (:selector-path config)
        receipt
        {:format :north/fram-forward-cutover-v1
         :operation (:fram-revision config)
         :preimage-coordinate (:fingerprint preimage)
         :fram contract
         :migration-encoding migration-encoding
         :migrations migrations
         :selection {:path (.getCanonicalPath (io/file selector))
                     :sha256 (sha256-bytes (.getBytes content StandardCharsets/UTF_8))}}
        receipt-path (str directory "/cutover.edn")]
    (durable-edn! receipt-path receipt)
    (durable-atomic-write! selector content)
    (let [selected (fingerprint! "canonical Fram selection" selector)]
      (when-not (= (get-in receipt [:selection :sha256]) (:sha256 selected))
        (fail! "published Fram selection differs from its sealed receipt"
               {:receipt receipt :selected selected}))
      {:receipt (fingerprint! "forward cutover receipt" receipt-path)
       :selector selected})))

(defn run-one-shot! [config]
  (let [directory (.getCanonicalPath (io/file (:operation-directory config)))]
    (when (path-entry-exists? (.toPath (io/file directory)))
      (fail! "cutover operation directory already exists; the one-shot operation refuses it"
             {:directory directory}))
    (require-fenced! config)
    (let [contract (exact-contract! config)]
      (let [operation-path (.toPath (io/file directory))
            parent (.getParent operation-path)]
        (Files/createDirectories parent
                                 (make-array java.nio.file.attribute.FileAttribute 0))
        (Files/createDirectory operation-path
                               (make-array java.nio.file.attribute.FileAttribute 0))
        (fsync-directory! parent))
      (let [preimage (seal-preimage! config directory contract)
            sealed (get-in preimage [:value :logs])
            migrations
            (into (sorted-map)
                  (for [store [:coordination :telemetry]]
                    [store (migrate-one! config directory contract store
                                         (get-in sealed [store :sealed]))]))
            selected (publish-selection! config directory contract preimage migrations)]
        (println
         (pr-str {:ok true :operation (:fram-revision config)
                  :preimage-coordinate (:fingerprint preimage)
                  :receipt (:receipt selected) :selector (:selector selected)}))
        selected))))

(def main-marker "\n(try\n  (if")

(try
  (if (seq *command-line-args*)
    (fail! "framlog-cutover takes no arguments; it is the exact one-shot operation" {})
    (run-one-shot! production-config))
  (catch Throwable error
    (binding [*out* *err*]
      (prn {:error :fram-forward-cutover-failed
            :message (.getMessage error) :data (ex-data error)}))
    (System/exit 1)))
