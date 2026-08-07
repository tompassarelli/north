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
(def exact-fram-revision "6da2eb6279a857b098f8b7e45c95aa3d3c9b4a30")
(def exact-fram-tree "9a42e1f2510e01af9ceb613d9ecd83b3823bf609")
(def exact-native-closure-sha256
  "f3709fe81a73af436bf5a1065c06a97f510ab5cf02d47a27847104d0d15c43b9")
(def exact-native-artifact-dir
  (str "/home/tom/.cache/fram/"
       "native-build-core-target-production-201ec968-6da2eb6/"
       exact-native-closure-sha256))
(def exact-fram-source "/home/tom/code/fram/wt-core-target-production-6da2eb6")
;; The one-shot shells these two out of the frozen source, so their bytes are
;; part of the pin, not an incidental property of the checkout.
(def exact-fram-converter
  {:bytes 471
   :sha256 "bb6d6a023d08617a9e23d9fc077e140e0c9b8e2b7f703810c7034645b54d50a4"})
(def exact-fram-server
  {:bytes 8000
   :sha256 "9712c1ef0bdabee634b37795bc77a9afb46cabf917e8c72b4afb71c76e0b19a2"})

;; Inert transcription of everything fram-native-build's input.manifest digests:
;; without it the closure hash above cannot be re-derived from source.
(def exact-native-build-command
  {:builder (str exact-fram-source "/bin/fram-native-build")
   :arguments ["--host" "server"]
   :environment
   {"FRAM_NATIVE_CACHE"
    (str "/home/tom/.cache/fram/"
         "native-build-core-target-production-201ec968-6da2eb6")}
   :working-directory exact-fram-source
   :source-list (str exact-fram-source "/native/core_closure_sources.txt")})
(def exact-native-build-identity
  {:builder-sha256
   "c78946197a83bbeef57dbe15c3b98a777cd4af9562d0e5b87751479034476a0e"
   :beagle
   {:realpath "/home/tom/code/beagle/main/bin/beagle"
    :identity
    (str "git:201ec968ff688695378802d3998dad71e17baa6c"
         ":build-core-"
         "e97697233654145fb8b13a66666a1b47df3e60349a73ba7f13affe4c811417b3")}
   :cc
   {:realpath
    "/nix/store/xcnqqnhw9hb4j5rjgds2yjryi8qki5f3-gcc-wrapper-15.2.0/bin/gcc"
    :version-line "gcc (GCC) 15.2.0"}
   :link "dynamic"})
