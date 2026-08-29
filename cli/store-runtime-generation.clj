(ns north.store-runtime-generation
  (:require [clojure.edn :as edn]
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

(defonce ^:private in-process-lock (Object.))

(def ^:dynamic *after-selector-move!*
  (fn [_selection] nil))

(def ^:private generation-file-name "generation.edn")
(def ^:private max-generation-bytes 32768)

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

(defn promote! [runtime-environment output]
  (with-selector-lock
    runtime-environment
    (fn []
      (let [candidate (observe-jvm-runtime! output)
            selected (read-selected-generation runtime-environment)
            generation (if selected
                         (manifest/promote-transition!
                          (:generation selected) candidate)
                         (manifest/initial-promotion-transition! candidate))]
        (if (and selected (= generation (:generation selected)))
          selected
          (publish-generation-under-lock!
           runtime-environment generation))))))

(defn rollback! [runtime-environment]
  (with-selector-lock
    runtime-environment
    (fn []
      (let [selected (selected-or-fail! runtime-environment)]
        (publish-generation-under-lock!
         runtime-environment
         (manifest/rollback-transition! (:generation selected)))))))

(defn restore! [runtime-environment]
  (with-selector-lock
    runtime-environment
    (fn []
      (let [selected (selected-or-fail! runtime-environment)
            generation (manifest/restore-transition! (:generation selected))]
        (if (= generation (:generation selected))
          selected
          (publish-generation-under-lock!
           runtime-environment generation))))))

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
    (println "       north-store-runtime restore"))
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

        (usage!))
      (catch Throwable error
        (binding [*out* *err*]
          (println (str "north-store-runtime: " (.getMessage error))))
        (System/exit 2)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
