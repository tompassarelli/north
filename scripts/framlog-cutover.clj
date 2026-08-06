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
(def exact-fram-revision "668515f23e490d59dbb14aef2a89ac9646d987d7")
(def exact-fram-tree "a712566e805bd8d3a632ced269f72c9b8a064d91")
(def exact-native-closure-sha256
  "508bedc10caa5131765454db7b905283db342f78bb27c8005c3666fb8cdecdea")
(def exact-native-artifact-dir
  (str "/home/tom/.cache/fram/"
       "native-build-core-target-production-5484b79a-668515f/"
       exact-native-closure-sha256))
(def exact-fram-source "/home/tom/code/fram/wt-core-target-production-668515f")

;; Inert transcription of everything fram-native-build's input.manifest digests:
;; without it the closure hash above cannot be re-derived from source.
(def exact-native-build-command
  {:builder (str exact-fram-source "/bin/fram-native-build")
   :arguments ["--host" "server"]
   :environment
   {"FRAM_NATIVE_CACHE"
    (str "/home/tom/.cache/fram/"
         "native-build-core-target-production-5484b79a-668515f")}
   :working-directory exact-fram-source
   :source-list (str exact-fram-source "/native/core_closure_sources.txt")})
(def exact-native-build-identity
  {:builder-sha256
   "ae938f8f2db6df6c819de37f3d5be17fa4045784482745841b32278276275948"
   :beagle
   {:realpath "/home/tom/code/beagle/main/bin/beagle"
    :identity
    (str "git:5484b79a00bb7d1ef459858a3a45176cd1ef77ce"
         ":build-core-"
         "a8f6cac1f749d0a22ee8a9a8bed3e2e2ade9126a8fb579402a1febfab83f32b4")}
   :cc
   {:realpath
    "/nix/store/xcnqqnhw9hb4j5rjgds2yjryi8qki5f3-gcc-wrapper-15.2.0/bin/gcc"
    :version-line "gcc (GCC) 15.2.0"}
   :link "dynamic"})
(def exact-native-program
  {:scope "fram-native-server"
   :configuration "profile=3"
   :closure-sha256
   "af2a1db5fe5e9d46c0ce330cfbdd67978de1632895e2c0d76c11919ebc290dfc"
   :digest
   "ac197f1cfbb47af2c8c0eba0eb49496a6355b8577c43ddc83d7c36d09cc9837c"
   :entries
   ["fram.native-server/server-generated-abi"
    "fram.native-server/server-store-boot!"
    "fram.native-server/server-store-dispatch!"
    "fram.native-server/server-store-shutdown"
    "fram.native-server/server-codec-read-request!"
    "fram.native-server/server-codec-write-response!"
    "fram.native-server/server-codec-release-request"
    "fram.native-server/server-codec-release-response"]})
;; Dependency-first and index-significant: the manifest digests this order.
(def exact-native-closure-sources
  [["src/fram/types.bgl"
    "261c4ff4360f01ef3f2457aa06d9418f3a8de2974ea3ec7bbb6e286cae144770"]
   ["src/fram/store.bgl"
    "a0ab9c0383d9a164a199fc7a3b3332f034b119cd4fafc6631549e5edb6a013a4"]
   ["src/fram/rotation.bgl"
    "6173a5ee5d859d2c01abd53f35f16811d0048568b9090f5db602a137a748769e"]
   ["src/fram/txn.bgl"
    "5efbd7fc4349d2250130b3981657a4ce5c1698e62d4370e952a5b12ba3f42787"]
   ["src/fram/schema.bgl"
    "7d4046c89f77e86d99e15a1adf32deedda90114d1fa5c188c4736358a57a7537"]
   ["src/fram/kernel.bgl"
    "db8025377776873cae0dc6cc07c12e3eced629bc587f100aaa07ca4f303eebda"]
   ["src/fram/text_index.bgl"
    "6ff59b4e9bef1636fc1a671c2120f6657577b0d9887c504628a91cbe1ef05c3d"]
   ["src/fram/text_search.bgl"
    "ff431b18ecb2bbe3a7eb672bbd639b528f216996e4ffb4bf39c2b222b2a2072e"]
   ["src/fram/datalog.bgl"
    "47170543f7c03718de0de5ab6f4ae952e0331ecbe9e768e07d8f41b93c4ca0ed"]
   ["src/fram/query.bgl"
    "1a0ab7b943fcd3f26cc82c9ec6311678c79b7771573b2f42e27f8f2d1e20cfff"]
   ["src/fram/native_wire_codec.bgl"
    "237182794573011034fd1921e90dce5c7156e19d63c64be6e84afb831c0773c9"]
   ["src/commit_plan.bgl"
    "f1e4ad6aa4c8966103309fb30100209844aebb0224477e68ca5f7ec09f1bf770"]
   ["src/fram/log_codec.bgl"
    "068515170892255a3159aa0410080276a7eea375135bd2b55ab9232b3b18411b"]
   ["src/fram/native_lease_ops.bgl"
    "09e045d3159e2bb33e49a27de7bf5d708c4a59e5557960920824454b8ab45b9c"]
   ["src/fram/native_query_ops.bgl"
    "fa2d8605c9b4d087b789abc97d1123351c6d2f47fd02cdb2f35c7ce5610b4499"]
   ["src/fram/native_dispatch.bgl"
    "b764c395a15da84a9a2f57bdf023776aa9faa587b6168b533cd6a2576d896ae9"]
   ["src/fram/native_server.bgl"
    "423c8ed57068758dcaeeb8753cea22e899f1abce348adf118aec42a9a735c152"]])
