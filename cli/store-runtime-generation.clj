(ns north.store-runtime-generation
  (:require [babashka.process :as proc]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [north.store-runtime-manifest :as manifest])
  (:import [java.nio ByteBuffer]
           [java.nio.channels FileChannel]
           [java.nio.charset StandardCharsets]
           [java.nio.file CopyOption]
           [java.nio.file Files]
           [java.nio.file LinkOption]
           [java.nio.file OpenOption]
           [java.nio.file Path]
           [java.nio.file Paths]
           [java.nio.file StandardCopyOption]
           [java.nio.file StandardOpenOption]
           [java.nio.file.attribute BasicFileAttributes]
           [java.nio.file.attribute FileAttribute]
           [java.security MessageDigest]
           [java.util UUID]
           [java.util.concurrent TimeUnit]))

(def ^:private ^String north-root (.getCanonicalPath (io/file (.getParentFile (io/file *file*)) "..")))

(load-file (str north-root "/cli/runtime-attestation.clj"))

(def ^:private runtime-read-selection! (requiring-resolve 'north.runtime-attestation/read-selection!))

(def ^:private runtime-launch-spec! (requiring-resolve 'north.runtime-attestation/launch-spec!))

(def ^:private runtime-generation-evidence! (requiring-resolve 'north.runtime-attestation/active-generation-evidence!))

(def ^:private runtime-publish-record! (requiring-resolve 'north.runtime-attestation/publish-runtime-record!))

(def ^:private runtime-default-record-path (requiring-resolve 'north.runtime-attestation/default-runtime-record-path))

(def ^:private read-edn-with-readers! (requiring-resolve 'clojure.edn/read-string))

(defrecord JVMPackageObservation [output nar-sha256 manifest-sha256 manifest-text])

(defn jvmpackageobservation-output [r] (:output r))

(defn jvmpackageobservation-nar-sha256 [r] (:nar-sha256 r))

(defn jvmpackageobservation-manifest-sha256 [r] (:manifest-sha256 r))

(defn jvmpackageobservation-manifest-text [r] (:manifest-text r))

(defrecord SelectedGeneration [target root generation])

(defn selectedgeneration-target [r] (:target r))

(defn selectedgeneration-root [r] (:root r))

(defn selectedgeneration-generation [r] (:generation r))

(defrecord WrittenGeneration [id root target generation selection])

(defn writtengeneration-id [r] (:id r))

(defn writtengeneration-root [r] (:root r))

(defn writtengeneration-target [r] (:target r))

(defn writtengeneration-generation [r] (:generation r))

(defn writtengeneration-selection [r] (:selection r))

(defonce in-process-lock (Object.))

(def ^:dynamic *after-selector-move!* (fn [^WrittenGeneration _selection] nil))

(def ^:private ^String generation-file-name "generation.edn")

(def ^:private ^String client-file-name "client.env")

(def ^:private max-generation-bytes 32768)

(def ^:private ^String live-unit "north-coordinator.service")

(def ^:private status-timeout-ms 15000)

(def ^:private command-output-limit 65536)

(def ^:private runtime-selection-keys #{"BEAGLE_STORE_HOME" "BEAGLE_STORE_BIN" "BEAGLE_STORE_OUT" "NORTH_STORE_OUT" "BEAGLE_STORE_PACKAGED" "BEAGLE_STORE_SERVER_RUNTIME" "BEAGLE_STORE_SERVER_CLASSPATH_FILE" "BEAGLE_STORE_JAVA" "BEAGLE_STORE_SERVER_LOG" "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" "BEAGLE_STORE_SERVER_ARTIFACT" "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" "BEAGLE_STORE_SERVER_G1_REGION" "BEAGLE_STORE_SERVER_NO_OOM_EXIT"})

(def ^:private generation-readers {'north.store_runtime_manifest.StoreRuntimeManifest manifest/map->StoreRuntimeManifest 'north.store_runtime_manifest.JVM manifest/map->JVM 'north.store_runtime_manifest.Native manifest/map->Native 'north.store_runtime_manifest.StoreRuntimeGeneration manifest/map->StoreRuntimeGeneration})

(defn- fail! [^String message data]
  (throw (ex-info message (assoc data :type :north.store-runtime-generation/error))))

(defn- path [value]
  (.toAbsolutePath (.normalize (.toPath (io/file value)))))

(defn environment! []
  (manifest/derive-runtime-environment! (or (System/getenv "NORTH_STORE_RUNTIME_STATE") manifest/canonical-store-runtime-root)))

(def environment environment!)

(defn- ^String published-selection-path []
  (or (System/getenv "NORTH_STORE_SELECTION") (str (or (System/getenv "XDG_STATE_HOME") (str (System/getProperty "user.home") "/.local/state")) "/north/beagle-store.env")))

(defn- ^Boolean live-environment? [runtime-environment]
  (= manifest/canonical-store-runtime-root (:state-root runtime-environment)))

(defn- no-links []
  (make-array LinkOption 0))

(defn- nofollow-links []
  (into-array LinkOption [LinkOption/NOFOLLOW_LINKS]))

(defn- file-attributes []
  (make-array FileAttribute 0))

(defn- open-options [values]
  (into-array OpenOption values))

(defn- copy-options [values]
  (into-array CopyOption values))

(defn- ensure-directory! [directory]
  (Files/createDirectories directory (file-attributes))
  (if (Files/isSymbolicLink directory) (do
  (fail! "Store runtime directory must not be a symbolic link" {:path (str directory)})))
  directory)

(defn- fsync-directory! [directory]
  (with-open [channel (FileChannel/open directory (open-options [StandardOpenOption/READ]))]
  (.force channel true)))

(defn- write-file-fsynced! [target bytes]
  (with-open [channel (FileChannel/open target (open-options [StandardOpenOption/CREATE_NEW StandardOpenOption/WRITE]))]
  (let [buffer (ByteBuffer/wrap bytes)]
  (loop []
  (if (.hasRemaining buffer) (do
  (.write channel buffer)
  (recur))))
  (.force channel true)))
  target)

(defn- atomic-write-file! [target bytes]
  (let [directory (.getParent ^Path target)
   temporary (.resolve directory (str ".client.next." (UUID/randomUUID)))]
  (try
  (write-file-fsynced! temporary bytes)
  (Files/move temporary target (copy-options [StandardCopyOption/ATOMIC_MOVE StandardCopyOption/REPLACE_EXISTING]))
  (fsync-directory! directory)
  (finally
    (Files/deleteIfExists temporary)))))

(defn- client-values-with! [validate-member! member base]
  (let [checked (validate-member! member)
   common (apply dissoc base runtime-selection-keys)
   values (let [match__0 checked]
  (cond
    (instance? north.store-runtime-manifest.JVM match__0) (let [output (:output match__0) _ (:package-nar-sha256 match__0) _ (:beagle-revision match__0) _ (:beagle-tree match__0) _ (:manifest-path match__0) _ (:manifest-bytes match__0) _ (:manifest-sha256 match__0) _ (:manifest match__0)] (let [out (manifest/jvm-store-out-for output)]
  {"BEAGLE_STORE_HOME" (manifest/jvm-store-home-for output) "BEAGLE_STORE_BIN" (manifest/jvm-store-bin-for output) "BEAGLE_STORE_OUT" out "NORTH_STORE_OUT" out "BEAGLE_STORE_PACKAGED" "1" "BEAGLE_STORE_SERVER_RUNTIME" "jvm"}))
    (instance? north.store-runtime-manifest.Native match__0) (let [release (:release-root match__0) _ (:beagle-revision match__0) _ (:beagle-tree match__0) artifact-root (:artifact-root match__0) closure-sha256 (:closure-sha256 match__0) server-artifact (:server-artifact match__0) server-sha256 (:server-sha256 match__0)] (let [out (str release "/out")]
  {"BEAGLE_STORE_HOME" release "BEAGLE_STORE_BIN" (str release "/bin") "BEAGLE_STORE_OUT" out "NORTH_STORE_OUT" out "BEAGLE_STORE_SERVER_RUNTIME" "native" "BEAGLE_STORE_NATIVE_ARTIFACT_DIR" artifact-root "BEAGLE_STORE_NATIVE_CLOSURE_SHA256" closure-sha256 "BEAGLE_STORE_SERVER_ARTIFACT" server-artifact "BEAGLE_STORE_SERVER_ARTIFACT_SHA256" server-sha256}))))]
  (merge common values)))

(defn- client-values! [member base]
  (client-values-with! manifest/validate-runtime-member! member base))

(defn- ^String client-text! [values]
  (apply str (for [[key value] (sort-by key values)]
  (do
  (if (not (and (re-matches #"[A-Z][A-Z0-9_]*" key) (string? value) (not (str/blank? value)) (nil? (re-find #"['\n\r]" value)))) (do
  (fail! "Store client selection contains a noncanonical binding" {:key key :value value})))
  (str "export " key "='" value "'\n")))))

(defn- client-path [generation-root]
  (.resolve ^Path generation-root client-file-name))

(defn- read-client! [generation-root]
  (runtime-read-selection! (str (client-path generation-root))))

(defn- published-selection-target []
  (let [selection (io/file (published-selection-path))
   parent (.getCanonicalFile (.getParentFile selection))]
  (.toPath (io/file parent (.getName selection)))))

(defn- active-client-target [runtime-environment]
  (.resolve (path (:active-selector runtime-environment)) client-file-name))

(defn- install-published-selection! [runtime-environment]
  (let [target (published-selection-target)
   directory (.getParent target)
   source (active-client-target runtime-environment)
   temporary (.resolve directory (str ".selection.next." (UUID/randomUUID)))]
  (ensure-directory! directory)
  (if (or (Files/isDirectory target (nofollow-links)) (and (Files/exists target (nofollow-links)) (not (or (Files/isRegularFile target (nofollow-links)) (Files/isSymbolicLink target))))) (do
  (fail! "Published Store client selection is not a file or link" {:path (str target)})))
  (try
  (Files/createSymbolicLink temporary source (file-attributes))
  (Files/move temporary target (copy-options [StandardCopyOption/ATOMIC_MOVE StandardCopyOption/REPLACE_EXISTING]))
  (fsync-directory! directory)
  (finally
    (Files/deleteIfExists temporary)))
  target))

(defn- restore-published-selection! [selection]
  (atomic-write-file! (published-selection-target) (.getBytes (client-text! selection) StandardCharsets/UTF_8)))

(defn- with-selector-lock! [runtime-environment operation]
  (locking in-process-lock (let [state-root (ensure-directory! (path (:state-root runtime-environment)))
   generations-root (ensure-directory! (path (:generations-root runtime-environment)))
   lock-path (path (:selector-lock runtime-environment))]
  (if (not (= (.getParent generations-root) state-root)) (do
  (fail! "Store runtime generations root escaped the state root" {:state-root (str state-root) :generations-root (str generations-root)})))
  (with-open [channel (FileChannel/open lock-path (open-options [StandardOpenOption/CREATE StandardOpenOption/WRITE LinkOption/NOFOLLOW_LINKS]))]
  (let [_held (.lock channel)]
  (operation))))))

(def ^:private with-selector-lock with-selector-lock!)

(defn- ^String sha256-hex [bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
  (apply str (map (fn [byte] (format "%02x" (bit-and (int byte) 255))) digest))))

(defn- ^String run-command! [arguments]
  (let [process-builder (ProcessBuilder. ^java.util.List (vec arguments))
   _ (.redirectErrorStream process-builder true)
   process (.start process-builder)
   ^String output (slurp (.getInputStream process))
   exit (.waitFor process)]
  (if (not (zero? exit)) (do
  (fail! "Store runtime metadata command failed" {:arguments (vec arguments) :exit exit :output output})))
  output))

(defn- ^String run-command-bounded! [arguments timeout-ms]
  (let [running (proc/process {:cmd (vec arguments) :out :string :err :string})
   process (:proc running)
   completed (.waitFor process (long timeout-ms) TimeUnit/MILLISECONDS)]
  (if (not completed) (do
  (.destroy process)
  (if (not (.waitFor process 2 TimeUnit/SECONDS)) (do
  (.destroyForcibly process)))
  (fail! "Store runtime command exceeded its exact deadline" {:arguments (vec arguments) :timeout-ms timeout-ms})))
  (let [result (deref running)
   output (str (:out result) (:err result))]
  (if (> (count output) command-output-limit) (do
  (fail! "Store runtime command exceeded its output bound" {:arguments (vec arguments) :maximum command-output-limit :actual (count output)})))
  (if (not (zero? (:exit result))) (do
  (fail! "Store runtime command failed" {:arguments (vec arguments) :exit (:exit result) :output output})))
  (str/trim output))))

(defn- ^String query-nar-sha256! [^String output]
  (let [json (run-command! ["nix" "path-info" "--json" output])
   found (re-find #"\"narHash\"\s*:\s*\"([^\"]+)\"" json)]
  (or (second found) (fail! "Nix path metadata omitted narHash" {:output output :metadata json}))))

(defn- ^JVMPackageObservation observe-jvm-package! [^String output]
  (let [output-path (path output)
   output-text (str output-path)
   _ (if (not (and (Files/isDirectory output-path (nofollow-links)) (manifest/canonical-package-root? output-text))) (do
  (fail! "Store JVM output must be a present canonical /nix/store directory" {:output output-text})))
   manifest-path (path (manifest/manifest-path-for output-text))
   _ (if (not (Files/isRegularFile manifest-path (nofollow-links))) (do
  (fail! "Store JVM runtime.manifest is missing or not a regular file" {:path (str manifest-path)})))
   bytes (Files/readAllBytes manifest-path)
   text (String. bytes StandardCharsets/UTF_8)
   manifest-sha256 (sha256-hex bytes)
   nar-sha256 (query-nar-sha256! output-text)]
  (->JVMPackageObservation output-text nar-sha256 manifest-sha256 text)))

(defn observe-jvm-runtime! [^String output]
  (let [^JVMPackageObservation observation (observe-jvm-package! output)]
  (manifest/promotion-candidate-jvm-runtime! (:output observation) (:nar-sha256 observation) (:manifest-sha256 observation) (:manifest-text observation))))

(defn- selected-jvm-member [generation]
  (let [match__1 (:current generation)]
  (cond
    (instance? north.store-runtime-manifest.JVM match__1) (let [_ (:output match__1) _ (:package-nar-sha256 match__1) _ (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:manifest-path match__1) _ (:manifest-bytes match__1) _ (:manifest-sha256 match__1) _ (:manifest match__1)] (:current generation))
    (instance? north.store-runtime-manifest.Native match__1) (let [_ (:release-root match__1) _ (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:artifact-root match__1) _ (:closure-sha256 match__1) _ (:server-artifact match__1) _ (:server-sha256 match__1)] (:previous generation)))))

(defn- ^String jvm-output! [member]
  (let [match__2 member]
  (cond
    (instance? north.store-runtime-manifest.JVM match__2) (let [output (:output match__2) _ (:package-nar-sha256 match__2) _ (:beagle-revision match__2) _ (:beagle-tree match__2) _ (:manifest-path match__2) _ (:manifest-bytes match__2) _ (:manifest-sha256 match__2) _ (:manifest match__2)] output)
    (instance? north.store-runtime-manifest.Native match__2) (let [_ (:release-root match__2) _ (:beagle-revision match__2) _ (:beagle-tree match__2) _ (:artifact-root match__2) _ (:closure-sha256 match__2) _ (:server-artifact match__2) _ (:server-sha256 match__2)] (fail! "Selected Store promotion source has no JVM member" {})))))

(defn- ^SelectedGeneration attest-selected-promotion-source! [^SelectedGeneration selected]
  (let [generation (:generation selected)]
  (if (manifest/promotion-source-generation? generation) (do
  (let [member (selected-jvm-member generation)
   ^JVMPackageObservation observation (observe-jvm-package! (jvm-output! member))]
  (manifest/attest-promotion-source-runtime! member (:nar-sha256 observation) (:manifest-sha256 observation) (:manifest-text observation)))))
  selected))

(defn- ^String generation-text! [generation]
  (str (pr-str (manifest/validate-runtime-generation! generation)) "\n"))

(defn- generation-record-state [record-path]
  (let [attributes (Files/readAttributes record-path BasicFileAttributes (nofollow-links))]
  [(.fileKey attributes) (.size attributes) (.lastModifiedTime attributes)]))

(defn- read-generation-with! [generation-root validate-generation!]
  (let [record-path (.resolve generation-root generation-file-name)]
  (if (not (Files/isRegularFile record-path (nofollow-links))) (do
  (fail! "Selected Store runtime generation has no regular record" {:path (str record-path)})))
  (let [before (generation-record-state record-path)
   bytes (Files/readAllBytes record-path)
   after (generation-record-state record-path)]
  (if (not (= before after)) (do
  (fail! "Store runtime generation record changed while it was read" {:path (str record-path)})))
  (if (> (alength bytes) max-generation-bytes) (do
  (fail! "Store runtime generation record exceeds its input bound" {:path (str record-path) :maximum max-generation-bytes :actual (alength bytes)})))
  (let [generation (read-edn-with-readers! {:readers generation-readers} (String. bytes StandardCharsets/UTF_8))]
  (validate-generation! generation)))))

(defn- read-generation! [generation-root]
  (read-generation-with! generation-root manifest/validate-runtime-generation!))

(defn- read-promotion-source-generation! [generation-root]
  (read-generation-with! generation-root manifest/validate-promotion-source-generation!))

(defn- selector-target! [runtime-environment]
  (let [selector (path (:active-selector runtime-environment))]
  (cond
  (Files/isSymbolicLink selector) (Files/readSymbolicLink selector)
  (Files/exists selector (nofollow-links)) (fail! "Store runtime active selector must be a symbolic link" {:selector (str selector)})
  :else nil)))

(defn- read-selected-generation-with! [runtime-environment read-generation!]
  (let [target (selector-target! runtime-environment)]
  (if target (do
  (if (.isAbsolute ^Path target) (do
  (fail! "Store runtime selector target must be relative" {:target (str target)})))
  (let [state-root (path (:state-root runtime-environment))
   generations-root (path (:generations-root runtime-environment))
   generation-root (.normalize (.resolve state-root ^Path target))]
  (if (not (and (.startsWith generation-root generations-root) (= (.getParent generation-root) generations-root))) (do
  (fail! "Store runtime selector escaped the generations directory" {:target (str target)})))
  (->SelectedGeneration target generation-root (read-generation! generation-root)))))))

(defn read-selected-generation! [runtime-environment]
  (read-selected-generation-with! runtime-environment read-generation!))

(def read-selected-generation read-selected-generation!)

(defn- read-selected-promotion-source! [runtime-environment]
  (read-selected-generation-with! runtime-environment read-promotion-source-generation!))

(def ^:private read-selected-promotion-source read-selected-promotion-source!)

(defn- ^WrittenGeneration write-generation! [runtime-environment generation base-selection]
  (let [generations-root (path (:generations-root runtime-environment))
   generation-id (str (System/currentTimeMillis) "-" (UUID/randomUUID))
   generation-root (.resolve generations-root generation-id)
   record-path (.resolve generation-root generation-file-name)
   client-selection (client-values! (:current generation) base-selection)
   bytes (.getBytes (generation-text! generation) StandardCharsets/UTF_8)
   client-bytes (.getBytes (client-text! client-selection) StandardCharsets/UTF_8)]
  (Files/createDirectory generation-root (file-attributes))
  (write-file-fsynced! record-path bytes)
  (write-file-fsynced! (client-path generation-root) client-bytes)
  (fsync-directory! generation-root)
  (fsync-directory! generations-root)
  (->WrittenGeneration generation-id generation-root (Paths/get (str "generations/" generation-id) (make-array String 0)) (read-generation! generation-root) (read-client! generation-root))))

(defn- prepare-client-publication-with! [runtime-environment selected validate-member!]
  (if selected (let [selected-root (:root selected)
   existing-client (client-path selected-root)
   base (if (Files/isRegularFile existing-client (nofollow-links)) (runtime-read-selection! (str existing-client)) (runtime-read-selection! (published-selection-path)))
   expected (client-values-with! validate-member! (:current (:generation selected)) base)]
  (if (Files/exists existing-client (nofollow-links)) (if (not (= expected (runtime-read-selection! (str existing-client)))) (do
  (fail! "Selected Store generation carries the wrong client identity" {:generation (str selected-root)}))) (do
  (write-file-fsynced! existing-client (.getBytes (client-text! expected) StandardCharsets/UTF_8))
  (fsync-directory! selected-root)))
  (install-published-selection! runtime-environment)
  expected) (runtime-read-selection! (published-selection-path))))

(defn- prepare-client-publication! [runtime-environment selected]
  (prepare-client-publication-with! runtime-environment selected manifest/validate-runtime-member!))

(defn- prepare-promotion-source-publication! [runtime-environment selected]
  (prepare-client-publication-with! runtime-environment selected manifest/validate-promotion-source-member!))

(defn- atomic-select! [runtime-environment target]
  (let [state-root (path (:state-root runtime-environment))
   selector (path (:active-selector runtime-environment))
   temporary (.resolve state-root (str ".active.next." (UUID/randomUUID)))]
  (try
  (Files/createSymbolicLink temporary ^Path target (file-attributes))
  (Files/move temporary selector (copy-options [StandardCopyOption/ATOMIC_MOVE StandardCopyOption/REPLACE_EXISTING]))
  (fsync-directory! state-root)
  (finally
    (Files/deleteIfExists temporary)))))

(defn- ^SelectedGeneration publish-generation-under-lock! [runtime-environment generation base-selection]
  (let [previous-target (selector-target! runtime-environment)
   written (write-generation! runtime-environment (manifest/validate-runtime-generation! generation) base-selection)
   moved? (volatile! false)]
  (try
  (atomic-select! runtime-environment (:target written))
  (vreset! moved? true)
  (*after-selector-move!* written)
  (install-published-selection! runtime-environment)
  (let [selected (read-selected-generation! runtime-environment)]
  (if (not (= (:generation written) (:generation selected))) (do
  (fail! "Store runtime selector readback differs from the complete generation" {:written (:generation written) :selected (:generation selected)})))
  (if (not (= (:selection written) (read-client! (:root selected)))) (do
  (fail! "Store runtime selector readback has the wrong client identity" {:generation (str (:root selected))})))
  selected)
  (catch Throwable original
    (if (deref moved?) (do
  (try
  (if previous-target (atomic-select! runtime-environment previous-target) (do
  (Files/deleteIfExists (path (:active-selector runtime-environment)))
  (fsync-directory! (path (:state-root runtime-environment)))
  (restore-published-selection! base-selection)))
  (catch Throwable restore-error
    (.addSuppressed original restore-error)))))
    (throw original)))))

(defn ^SelectedGeneration publish-generation! [runtime-environment generation]
  (with-selector-lock! runtime-environment (fn [] (let [selected (read-selected-generation! runtime-environment)
   base (prepare-client-publication! runtime-environment selected)]
  (publish-generation-under-lock! runtime-environment generation base)))))

(defn- ^SelectedGeneration selected-or-fail! [runtime-environment]
  (or (read-selected-generation! runtime-environment) (fail! "No Store runtime generation is selected" {:selector (:active-selector runtime-environment)})))

(defn- current-member! [runtime-environment]
  (:current (:generation (selected-or-fail! runtime-environment))))

(defn launch-current! [runtime-environment]
  (let [^SelectedGeneration selected (selected-or-fail! runtime-environment)
   evidence (runtime-generation-evidence! (:state-root runtime-environment))
   _ (if (not (= (:generation selected) (:generation evidence))) (do
  (fail! "Selected Store generation changed before launch" {:selected (:generation selected) :evidence (:generation evidence)})))
   member (:current (:generation selected))
   selection (read-client! (:root selected))
   {:keys [executable arguments environment]} (runtime-launch-spec! member selection)]
  (apply proc/exec {:extra-env environment} executable arguments)))

(defn publish-current-runtime! [runtime-environment pid ^String controller-unit]
  (let [selected (selected-or-fail! runtime-environment)
   evidence (runtime-generation-evidence! (:state-root runtime-environment))]
  (if (not (= (str (:root selected)) (:root evidence))) (do
  (fail! "Selected Store generation changed before runtime publication" {:selected (str (:root selected)) :evidence (:root evidence)})))
  (if (not (= (:generation selected) (:generation evidence))) (do
  (fail! "Selected Store generation content changed before runtime publication" {:selected (:generation selected) :evidence (:generation evidence)})))
  (runtime-publish-record! {:member (:current (:generation selected)) :generation-evidence evidence :pid pid :controller-unit controller-unit :record-path (runtime-default-record-path) :selection (read-client! (:root selected))})))

(defn ^String babashka-executable []
  (or (System/getenv "NORTH_BB") (let [self (path "/proc/self/exe")]
  (if (Files/isExecutable self) (do
  (str (.toRealPath self (no-links)))))) "bb"))

(defn store-status-command! [member]
  (let [checked (manifest/validate-runtime-member! member)]
  (let [match__3 checked]
  (cond
    (instance? north.store-runtime-manifest.JVM match__3) (let [output (:output match__3) _ (:package-nar-sha256 match__3) _ (:beagle-revision match__3) _ (:beagle-tree match__3) _ (:manifest-path match__3) _ (:manifest-bytes match__3) _ (:manifest-sha256 match__3) _ (:manifest match__3)] [(manifest/jvm-dispatcher-path-for output) "store" "status"])
    (instance? north.store-runtime-manifest.Native match__3) (let [release (:release-root match__3) _ (:beagle-revision match__3) _ (:beagle-tree match__3) _ (:artifact-root match__3) _ (:closure-sha256 match__3) _ (:server-artifact match__3) _ (:server-sha256 match__3)] [(babashka-executable) "-cp" (manifest/native-client-classpath-for release) (manifest/native-client-path-for release) "status"])))))

(defn- ^String validate-store-status! [member ^String status]
  (let [expected-token (manifest/expected-store-status-engine-token! member)
   selected-kind (manifest/runtime-member-kind member)
   fields (str/split status #"\|" -1)
   actual-token (last fields)]
  (if (not (and (= 5 (count fields)) (= "up" (nth fields 0)) (re-matches #"(?:0|[1-9][0-9]*)" (nth fields 1)) (re-matches #"(?:0|[1-9][0-9]*)" (nth fields 2)) (= "ready" (nth fields 3)) (= expected-token actual-token))) (do
  (fail! "beagle store status does not report the selected ready runtime" {:selected selected-kind :expected-token expected-token :status status})))
  status))

(defn- ^String strip-store-status-line-terminator [^String output]
  (cond
  (str/ends-with? output "\r\n") (subs output 0 (- (count output) 2))
  (str/ends-with? output "\n") (subs output 0 (dec (count output)))
  :else output))

(defn- ^String validate-store-status-output! [member ^String output]
  (validate-store-status! member (strip-store-status-line-terminator output)))

(defn- ^String bounded-store-status-for! [runtime-environment generation]
  (let [current (:current generation)
   command (store-status-command! current)
   selected (read-selected-generation! runtime-environment)
   selection (if selected (read-client! (:root selected)) (runtime-read-selection! (published-selection-path)))
   launch-environment (:environment (runtime-launch-spec! current selection))
   port (get launch-environment "BEAGLE_STORE_SERVER_PORT")
   result (let [running (proc/process {:cmd command :extra-env (assoc launch-environment "NORTH_PORT" port) :out :string :err :string})
   process (:proc running)
   completed (.waitFor process status-timeout-ms TimeUnit/MILLISECONDS)]
  (if (not completed) (do
  (.destroy process)
  (if (not (.waitFor process 2 TimeUnit/SECONDS)) (do
  (.destroyForcibly process)))
  (fail! "beagle store status exceeded its exact deadline" {:timeout-ms status-timeout-ms})))
  (let [done (deref running)
   output (str (:out done) (:err done))]
  (if (not (and (zero? (:exit done)) (<= (count output) command-output-limit))) (do
  (fail! "beagle store status failed" {:exit (:exit done) :output output})))
  output))]
  (validate-store-status-output! current result)))

(defn- ^String bounded-store-status! [runtime-environment]
  (let [generation (:generation (selected-or-fail! runtime-environment))]
  (bounded-store-status-for! runtime-environment generation)))

(def ^:private coordinator-environment-keys ["BEAGLE_STORE_HOME" "BEAGLE_STORE_BIN" "BEAGLE_STORE_OUT" "BEAGLE_STORE_PACKAGED" "BEAGLE_STORE_SERVER_RUNTIME" "BEAGLE_STORE_SERVER_CLASSPATH_FILE" "BEAGLE_STORE_JAVA" "BEAGLE_STORE_LOG" "BEAGLE_STORE_SPACE_ID" "BEAGLE_STORE_SERVER_PORT" "NORTH_PORT" "NORTH_STORE_OUT"])

(defn- properties [^String text]
  (into {} (keep (fn [^String line] (let [index (.indexOf line "=")]
  (if (pos? index) (do
  [(subs line 0 index) (subs line (inc index))])))) (str/split-lines text))))

(defn- live-controller-pid! []
  (let [values (properties (run-command-bounded! ["systemctl" "--user" "show" live-unit "--no-pager" "--property" "Id" "--property" "LoadState" "--property" "ActiveState" "--property" "SubState" "--property" "MainPID"] status-timeout-ms))
   pid (int (or (parse-long (get values "MainPID")) 0))]
  (if (and (= live-unit (get values "Id")) (= "loaded" (get values "LoadState")) (= "active" (get values "ActiveState")) (= "running" (get values "SubState")) (pos? pid)) pid (fail! "North coordinator is not one loaded running user service" {:unit live-unit :properties values}))))

(defn- process-bytes! [pid ^String leaf]
  (let [source (path (str "/proc/" pid "/" leaf))
   bytes (Files/readAllBytes source)]
  (if (<= (alength ^bytes bytes) command-output-limit) bytes (fail! "North coordinator process metadata exceeds its input bound" {:pid pid :leaf leaf :maximum command-output-limit}))))

(defn- process-environment! [pid]
  (let [^String text (String. ^bytes (process-bytes! pid "environ") StandardCharsets/UTF_8)]
  (into {} (keep (fn [^String entry] (let [index (.indexOf entry "=")]
  (if (pos? index) (do
  [(subs entry 0 index) (subs entry (inc index))])))) (str/split text #"\u0000")))))

(defn- coordinator-environment-mismatches [expected actual]
  (vec (filter (fn [^String key] (not (= (get expected key) (get actual key)))) coordinator-environment-keys)))

(defn- coordinator-process! [pid expected]
  (let [actual (process-environment! pid)
   mismatches (coordinator-environment-mismatches expected actual)
   ^String executable (str (.toRealPath (path (str "/proc/" pid "/exe")) (no-links)))
   ^String expected-executable (str (.toRealPath (path (get expected "BEAGLE_STORE_JAVA")) (no-links)))
   ^String arguments (String. ^bytes (process-bytes! pid "cmdline") StandardCharsets/UTF_8)]
  (if (not (and (= expected-executable executable) (str/includes? arguments "clojure.main\u0000-m\u0000north.coordinator\u0000") (empty? mismatches))) (do
  (fail! "North coordinator process differs from the selected Store runtime" {:pid pid :expected-executable expected-executable :actual-executable executable :environment-mismatches mismatches})))
  {:unit live-unit :pid pid :executable executable}))

(defn- ^String await-store-status! [runtime-environment]
  (let [deadline (+ (System/nanoTime) (* status-timeout-ms 1000000))]
  (loop [last-error nil]
  (let [attempt (try
  [(bounded-store-status! runtime-environment) nil]
  (catch Throwable error
    [nil error]))
   status (nth attempt 0)
   error (nth attempt 1)]
  (if (some? status) status (if (< (System/nanoTime) deadline) (do
  (Thread/sleep 100)
  (recur error)) (do
  (if (some? last-error) (do
  (.addSuppressed error last-error)))
  (throw error))))))))

(defn attest-selected-live! [runtime-environment]
  (let [^SelectedGeneration selected (selected-or-fail! runtime-environment)
   current (:current (:generation selected))
   ^String kind (manifest/runtime-member-kind current)
   selection (read-client! (:root selected))
   launch-environment (:environment (runtime-launch-spec! current selection))
   expected (assoc (merge selection launch-environment) "NORTH_PORT" (get launch-environment "BEAGLE_STORE_SERVER_PORT"))]
  (if (not (= "jvm" kind)) (do
  (fail! "North coordinator requires the selected JVM Store runtime" {:selected kind})))
  (let [controller (coordinator-process! (live-controller-pid!) expected)
   ^String status (await-store-status! runtime-environment)]
  {:controller controller :status status})))

(defn- switch-live! [runtime-environment]
  (run-command! ["systemctl" "--user" "restart" live-unit])
  (attest-selected-live! runtime-environment))

(defn- restore-selection-after-live-failure! [runtime-environment previous]
  (if previous (do
  (atomic-select! runtime-environment (:target previous))
  (let [restored (read-selected-generation! runtime-environment)]
  (if (not (and (= (:target previous) (:target restored)) (= (:root previous) (:root restored)) (= (:generation previous) (:generation restored)))) (do
  (fail! "Store runtime recovery did not restore the exact predecessor" {:expected-root (str (:root previous)) :actual-root (some-> restored :root str)}))))
  (install-published-selection! runtime-environment)
  (switch-live! runtime-environment)) (fail! "Live Store transition has no predecessor generation to restore" {:selector (:active-selector runtime-environment)})))

(defn- ^SelectedGeneration commit-live-transition! [runtime-environment previous generation base-selection]
  (try
  (if (and previous (= generation (:generation previous))) (do
  (try
  (attest-selected-live! runtime-environment)
  (catch Throwable _
    (switch-live! runtime-environment)))
  previous) (let [selected (publish-generation-under-lock! runtime-environment generation base-selection)]
  (switch-live! runtime-environment)
  selected))
  (catch Throwable original
    (try
  (restore-selection-after-live-failure! runtime-environment previous)
  (catch Throwable restore-error
    (.addSuppressed original restore-error)))
    (throw original))))

(defn promote! [runtime-environment ^String output]
  (let [candidate (observe-jvm-runtime! output)]
  (with-selector-lock! runtime-environment (fn [] (let [selected (some-> (read-selected-promotion-source runtime-environment) attest-selected-promotion-source!)
   base-selection (prepare-promotion-source-publication! runtime-environment selected)
   generation (if selected (manifest/promote-authority-transition! (:generation selected) candidate) (manifest/initial-promotion-transition! candidate))]
  (if (live-environment? runtime-environment) (commit-live-transition! runtime-environment selected generation base-selection) (if (and selected (= generation (:generation selected))) selected (publish-generation-under-lock! runtime-environment generation base-selection))))))))

(defn rollback! [runtime-environment]
  (with-selector-lock! runtime-environment (fn [] (let [selected (selected-or-fail! runtime-environment)
   base-selection (prepare-client-publication! runtime-environment selected)
   generation (manifest/rollback-transition! (:generation selected))]
  (if (live-environment? runtime-environment) (commit-live-transition! runtime-environment selected generation base-selection) (publish-generation-under-lock! runtime-environment generation base-selection))))))

(defn restore! [runtime-environment]
  (with-selector-lock! runtime-environment (fn [] (let [selected (selected-or-fail! runtime-environment)
   base-selection (prepare-client-publication! runtime-environment selected)
   generation (manifest/restore-transition! (:generation selected))]
  (if (live-environment? runtime-environment) (commit-live-transition! runtime-environment selected generation base-selection) (if (= generation (:generation selected)) selected (publish-generation-under-lock! runtime-environment generation base-selection)))))))

(defn print-status! [runtime-environment]
  (let [selected (read-selected-generation! runtime-environment)]
  (if selected (do
  (println (str "generation=" (.getFileName ^Path (:root selected))))
  (println (str "selector=" (:active-selector runtime-environment)))
  (doseq [line (manifest/generation-status-lines! (:generation selected))]
  (println line))
  selected) (do
  (println "generation=none")
  (println (str "selector=" (:active-selector runtime-environment)))
  nil))))

(defn- usage! []
  (binding [*out* *err*]
  (println "usage: north-store-runtime status")
  (println "       north-store-runtime promote OUT")
  (println "       north-store-runtime rollback")
  (println "       north-store-runtime restore")
  (println "       north-store-runtime attest")
  (println "       north-store-runtime launch")
  (println "       north-store-runtime publish-runtime PID UNIT"))
  (System/exit 2))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [runtime-environment (environment)
   command (first args)]
  (try
  (case command
    "status" (if (= 1 (count args)) (print-status! runtime-environment) (usage!))
    "promote" (if (= 2 (count args)) (do
  (promote! runtime-environment (second args))
  (print-status! runtime-environment)) (usage!))
    "rollback" (if (= 1 (count args)) (do
  (rollback! runtime-environment)
  (print-status! runtime-environment)) (usage!))
    "restore" (if (= 1 (count args)) (do
  (restore! runtime-environment)
  (print-status! runtime-environment)) (usage!))
    "attest" (if (= 1 (count args)) (let [{:keys [status]} (attest-selected-live! runtime-environment)]
  (println (str "usable_rpc=" status))
  (print-status! runtime-environment)) (usage!))
    "launch" (if (= 1 (count args)) (launch-current! runtime-environment) (usage!))
    "publish-runtime" (if (= 3 (count args)) (let [attestation (publish-current-runtime! runtime-environment (second args) (nth args 2))]
  (println (str "attested=" (get-in attestation [:identity :runtime-kind])))) (usage!))
    (usage!))
  (catch Throwable error
    (binding [*out* *err*]
  (println (str "north-store-runtime: " (.getMessage error)))
  (let [data (ex-data error)]
  (if data (do
  (println (str "north-store-runtime: " (pr-str data))))))
  (doseq [suppressed (.getSuppressed error)]
  (println (str "north-store-runtime: recovery: " (.getMessage suppressed)))
  (let [data (ex-data suppressed)]
  (if data (do
  (println (str "north-store-runtime: recovery: " (pr-str data))))))))
    (System/exit 2))))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (apply -main *command-line-args*)))
