#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[north.store-runtime-manifest :as manifest])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(load-file (str root "/cli/store-runtime-generation.clj"))
(require '[north.store-runtime-generation :as generation])

(defn private-var [symbol]
  (or (ns-resolve 'north.store-runtime-generation symbol)
      (throw (ex-info "missing private runtime-generation var" {:symbol symbol}))))

(defn manifest-var [symbol]
  (or (ns-resolve 'north.store-runtime-manifest symbol)
      (throw (ex-info "missing typed runtime-manifest var" {:symbol symbol}))))

(def old-jvm (var-get (manifest-var 'promotion-source-jvm)))
(def old-manifest-text
  (manifest/canonical-manifest-text! (:manifest old-jvm)))
(def old-forward
  (manifest/->StoreRuntimeGeneration old-jvm manifest/accepted-native-runtime))
(def old-reverse
  (manifest/->StoreRuntimeGeneration manifest/accepted-native-runtime old-jvm))

(def candidate-output
  "/nix/store/11111111111111111111111111111111-beagle-store-jvm")
(def candidate
  (manifest/accepted-jvm-runtime!
   candidate-output
   manifest/accepted-jvm-nar-sha256
   manifest/accepted-jvm-manifest-sha256
   manifest/accepted-runtime-manifest-text))
(def promoted
  (manifest/initial-promotion-transition! candidate))

(def base-selection
  {"BEAGLE_STORE_SPACE_ID" "authority-transition-test"
   "BEAGLE_STORE_SERVER_PORT" "47979"
   "BEAGLE_STORE_LOG" "/tmp/north-store-authority-transition.storelog"})

(def checks (atom []))

(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn denied? [operation]
  (try
    (operation)
    false
    (catch Throwable _ true)))

(defn delete-tree! [file]
  (when (and (.isDirectory file)
             (not (java.nio.file.Files/isSymbolicLink (.toPath file))))
    (doseq [child (or (.listFiles file) (make-array java.io.File 0))]
      (delete-tree! child)))
  (java.nio.file.Files/deleteIfExists (.toPath file)))

(defn seed-selected! [runtime-generation]
  (let [temp (.toFile
              (java.nio.file.Files/createTempDirectory
               "north-store-authority-transition-"
               (make-array java.nio.file.attribute.FileAttribute 0)))
        state-root (io/file temp "store-runtime")
        generation-root (io/file state-root "generations" "source")
        selector (io/file state-root "active")
        published (io/file temp "published.env")
        selection
        ((var-get (private-var 'client-values-with!))
         manifest/validate-promotion-source-member!
         (:current runtime-generation)
         base-selection)
        client-text ((var-get (private-var 'client-text!)) selection)]
    (.mkdirs generation-root)
    (spit (io/file generation-root "generation.edn")
          (str (pr-str runtime-generation) "\n"))
    (spit (io/file generation-root "client.env") client-text)
    (java.nio.file.Files/createSymbolicLink
     (.toPath selector)
     (java.nio.file.Paths/get "generations/source" (make-array String 0))
     (make-array java.nio.file.attribute.FileAttribute 0))
    (spit published client-text)
    {:temp temp
     :environment
     (manifest/derive-runtime-environment! (.getCanonicalPath state-root))
     :published (.getCanonicalPath published)}))

(defn selector-target [{:keys [environment]}]
  (str (java.nio.file.Files/readSymbolicLink
        (.toPath (io/file (:active-selector environment))))))

(defn with-published-selection [fixture operation]
  (with-redefs-fn
    {(private-var 'published-selection-path)
     (fn [] (:published fixture))}
    operation))

(defn promote-fixture!
  ([fixture] (promote-fixture! fixture {}))
  ([fixture extra-redefs]
   (with-redefs-fn
     (merge
      {(private-var 'published-selection-path)
       (fn [] (:published fixture))
       #'generation/observe-jvm-runtime!
       (fn [output]
         (when-not (= output candidate-output)
           (throw (ex-info "wrong candidate output" {:output output})))
         candidate)
       (private-var 'observe-jvm-package!)
       (fn [output]
         (when-not (= output (:output old-jvm))
           (throw (ex-info "wrong promotion source output" {:output output})))
         {:output output
          :nar-sha256 (:package-nar-sha256 old-jvm)
          :manifest-sha256 (:manifest-sha256 old-jvm)
          :manifest-text old-manifest-text})}
      extra-redefs)
     #(generation/promote! (:environment fixture) candidate-output))))

(def alternate-oid (apply str (repeat 40 "c")))
(def alternate-sha (apply str (repeat 64 "c")))
(def alternate-nar (str "sha256-" (apply str (repeat 43 "A")) "="))
(def alternate-output
  "/nix/store/22222222222222222222222222222222-beagle-store-jvm")

(def old-jvm-mutants
  [(assoc old-jvm :output alternate-output
                  :manifest-path (manifest/manifest-path-for alternate-output))
   (assoc old-jvm :package-nar-sha256 alternate-nar)
   (assoc old-jvm :beagle-revision alternate-oid)
   (assoc old-jvm :beagle-tree alternate-oid)
   (assoc old-jvm :manifest-path (str (:manifest-path old-jvm) ".other"))
   (assoc old-jvm :manifest-bytes (inc (:manifest-bytes old-jvm)))
   (assoc old-jvm :manifest-sha256 alternate-sha)
   (assoc-in old-jvm [:manifest :beagle-revision] alternate-oid)
   (assoc-in old-jvm [:manifest :source-tree] alternate-oid)])

(check! "normal runtime authority rejects the prior JVM generation"
        (denied? #(manifest/validate-runtime-generation! old-forward)))
(check! "promotion source accepts exactly both prior generation orientations"
        (and (= old-forward
                (manifest/validate-promotion-source-generation! old-forward))
             (= old-reverse
                (manifest/validate-promotion-source-generation! old-reverse))))
(check! "every prior JVM identity seam fails closed"
        (every?
         true?
         (mapv
          #(denied?
            (fn []
              (manifest/validate-promotion-source-generation!
               (manifest/->StoreRuntimeGeneration
                % manifest/accepted-native-runtime))))
          old-jvm-mutants)))
(check! "a substituted Native recovery member fails closed"
        (denied?
         #(manifest/validate-promotion-source-generation!
           (manifest/->StoreRuntimeGeneration
            old-jvm
            (assoc manifest/accepted-native-runtime
                   :server-sha256 alternate-sha)))))
(check! "mixed and same-kind promotion source shapes fail closed"
        (and
         (denied?
          #(manifest/validate-promotion-source-generation!
            (manifest/->StoreRuntimeGeneration old-jvm candidate)))
         (denied?
          #(manifest/validate-promotion-source-generation!
            (manifest/->StoreRuntimeGeneration old-jvm old-jvm)))))
(check! "the prior JVM cannot be observed as a fresh promotion candidate"
        (and
         (denied?
          #(manifest/accepted-jvm-runtime!
            (:output old-jvm)
            (:package-nar-sha256 old-jvm)
            (:manifest-sha256 old-jvm)
            old-manifest-text))
         (denied?
          #(manifest/promote-authority-transition! old-forward old-jvm))))

(doseq [[label source] [["JVM-current" old-forward]
                        ["Native-current" old-reverse]]]
  (let [next (manifest/promote-authority-transition! source candidate)]
    (check! (str label " source emits only the current JVM/Native pair")
            (= promoted next))))

(let [fixture (seed-selected! old-forward)]
  (try
    (let [read-source (var-get (private-var 'read-selected-promotion-source))]
      (check! "an actual serialized predecessor record is promotion-readable only"
              (and
               (denied?
                #(generation/read-selected-generation (:environment fixture)))
               (= old-forward
                  (:generation (read-source (:environment fixture)))))))
    (let [selected (promote-fixture! fixture)
          first-target (selector-target fixture)]
      (check! "serialized predecessor promotion emits a normal current generation"
              (and (= promoted (:generation selected))
                   (= promoted
                      (:generation
                       (generation/read-selected-generation
                        (:environment fixture))))
                   (not (re-find
                         #"6fcf9b92756b6213b792d5300cad004de9d10341"
                         (slurp (str (.resolve ^java.nio.file.Path
                                              (:root selected)
                                              "generation.edn")))))))
      (promote-fixture! fixture)
      (check! "re-promoting the current authority is selector-idempotent"
              (= first-target (selector-target fixture))))
    (with-published-selection
      fixture
      #(do
         (generation/rollback! (:environment fixture))
         (check! "the promoted generation rolls back to exact Native"
                 (= "native"
                    (manifest/runtime-member-kind
                     (:current
                      (:generation
                       (generation/read-selected-generation
                        (:environment fixture)))))))
         (generation/restore! (:environment fixture))
         (check! "Native rollback restores the exact current JVM generation"
                 (= promoted
                    (:generation
                     (generation/read-selected-generation
                      (:environment fixture)))))))
    (finally
      (delete-tree! (:temp fixture)))))

(let [fixture (seed-selected! old-reverse)]
  (try
    (check! "a Native-current serialized predecessor promotes to JVM current"
            (= promoted (:generation (promote-fixture! fixture))))
    (finally
      (delete-tree! (:temp fixture)))))

(doseq [[label failure]
        [["pre-selector"
          {(private-var 'write-generation!)
           (fn [& _] (throw (ex-info "synthetic pre-selector failure" {})))}]
         ["post-selector"
          {#'generation/*after-selector-move!*
           (fn [_] (throw (ex-info "synthetic post-selector failure" {})))}]]]
  (let [fixture (seed-selected! old-forward)]
    (try
      (check! (str label " promotion failure is reported")
              (denied? #(promote-fixture! fixture failure)))
      (check! (str label " promotion failure preserves the predecessor selector")
              (= "generations/source" (selector-target fixture)))
      (finally
        (delete-tree! (:temp fixture))))))

(let [fixture (seed-selected! old-forward)
      recovered (atom nil)
      restore-after-failure
      (var-get (private-var 'restore-selection-after-live-failure!))
      read-source (var-get (private-var 'read-selected-promotion-source))]
  (try
    (with-redefs-fn
      {(private-var 'published-selection-path)
       (fn [] (:published fixture))
       (private-var 'switch-live!)
       (fn [runtime-environment]
         (reset! recovered
                 (:generation
                  (generation/read-selected-generation runtime-environment))))}
      #(restore-after-failure
        (:environment fixture)
        (read-source (:environment fixture))
        promoted
        nil
        base-selection))
    (check! "failed live predecessor cutover recovers to launchable Native"
            (= (manifest/rollback-transition! promoted) @recovered))
    (finally
      (delete-tree! (:temp fixture)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nStore runtime authority transition: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