(def exact-native-host-sources
  [["native/server_host.c"
    "a7dd027c974478576733b4ba68065178057babd2bff8d4bd3f35ff58016fc6bd"]
   ["native/server_host.h"
    "149650a96f65a5a595740ea28873b35103f31f37f36ec7dc3043a055ac2d6b36"]
   ["native/server_generated.c"
    "007888f3a6b3dc5b245f146987bbf71286c4a93f4da2b58b3ea22ac134235a0c"]])
(def exact-native-closure-provenance
  {:build exact-native-build-command
   :identity exact-native-build-identity
   :native-program exact-native-program
   :sources exact-native-closure-sources
   :host-sources exact-native-host-sources})
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
   :native-artifact-dir exact-native-artifact-dir
   :native-closure-sha256 exact-native-closure-sha256
   :native-closure-provenance exact-native-closure-provenance})

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
        dirty (git-value! source "status" "--porcelain" "--untracked-files=all")
        selected-artifact-dir (:native-artifact-dir config)
        _ (when-not (.isAbsolute (io/file selected-artifact-dir))
            (fail! "exact Native artifact directory must be absolute"
                   {:path selected-artifact-dir}))
        artifact-dir (.getCanonicalFile (io/file selected-artifact-dir))
        _ (when-not (.isDirectory artifact-dir)
            (fail! "exact Native artifact directory is unavailable"
                   {:path (.getPath artifact-dir)}))
        closure-sha256 (:native-closure-sha256 config)
        _ (when-not (and (string? closure-sha256)
                         (re-matches #"[0-9a-f]{64}" closure-sha256))
            (fail! "exact Native closure SHA-256 is invalid"
                   {:sha256 closure-sha256}))
        ready (fingerprint! "exact Native READY receipt"
                            (str (.getPath artifact-dir) "/READY"))
        ready-line (slurp (:path ready))
        input-manifest (fingerprint! "exact Native input manifest"
                                     (str (.getPath artifact-dir) "/input.manifest"))
        native-server
        (fingerprint!
         "exact Native Fram server"
         (required-executable!
          "exact Native Fram server"
          (str (.getPath artifact-dir) "/bin/fram-server-native")))
        migrator (required-executable!
                  "exact Fram converter" (str source "/bin/fram-migrate-triple-log"))
        launcher (required-executable! "exact Fram server" (str source "/bin/fram-server"))
        out (.getCanonicalFile (io/file source "out"))]
    (when-not (= (:fram-revision config) revision)
      (fail! "Fram source is not the exact frozen cutover revision"
             {:expected (:fram-revision config) :head revision}))
    (when-not (= (:fram-tree config) tree)
      (fail! "Fram source tree differs from the exact cutover tree"
             {:expected (:fram-tree config) :actual tree}))
    (when-not (str/blank? dirty)
      (fail! "exact Fram source must be clean"
             {:path source :changes (str/split-lines dirty)}))
    (when-not (= (str "fram-native-build/v1 " closure-sha256 "\n") ready-line)
      (fail! "Native READY receipt differs from the exact closure"
             {:path (:path ready) :closure-sha256 closure-sha256
              :content ready-line}))
    (when-not (= closure-sha256 (:sha256 input-manifest))
      (fail! "Native input manifest differs from the READY closure"
             {:path (:path input-manifest) :expected closure-sha256
              :actual (:sha256 input-manifest)}))
    (when-not (.isDirectory out)
      (fail! "exact Fram generated output is unavailable" {:path (.getPath out)}))
    {:source {:path source :revision revision :tree tree
              :converter (fingerprint! "exact Fram converter" migrator)
              :server (fingerprint! "exact Fram server" launcher)
              :out (.getPath out)}
     :native-artifact
     {:directory (.getPath artifact-dir)
      :closure-sha256 closure-sha256
      :ready ready
      :input-manifest input-manifest
      :server native-server}}))

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
    (str "# Exact frozen Fram selection for the canonical FRAMRPC/FRAMLOG cutover.\n"
         "export FRAM_HOME=" (shell-quote source) "\n"
         "export FRAM_BIN=" (shell-quote (str source "/bin")) "\n"
         "export FRAM_OUT=" (shell-quote out) "\n"
         "export NORTH_FRAMRPC_OUT=" (shell-quote out) "\n"
         "export FRAM_SERVER_RUNTIME='native'\n"
         "export FRAM_NATIVE_ARTIFACT_DIR="
         (shell-quote (get-in contract [:native-artifact :directory])) "\n"
         "export FRAM_NATIVE_CLOSURE_SHA256="
         (shell-quote (get-in contract [:native-artifact :closure-sha256])) "\n"
         "export FRAM_SERVER_ARTIFACT="
         (shell-quote (get-in contract [:native-artifact :server :path])) "\n"
         "export FRAM_SERVER_ARTIFACT_SHA256="
         (shell-quote (get-in contract [:native-artifact :server :sha256])) "\n"
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