(def exact-native-program
  {:scope "fram-native-server"
   :configuration "profile=3"
   :abi "lp64"
   :closure-sha256
   "f7a5ccd43dd34d750af05115726525c9e499de7c0396b9503f9efe9dac44fb0d"
   :digest
   "0a0bf7114569afad8a69aaf4bceecc9be9a49eaa70d589a642489cf2ab62298f"
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
  [["src/fram/slots.bgl"
    "5ba91a6c86b887ea70584c6f66bc15e1ef0e5b2e98973f049dca7a464f305429"]
   ["src/fram/types.bgl"
    "b7152ace4e19f2a8c388e3a7f057a03eb1e6dec09b19943f72782a5c2a7efafc"]
   ["src/fram/store.bgl"
    "49fbed624852695995c29b3361d9d07607c50a99e9e5f1a77bd226d838ccb9de"]
   ["src/fram/rotation.bgl"
    "b70a00b3d90e4daa540c9ca05132c57119ad9e598da7ec29dfefc5c088d2d418"]
   ["src/fram/txn.bgl"
    "4f601e0bdcba00a7db0aab361c5da680ce9d43730d6201a78049a8aa76ea537a"]
   ["src/fram/schema.bgl"
    "6fd39b71f2887845fddb2d9cd75ab73255d4d691ddba8bfd5ae2a363b5b961a0"]
   ["src/fram/kernel.bgl"
    "db8025377776873cae0dc6cc07c12e3eced629bc587f100aaa07ca4f303eebda"]
   ["src/fram/text_index.bgl"
    "31f4630ebd0ef85947eec8c6e14b63a1783354dad2b3bffccc17de561dc02376"]
   ["src/fram/text_search.bgl"
    "6d3b2709d6d826f80d5e21d8a2df9b252458359f9746f6855c58d629807e3ed0"]
   ["src/fram/datalog.bgl"
    "0a002a1c20a9732286a326abd26c67186c0f74fac08d4092b52f8b01183d7172"]
   ["src/fram/query.bgl"
    "1298bc6a72efc043a85323b4a1308d305b160f287fcc2a44e1ee8b559f8f5e04"]
   ["src/fram/native_wire_codec.bgl"
    "4ae095727e466b5d52a6e290db520aa734afa6322990f028f3091f6d203e1d8f"]
   ["src/commit_plan.bgl"
    "8c3808d45c4664fc6fbbb3cc98742ebf4835f37449a8c8bc4daa4c58b783e0b3"]
   ["src/fram/log_codec.bgl"
    "dd38012896bea5b820907f85e8c9ccb8716858e2b934cc6081b0a146c39e8990"]
   ["src/fram/snapshot_codec.bgl"
    "9fa3ddbe910586965630e4f9cefd3b0684be76a98592917c50b1211679653bda"]
   ["src/fram/native_lease_ops.bgl"
    "b50047b437bd27f8a1fa689662503885a36174ff566facc71a2f4e51b6d6ebc5"]
   ["src/fram/native_query_ops.bgl"
    "9c33e649112a2d9b354a5e2d34c04b1750d62500f756719147e092d4fabb17a2"]
   ["src/fram/native_dispatch.bgl"
    "eef0c5ec09aa3e85e0278119c85c3f186ac33da29ea94c13f4d3389eb0a87913"]
   ["src/fram/native_server.bgl"
    "fa80f5067a133f7737cd1d324611f132b62c3286bc10268aeeea1f0e31565799"]])
(def exact-native-host-sources
  [["native/server_host.c"
    "ad135914ecd8950e63229a5bec7b65d2bab89edf7e3bce6c33299ad8bef3c73b"]
   ["native/server_host.h"
    "0522ea73947cfcdfb7285e99f323b2df48bf9ea884bea4c7f7eb21579c2e9d3c"]
   ["native/server_generated.c"
    "543319fb870d45fd6706f8be28ed6dc028308dc25765f21a4a25055df1e1067b"]])
(def exact-native-closure-provenance
  {:build exact-native-build-command
   :identity exact-native-build-identity
   :native-program exact-native-program
   :sources exact-native-closure-sources
   :host-sources exact-native-host-sources})
(def migration-encoding
  {:encoding :uncompressed :framlog-version 1 :framlog-flags 0})
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
   :fram-converter exact-fram-converter
   :fram-server exact-fram-server
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
        migrator (fingerprint!
                  "exact Fram converter"
                  (required-executable!
                   "exact Fram converter" (str source "/bin/fram-migrate-triple-log")))
        launcher (fingerprint!
                  "exact Fram server"
                  (required-executable!
                   "exact Fram server" (str source "/bin/fram-server")))
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
    (when-not (= (:fram-converter config)
                 (select-keys migrator [:bytes :sha256]))
      (fail! "exact Fram converter differs from the frozen cutover converter"
             {:expected (:fram-converter config) :actual migrator}))
    (when-not (= (:fram-server config) (select-keys launcher [:bytes :sha256]))
      (fail! "exact Fram server differs from the frozen cutover server"
             {:expected (:fram-server config) :actual launcher}))
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
              :converter migrator
              :server launcher
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
       (= {:version 1 :flags 0 :space-id space} header)
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
    (process! (str (name store) " uncompressed FRAMLOG conversion")
              [converter (:path normalized) space target]
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
