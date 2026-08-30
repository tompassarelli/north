(ns north.store-runtime-generation
  (:require [babashka.process :as proc]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [north.store-runtime-manifest :as manifest])
  (:import [java.nio ByteBuffer]
           [java.nio.channels FileChannel]
           [java.nio.charset StandardCharsets]
           [java.nio.file CopyOption Files LinkOption OpenOption Path Paths
            StandardCopyOption StandardOpenOption]
           [java.nio.file.attribute FileAttribute]
           [java.security MessageDigest]
           [java.util UUID]))

(def ^:private north-root
  (.getCanonicalPath
   (io/file (.getParentFile (io/file *file*)) "..")))

(load-file (str north-root "/cli/runtime-attestation.clj"))

(def ^:private runtime-read-selection!
  (requiring-resolve 'north.runtime-attestation/read-selection!))
(def ^:private runtime-launch-spec!
  (requiring-resolve 'north.runtime-attestation/launch-spec!))
(def ^:private runtime-generation-evidence!
  (requiring-resolve 'north.runtime-attestation/active-generation-evidence!))
(def ^:private runtime-publish-record!
  (requiring-resolve 'north.runtime-attestation/publish-runtime-record!))
(def ^:private runtime-attest-record!
  (requiring-resolve 'north.runtime-attestation/attest-runtime-record!))
(def ^:private runtime-default-record-path
  (requiring-resolve 'north.runtime-attestation/default-runtime-record-path))
(def ^:private runtime-state-root-var
  (requiring-resolve 'north.runtime-attestation/*store-runtime-state-root*))

(defonce ^:private in-process-lock (Object.))

(def ^:dynamic *after-selector-move!*
  (fn [_selection] nil))

(def ^:private generation-file-name "generation.edn")
(def ^:private max-generation-bytes 32768)
(def ^:private live-unit "north-store.service")
(def ^:private live-switch-timeout-ms 45000)
(def ^:private status-timeout-ms 15000)
(def ^:private command-output-limit 65536)

(def ^:private generation-readers
  {'north.store_runtime_manifest.StoreRuntimeManifest
   manifest/map->StoreRuntimeManifest
   'north.store_runtime_manifest.JVM manifest/map->JVM
   'north.store_runtime_manifest.Native manifest/map->Native
   'north.store_runtime_manifest.StoreRuntimeGeneration
   manifest/map->StoreRuntimeGeneration})

(defn- fail! [message data]
  (throw (ex-info message (assoc data :type :north.store-runtime-generation/error))))

(defn- path [value]
  (.toAbsolutePath (.normalize (.toPath (io/file value)))))

(defn environment []
  (manifest/derive-runtime-environment!
   (or (System/getenv "NORTH_STORE_RUNTIME_STATE")
       manifest/canonical-store-runtime-root)))

(defn- selection-path []
  (or (System/getenv "NORTH_STORE_SELECTION")
      (str (or (System/getenv "XDG_STATE_HOME")
               (str (System/getProperty "user.home") "/.local/state"))
           "/north/beagle-store.env")))

(defn- live-environment? [runtime-environment]
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
  (when (Files/isSymbolicLink directory)
    (fail! "Store runtime directory must not be a symbolic link"
           {:path (str directory)}))
  directory)

(defn- fsync-directory! [directory]
  (with-open [channel (FileChannel/open
                       directory
                       (open-options [StandardOpenOption/READ]))]
    (.force channel true)))

(defn- write-file-fsynced! [target bytes]
  (with-open [channel (FileChannel/open
                       target
                       (open-options [StandardOpenOption/CREATE_NEW
                                      StandardOpenOption/WRITE]))]
    (let [buffer (ByteBuffer/wrap bytes)]
      (while (.hasRemaining buffer)
        (.write channel buffer))
      (.force channel true)))
  target)

(defn- with-selector-lock [runtime-environment operation]
  (locking in-process-lock
    (let [state-root (ensure-directory!
                      (path (:state-root runtime-environment)))
          generations-root (ensure-directory!
                            (path (:generations-root runtime-environment)))
          lock-path (path (:selector-lock runtime-environment))]
      (when-not (= (.getParent generations-root) state-root)
        (fail! "Store runtime generations root escaped the state root"
               {:state-root (str state-root)
                :generations-root (str generations-root)}))
      (with-open [channel (FileChannel/open
                           lock-path
                           (open-options [StandardOpenOption/CREATE
                                          StandardOpenOption/WRITE
                                          LinkOption/NOFOLLOW_LINKS]))]
        (let [_held (.lock channel)]
          (operation))))))

(defn- sha256-hex [bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
    (apply str (map #(format "%02x" (bit-and (int %) 0xff)) digest))))

(defn- run-command! [arguments]
  (let [process-builder (ProcessBuilder. ^java.util.List (vec arguments))
        _ (.redirectErrorStream process-builder true)
        process (.start process-builder)
        output (slurp (.getInputStream process))
        exit (.waitFor process)]
    (when-not (zero? exit)
      (fail! "Store runtime metadata command failed"
             {:arguments (vec arguments) :exit exit :output output}))
    output))

(defn- run-command-bounded! [arguments timeout-ms]
  (let [running (proc/process {:cmd (vec arguments) :out :string :err :string})
        process ^Process (:proc running)
        completed (.waitFor process (long timeout-ms)
                            java.util.concurrent.TimeUnit/MILLISECONDS)]
    (when-not completed
      (.destroy process)
      (when-not (.waitFor process 2 java.util.concurrent.TimeUnit/SECONDS)
        (.destroyForcibly process))
      (fail! "Store runtime command exceeded its exact deadline"
             {:arguments (vec arguments) :timeout-ms timeout-ms}))
    (let [result @running
          output (str (:out result) (:err result))]
      (when (> (count output) command-output-limit)
        (fail! "Store runtime command exceeded its output bound"
               {:arguments (vec arguments)
                :maximum command-output-limit :actual (count output)}))
      (when-not (zero? (:exit result))
        (fail! "Store runtime command failed"
               {:arguments (vec arguments) :exit (:exit result)
                :output output}))
      (str/trim output))))

(defn- service-override-path []
  (io/file (or (System/getenv "XDG_CONFIG_HOME")
               (str (System/getProperty "user.home") "/.config"))
           "systemd" "user" (str live-unit ".d")
           "50-north-store-runtime.conf"))

(defn- install-live-service-override! []
  (when-not (= north-root "/home/tom/code/north/main")
    (fail! "Live Store switching must run from canonical North main"
           {:north-root north-root}))
  (let [target (.toPath (service-override-path))
        directory (.getParent target)
        temporary (.resolve directory (str ".runtime.next." (UUID/randomUUID)))
        tool (str north-root "/bin/north-store-runtime")
        text (str "[Service]\n"
                  "ExecStart=\n"
                  "ExecStart=" tool " launch\n"
                  "ExecStartPost=\n"
                  "ExecStartPost=" tool
                  " publish-runtime $MAINPID " live-unit "\n"
                  "Environment=NORTH_STORE_RUNTIME_STATE="
                  manifest/canonical-store-runtime-root "\n")
        existed? (Files/exists target (nofollow-links))
        _ (when (Files/isSymbolicLink target)
            (fail! "Store runtime systemd override must not be a link"
                   {:path (str target)}))
        previous (when existed? (Files/readAllBytes target))]
    (Files/createDirectories directory (file-attributes))
    (when (Files/isSymbolicLink directory)
      (fail! "Store runtime systemd override directory must not be a link"
             {:path (str directory)}))
    (try
      (Files/writeString temporary text StandardCharsets/UTF_8
                         (into-array OpenOption
                                     [StandardOpenOption/CREATE_NEW
                                      StandardOpenOption/WRITE]))
      (Files/move temporary target
                  (copy-options [StandardCopyOption/ATOMIC_MOVE
                                 StandardCopyOption/REPLACE_EXISTING]))
      (finally
        (Files/deleteIfExists temporary)))
    (try
      (run-command-bounded! ["systemctl" "--user" "daemon-reload"]
                            status-timeout-ms)
      (catch Throwable original
        (try
          (if existed?
            (Files/write target ^bytes previous
                         (open-options [StandardOpenOption/TRUNCATE_EXISTING
                                        StandardOpenOption/WRITE]))
            (Files/deleteIfExists target))
          (run-command-bounded! ["systemctl" "--user" "daemon-reload"]
                                status-timeout-ms)
          (catch Throwable restore-error
            (.addSuppressed original restore-error)))
        (throw original)))
    target))

(defn- remove-live-service-override! [override-path]
  (Files/deleteIfExists ^Path override-path)
  (run-command-bounded! ["systemctl" "--user" "daemon-reload"]
                        status-timeout-ms))

(defn- query-nar-sha256! [output]
  (let [json (run-command! ["nix" "path-info" "--json" output])
        found (re-find #"\"narHash\"\s*:\s*\"([^\"]+)\"" json)]
    (or (second found)
        (fail! "Nix path metadata omitted narHash"
               {:output output :metadata json}))))

(defn observe-jvm-runtime! [output]
  (let [output-path (path output)
        output-text (str output-path)
        _ (when-not (and (Files/isDirectory output-path (nofollow-links))
                         (manifest/canonical-package-root? output-text))
            (fail! "Store JVM output must be a present canonical /nix/store directory"
                   {:output output-text}))
        manifest-path (path (manifest/manifest-path-for output-text))
        _ (when-not (Files/isRegularFile manifest-path (nofollow-links))
            (fail! "Store JVM runtime.manifest is missing or not a regular file"
                   {:path (str manifest-path)}))
        bytes (Files/readAllBytes manifest-path)
        text (String. bytes StandardCharsets/UTF_8)
        manifest-sha256 (sha256-hex bytes)
        nar-sha256 (query-nar-sha256! output-text)]
    (manifest/accepted-jvm-runtime!
     output-text nar-sha256 manifest-sha256 text)))

(defn- generation-text [generation]
  (str (pr-str (manifest/validate-runtime-generation! generation)) "\n"))

(defn- read-generation! [generation-root]
  (let [record-path (.resolve generation-root generation-file-name)]
    (when-not (Files/isRegularFile record-path (nofollow-links))
      (fail! "Selected Store runtime generation has no regular record"
             {:path (str record-path)}))
    (let [bytes (Files/readAllBytes record-path)]
      (when (> (alength bytes) max-generation-bytes)
        (fail! "Store runtime generation record exceeds its input bound"
               {:path (str record-path)
                :maximum max-generation-bytes
                :actual (alength bytes)}))
      (let [generation (edn/read-string
                        {:readers generation-readers}
                        (String. bytes StandardCharsets/UTF_8))]
        (manifest/validate-runtime-generation! generation)))))

(defn- selector-target [runtime-environment]
  (let [selector (path (:active-selector runtime-environment))]
    (cond
      (Files/isSymbolicLink selector)
      (Files/readSymbolicLink selector)

      (Files/exists selector (nofollow-links))
      (fail! "Store runtime active selector must be a symbolic link"
             {:selector (str selector)})

      :else nil)))

(defn read-selected-generation [runtime-environment]
  (when-let [target (selector-target runtime-environment)]
    (when (.isAbsolute ^Path target)
      (fail! "Store runtime selector target must be relative"
             {:target (str target)}))
    (let [state-root (path (:state-root runtime-environment))
          generations-root (path (:generations-root runtime-environment))
          generation-root (.normalize (.resolve state-root ^Path target))]
      (when-not (and (.startsWith generation-root generations-root)
                     (= (.getParent generation-root) generations-root))
        (fail! "Store runtime selector escaped the generations directory"
               {:target (str target)}))
      {:target target
       :root generation-root
       :generation (read-generation! generation-root)})))

(defn- write-generation! [runtime-environment generation]
  (let [generations-root (path (:generations-root runtime-environment))
        generation-id (str (System/currentTimeMillis) "-" (UUID/randomUUID))
        generation-root (.resolve generations-root generation-id)
        record-path (.resolve generation-root generation-file-name)
        bytes (.getBytes (generation-text generation) StandardCharsets/UTF_8)]
    (Files/createDirectory generation-root (file-attributes))
    (write-file-fsynced! record-path bytes)
    (fsync-directory! generation-root)
    (fsync-directory! generations-root)
    {:id generation-id
     :root generation-root
     :target (Paths/get (str "generations/" generation-id)
                        (make-array String 0))
     :generation (read-generation! generation-root)}))

(defn- atomic-select! [runtime-environment target]
  (let [state-root (path (:state-root runtime-environment))
        selector (path (:active-selector runtime-environment))
        temporary (.resolve
                   state-root
                   (str ".active.next." (UUID/randomUUID)))]
    (try
      (Files/createSymbolicLink temporary ^Path target (file-attributes))
      (Files/move temporary selector
                  (copy-options [StandardCopyOption/ATOMIC_MOVE
                                 StandardCopyOption/REPLACE_EXISTING]))
      (fsync-directory! state-root)
      (finally
        (Files/deleteIfExists temporary)))))

(defn- publish-generation-under-lock! [runtime-environment generation]
  (let [previous-target (selector-target runtime-environment)
        written (write-generation!
                 runtime-environment
                 (manifest/validate-runtime-generation! generation))
        moved? (volatile! false)]
    (try
      (atomic-select! runtime-environment (:target written))
      (vreset! moved? true)
      (*after-selector-move!* written)
      (let [selected (read-selected-generation runtime-environment)]
        (when-not (= (:generation written) (:generation selected))
          (fail! "Store runtime selector readback differs from the complete generation"
                 {:written (:generation written)
                  :selected (:generation selected)}))
        selected)
      (catch Throwable original
        (when @moved?
          (try
            (if previous-target
              (atomic-select! runtime-environment previous-target)
              (do
                (Files/deleteIfExists
                 (path (:active-selector runtime-environment)))
                (fsync-directory!
                 (path (:state-root runtime-environment)))))
            (catch Throwable restore-error
              (.addSuppressed original restore-error))))
        (throw original)))))

(defn publish-generation! [runtime-environment generation]
  (with-selector-lock
    runtime-environment
    #(publish-generation-under-lock! runtime-environment generation)))

(defn- selected-or-fail! [runtime-environment]
  (or (read-selected-generation runtime-environment)
      (fail! "No Store runtime generation is selected"
             {:selector (:active-selector runtime-environment)})))

(defn- current-member! [runtime-environment]
  (:current (:generation (selected-or-fail! runtime-environment))))

(defn launch-current! [runtime-environment]
  (let [selected (selected-or-fail! runtime-environment)
        evidence (runtime-generation-evidence! (:state-root runtime-environment))
        _ (when-not (= (:generation selected) (:generation evidence))
            (fail! "Selected Store generation changed before launch"
                   {:selected (:generation selected)
                    :evidence (:generation evidence)}))
        member (:current (:generation selected))
        selection (runtime-read-selection! (selection-path))
        {:keys [executable arguments environment]}
        (runtime-launch-spec! member selection)]
    (apply proc/exec {:extra-env environment} executable arguments)))

(defn publish-current-runtime!
  [runtime-environment pid controller-unit]
  (let [selected (selected-or-fail! runtime-environment)
        evidence (runtime-generation-evidence! (:state-root runtime-environment))]
    (when-not (= (str (:root selected)) (:root evidence))
      (fail! "Selected Store generation changed before runtime publication"
             {:selected (str (:root selected)) :evidence (:root evidence)}))
    (when-not (= (:generation selected) (:generation evidence))
      (fail! "Selected Store generation content changed before runtime publication"
             {:selected (:generation selected) :evidence (:generation evidence)}))
    (runtime-publish-record!
     {:member (:current (:generation selected))
      :generation-evidence evidence
      :pid pid
      :controller-unit controller-unit
      :record-path (runtime-default-record-path)
      :selection (runtime-read-selection! (selection-path))})))

(defn babashka-executable []
  (or (System/getenv "NORTH_BB")
      (let [self (path "/proc/self/exe")]
        (when (Files/isExecutable self)
          (str (.toRealPath self (no-links)))))
      "bb"))

(defn store-status-command! [member]
  (let [checked (manifest/validate-runtime-member! member)]
    (case (manifest/runtime-member-kind checked)
      "jvm"
      [(manifest/jvm-dispatcher-path-for (:output checked)) "store" "status"]

      "native"
      [(babashka-executable)
       "-cp" (manifest/native-client-classpath-for (:release-root checked))
       (manifest/native-client-path-for (:release-root checked))
       "status"])))

(defn- bounded-store-status-for! [generation expected-kind]
  (let [current (:current generation)
        command (store-status-command! current)
        selection (runtime-read-selection! (selection-path))
        selected-kind (manifest/runtime-member-kind current)
        launch-environment (:environment
                            (runtime-launch-spec! current selection))
        port (get launch-environment "BEAGLE_STORE_SERVER_PORT")
        result
        (let [running
              (proc/process
               {:cmd command
                :extra-env (assoc launch-environment "NORTH_PORT" port)
                :out :string :err :string})
              process ^Process (:proc running)
              completed (.waitFor process status-timeout-ms
                                  java.util.concurrent.TimeUnit/MILLISECONDS)]
          (when-not completed
            (.destroy process)
            (when-not (.waitFor process 2 java.util.concurrent.TimeUnit/SECONDS)
              (.destroyForcibly process))
            (fail! "beagle store status exceeded its exact deadline"
                   {:timeout-ms status-timeout-ms}))
          (let [done @running
                output (str (:out done) (:err done))]
            (when-not (and (zero? (:exit done))
                           (<= (count output) command-output-limit))
              (fail! "beagle store status failed"
                     {:exit (:exit done) :output output}))
            (str/trim output)))
        fields (str/split result #"\|")
        actual-kind (last fields)]
    (when-not (and (= 5 (count fields))
                   (= "up" (nth fields 0))
                   (re-matches #"[0-9]+" (nth fields 1))
                   (re-matches #"[0-9]+" (nth fields 2))
                   (= "ready" (nth fields 3))
                   (= expected-kind actual-kind))
      (fail! "beagle store status does not report the selected ready runtime"
             {:selected selected-kind :expected expected-kind :status result}))
    result))

(defn- bounded-store-status! [runtime-environment]
  (let [generation (:generation (selected-or-fail! runtime-environment))]
    (bounded-store-status-for!
     generation (manifest/runtime-member-kind (:current generation)))))

(defn attest-selected-live! [runtime-environment]
  (let [record (runtime-default-record-path)
        attestation
        (with-bindings {runtime-state-root-var (:state-root runtime-environment)}
          (runtime-attest-record! record))
        status (bounded-store-status! runtime-environment)
        selected-kind (manifest/runtime-member-kind
                       (current-member! runtime-environment))]
    (when-not (= selected-kind (get-in attestation [:identity :runtime-kind]))
      (fail! "Selected Store generation and listener attestation disagree"
             {:selected selected-kind
              :attested (get-in attestation [:identity :runtime-kind])}))
    {:attestation attestation :status status}))

(defn- attest-native-baseline! [runtime-environment generation]
  (let [record (runtime-default-record-path)
        attestation
        (with-bindings {runtime-state-root-var (:state-root runtime-environment)}
          (runtime-attest-record! record))
        status (bounded-store-status-for! generation "native")]
    (when-not (= "native" (get-in attestation [:identity :runtime-kind]))
      (fail! "Restored no-generation baseline is not the accepted Native listener"
             {:attestation attestation}))
    {:attestation attestation :status status}))

(defn- switch-live! [runtime-environment]
  (run-command-bounded! ["systemctl" "--user" "restart" live-unit]
                        live-switch-timeout-ms)
  (attest-selected-live! runtime-environment))

(defn- restore-selection-after-live-failure!
  [runtime-environment previous generation override-snapshot]
  (if previous
    (do
      (atomic-select! runtime-environment (:target previous))
      (switch-live! runtime-environment))
    (do
      (Files/deleteIfExists (path (:active-selector runtime-environment)))
      (fsync-directory! (path (:state-root runtime-environment)))
      (remove-live-service-override! override-snapshot)
      (run-command-bounded! ["systemctl" "--user" "restart" live-unit]
                            live-switch-timeout-ms)
      (attest-native-baseline! runtime-environment generation))))

(defn- commit-live-transition!
  [runtime-environment previous generation override-snapshot]
  (try
    (if (and previous (= generation (:generation previous)))
      (do
        (try
          (attest-selected-live! runtime-environment)
          (catch Throwable _
            (switch-live! runtime-environment)))
        previous)
      (let [selected
            (publish-generation-under-lock! runtime-environment generation)]
        (switch-live! runtime-environment)
        selected))
    (catch Throwable original
      (try
        (restore-selection-after-live-failure!
         runtime-environment previous generation override-snapshot)
        (catch Throwable restore-error
          (.addSuppressed original restore-error)))
      (throw original))))

(defn promote! [runtime-environment output]
  (let [candidate (observe-jvm-runtime! output)]
    (with-selector-lock
      runtime-environment
      (fn []
        (let [selected (read-selected-generation runtime-environment)
              generation (if selected
                           (manifest/promote-transition!
                            (:generation selected) candidate)
                           (manifest/initial-promotion-transition! candidate))
              override-snapshot
              (when (live-environment? runtime-environment)
                (install-live-service-override!))]
          (if (live-environment? runtime-environment)
            (commit-live-transition! runtime-environment selected generation
                                     override-snapshot)
            (if (and selected (= generation (:generation selected)))
              selected
              (publish-generation-under-lock!
               runtime-environment generation))))))))

(defn rollback! [runtime-environment]
  (with-selector-lock
    runtime-environment
    (fn []
      (let [selected (selected-or-fail! runtime-environment)
            generation (manifest/rollback-transition! (:generation selected))
            override-snapshot
            (when (live-environment? runtime-environment)
              (install-live-service-override!))]
          (if (live-environment? runtime-environment)
            (commit-live-transition! runtime-environment selected generation
                                     override-snapshot)
            (publish-generation-under-lock!
             runtime-environment generation))))))

(defn restore! [runtime-environment]
  (with-selector-lock
    runtime-environment
    (fn []
      (let [selected (selected-or-fail! runtime-environment)
            generation (manifest/restore-transition! (:generation selected))
            override-snapshot
            (when (live-environment? runtime-environment)
              (install-live-service-override!))]
          (if (live-environment? runtime-environment)
            (commit-live-transition! runtime-environment selected generation
                                     override-snapshot)
            (if (= generation (:generation selected))
              selected
              (publish-generation-under-lock!
               runtime-environment generation)))))))

(defn print-status! [runtime-environment]
  (if-let [selected (read-selected-generation runtime-environment)]
    (do
      (println (str "generation=" (.getFileName ^Path (:root selected))))
      (println (str "selector=" (:active-selector runtime-environment)))
      (doseq [line (manifest/generation-status-lines! (:generation selected))]
        (println line))
      selected)
    (do
      (println "generation=none")
      (println (str "selector=" (:active-selector runtime-environment)))
      nil)))

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

(defn -main [& args]
  (let [runtime-environment (environment)
        command (first args)]
    (try
      (case command
        "status"
        (if (= 1 (count args))
          (print-status! runtime-environment)
          (usage!))

        "promote"
        (if (= 2 (count args))
          (do
            (promote! runtime-environment (second args))
            (print-status! runtime-environment))
          (usage!))

        "rollback"
        (if (= 1 (count args))
          (do
            (rollback! runtime-environment)
            (print-status! runtime-environment))
          (usage!))

        "restore"
        (if (= 1 (count args))
          (do
            (restore! runtime-environment)
            (print-status! runtime-environment))
          (usage!))

        "attest"
        (if (= 1 (count args))
          (let [{:keys [status]} (attest-selected-live! runtime-environment)]
            (println (str "usable_rpc=" status))
            (print-status! runtime-environment))
          (usage!))

        "launch"
        (if (= 1 (count args))
          (launch-current! runtime-environment)
          (usage!))

        "publish-runtime"
        (if (= 3 (count args))
          (let [attestation
                (publish-current-runtime!
                 runtime-environment (second args) (nth args 2))]
            (println
             (str "attested="
                  (get-in attestation [:identity :runtime-kind]))))
          (usage!))

        (usage!))
      (catch Throwable error
        (binding [*out* *err*]
          (println (str "north-store-runtime: " (.getMessage error)))
          (when-let [data (ex-data error)]
            (println (str "north-store-runtime: " (pr-str data))))
          (doseq [suppressed (.getSuppressed error)]
            (println (str "north-store-runtime: recovery: "
                          (.getMessage suppressed)))
            (when-let [data (ex-data suppressed)]
              (println (str "north-store-runtime: recovery: "
                            (pr-str data))))))
        (System/exit 2)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
