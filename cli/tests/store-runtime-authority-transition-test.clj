(ns north.store-runtime-authority-transition-test
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [north.store-runtime-manifest :as manifest])
  (:import [java.io File]
           [java.nio.file Files]
           [java.nio.file Path]
           [java.nio.file Paths]
           [java.nio.file.attribute FileAttribute]))

(defrecord Fixture [temp environment published])

(defn fixture-temp [r] (:temp r))

(defn fixture-environment [r] (:environment r))

(defn fixture-published [r] (:published r))

(defrecord RecoverySnapshot [selector generation-bytes client-bytes published-client-bytes generation-directories])

(defn recoverysnapshot-selector [r] (:selector r))

(defn recoverysnapshot-generation-bytes [r] (:generation-bytes r))

(defn recoverysnapshot-client-bytes [r] (:client-bytes r))

(defn recoverysnapshot-published-client-bytes [r] (:published-client-bytes r))

(defn recoverysnapshot-generation-directories [r] (:generation-directories r))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/store-runtime-generation.clj"))

(defn private-var [name]
  (or (ns-resolve 'north.store-runtime-generation name) (throw (ex-info "missing private runtime-generation var" {:symbol name}))))

(defn read-selected-generation [runtime-environment]
  ((private-var 'read-selected-generation) runtime-environment))

(defn promote! [runtime-environment ^String output]
  ((private-var 'promote!) runtime-environment output))

(defn rollback! [runtime-environment]
  ((private-var 'rollback!) runtime-environment))

(defn restore! [runtime-environment]
  ((private-var 'restore!) runtime-environment))

(defn manifest-var [name]
  (or (ns-resolve 'north.store-runtime-manifest name) (throw (ex-info "missing typed runtime-manifest var" {:symbol name}))))

(defn ^String required-string! [^String label value]
  (if (some? value) value (throw (ex-info (str label " is absent") {:label label}))))

(defn required-int! [^String label value]
  (if (some? value) value (throw (ex-info (str label " is absent") {:label label}))))

(defn required-manifest! [value]
  (if (some? value) value (throw (ex-info "JVM manifest is absent" {}))))

(def old-jvm (manifest/validate-promotion-source-member! (var-get (manifest-var 'promotion-source-jvm))))

(def ^String old-manifest-text (manifest/canonical-manifest-text! (required-manifest! (:manifest old-jvm))))

(def ^String old-jvm-output (required-string! "retained JVM output" (:output old-jvm)))

(def ^String old-jvm-nar-sha256 (required-string! "retained JVM NAR SHA-256" (:package-nar-sha256 old-jvm)))

(def ^String old-jvm-manifest-sha256 (required-string! "retained JVM manifest SHA-256" (:manifest-sha256 old-jvm)))

(def old-jvm-manifest-bytes (required-int! "retained JVM manifest byte count" (:manifest-bytes old-jvm)))

(def old-forward (manifest/->StoreRuntimeGeneration old-jvm manifest/accepted-native-runtime))

(def old-reverse (manifest/->StoreRuntimeGeneration manifest/accepted-native-runtime old-jvm))

(def ^String candidate-output "/nix/store/11111111111111111111111111111111-beagle-store-jvm")

(def candidate (manifest/promotion-candidate-jvm-runtime! candidate-output manifest/accepted-jvm-nar-sha256 manifest/accepted-jvm-manifest-sha256 manifest/accepted-runtime-manifest-text))

(def promoted (manifest/initial-promotion-transition! candidate))

(def base-selection {"BEAGLE_STORE_SPACE_ID" "authority-transition-test" "BEAGLE_STORE_SERVER_PORT" "47979" "BEAGLE_STORE_LOG" "/tmp/north-store-authority-transition.storelog"})

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj [label (boolean value)])
  nil))

(defn ^Boolean denied? [operation]
  (try
  (do
  (operation)
  false)
  (catch Throwable _
    true)))

(defn delete-tree! [file]
  (do
  (if (and (.isDirectory file) (not (Files/isSymbolicLink (.toPath file)))) (do
  (doseq [child (or (.listFiles file) (make-array File 0))]
  (delete-tree! child))))
  (Files/deleteIfExists (.toPath file))
  nil))

(defn ^Fixture seed-selected! [runtime-generation]
  (let [temp (.toFile (Files/createTempDirectory "north-store-authority-transition-" (make-array FileAttribute 0)))
   state-root (io/file temp "store-runtime")
   generation-root (io/file state-root "generations" "source")
   selector (io/file state-root "active")
   published (io/file temp "published.env")
   selection ((var-get (private-var 'client-values-with!)) manifest/validate-promotion-source-member! (:current runtime-generation) base-selection)
   ^String client-text ((var-get (private-var 'client-text!)) selection)]
  (.mkdirs generation-root)
  (spit (io/file generation-root "generation.edn") (str (pr-str runtime-generation) "\n"))
  (spit (io/file generation-root "client.env") client-text)
  (Files/createSymbolicLink (.toPath selector) (Paths/get "generations/source" (make-array String 0)) (make-array FileAttribute 0))
  (spit published client-text)
  (->Fixture temp (manifest/derive-runtime-environment! (.getCanonicalPath state-root)) (.getCanonicalPath published))))

(defn ^String selector-target [^Fixture fixture]
  (str (Files/readSymbolicLink (.toPath (io/file (:active-selector (fixture-environment fixture)))))))

(defn generation-directory-names [^Fixture fixture]
  (let [directory (io/file (:generations-root (fixture-environment fixture)))]
  (vec (sort (mapv (fn [entry] (.getName entry)) (or (.listFiles directory) (make-array File 0)))))))

(defn file-bytes [file]
  (vec (Files/readAllBytes (.toPath file))))

(defn ^RecoverySnapshot recovery-snapshot [^Fixture fixture]
  (let [environment (fixture-environment fixture)
   generation-root (io/file (:generations-root environment) "source")]
  (->RecoverySnapshot (selector-target fixture) (file-bytes (io/file generation-root "generation.edn")) (file-bytes (io/file generation-root "client.env")) (file-bytes (io/file (fixture-published fixture))) (generation-directory-names fixture))))

(defn with-published-selection [^Fixture fixture operation]
  (with-redefs-fn {(private-var 'published-selection-path) (fn [] (fixture-published fixture))} operation))

(defn promote-fixture!
  ([^Fixture fixture]
    (promote-fixture! fixture {}))
  ([^Fixture fixture extra-redefs]
    (with-redefs-fn (merge {(private-var 'published-selection-path) (fn [] (fixture-published fixture)) (private-var 'observe-jvm-runtime!) (fn [^String output] (do
  (if (not (= output candidate-output)) (do
  (throw (ex-info "wrong candidate output" {:output output}))))
  candidate)) (private-var 'observe-jvm-package!) (fn [^String output] (do
  (if (not (= output old-jvm-output)) (do
  (throw (ex-info "wrong promotion source output" {:output output}))))
  {:output output :nar-sha256 old-jvm-nar-sha256 :manifest-sha256 old-jvm-manifest-sha256 :manifest-text old-manifest-text}))} extra-redefs) (fn [] (promote! (fixture-environment fixture) candidate-output)))))

(def ^String alternate-oid (apply str (repeat 40 "c")))

(def ^String alternate-sha (apply str (repeat 64 "c")))

(def ^String alternate-nar (str "sha256-" (apply str (repeat 43 "A")) "="))

(def ^String alternate-output "/nix/store/22222222222222222222222222222222-beagle-store-jvm")

(def old-jvm-mutants [(assoc old-jvm :output alternate-output :manifest-path (manifest/manifest-path-for alternate-output)) (assoc old-jvm :package-nar-sha256 alternate-nar) (assoc old-jvm :beagle-revision alternate-oid) (assoc old-jvm :beagle-tree alternate-oid) (assoc old-jvm :manifest-path (str (:manifest-path old-jvm) ".other")) (assoc old-jvm :manifest-bytes (inc old-jvm-manifest-bytes)) (assoc old-jvm :manifest-sha256 alternate-sha) (assoc-in old-jvm [:manifest :beagle-revision] alternate-oid) (assoc-in old-jvm [:manifest :source-tree] alternate-oid)])

(let [^String expected-root (.getCanonicalPath (io/file (System/getProperty "user.home") "code" "north" "main"))
   ^String authority-root (var-get (private-var 'canonical-live-north-root))
   validate-root! (private-var 'validate-live-north-root!)]
  (check! "live Store service authority admits only derived canonical main" (and (= expected-root authority-root) (= expected-root (validate-root! expected-root)) (denied? (fn [] (validate-root! (str expected-root "/worktrees/test")))))))

(check! "normal runtime authority accepts both exact retained JVM orientations" (and (= old-forward (manifest/validate-runtime-generation! old-forward)) (= old-reverse (manifest/validate-runtime-generation! old-reverse))))

(check! "promotion source accepts both exact retained generation orientations" (and (= old-forward (manifest/validate-promotion-source-generation! old-forward)) (= old-reverse (manifest/validate-promotion-source-generation! old-reverse))))

(check! "every prior JVM identity seam fails closed" (every? true? (mapv (fn [mutant] (denied? (fn [] (manifest/validate-promotion-source-generation! (manifest/->StoreRuntimeGeneration mutant manifest/accepted-native-runtime))))) old-jvm-mutants)))

(check! "a substituted Native recovery member fails closed" (denied? (fn [] (manifest/validate-promotion-source-generation! (manifest/->StoreRuntimeGeneration old-jvm (assoc manifest/accepted-native-runtime :server-sha256 alternate-sha))))))

(check! "mixed and same-kind promotion source shapes fail closed" (and (denied? (fn [] (manifest/validate-promotion-source-generation! (manifest/->StoreRuntimeGeneration old-jvm candidate)))) (denied? (fn [] (manifest/validate-promotion-source-generation! (manifest/->StoreRuntimeGeneration old-jvm old-jvm))))))

(check! "the retained JVM cannot be admitted as a fresh promotion candidate" (and (denied? (fn [] (manifest/promotion-candidate-jvm-runtime! old-jvm-output old-jvm-nar-sha256 old-jvm-manifest-sha256 old-manifest-text))) (denied? (fn [] (manifest/promote-authority-transition! old-forward old-jvm)))))

(defn check-promotion-source! [^String label source]
  (let [next (manifest/promote-authority-transition! source candidate)]
  (check! (str label " source emits only the current JVM/Native pair") (= promoted next))))

(check-promotion-source! "JVM-current" old-forward)

(check-promotion-source! "Native-current" old-reverse)

(let [^Fixture fixture (seed-selected! old-forward)]
  (try
  (let [read-source (var-get (private-var 'read-selected-promotion-source))]
  (check! "an actual serialized predecessor record is normally launchable" (= old-forward (:generation (read-selected-generation (fixture-environment fixture)))))
  (check! "an actual serialized predecessor remains promotion-readable" (= old-forward (:generation (read-source (fixture-environment fixture))))))
  (let [selected (promote-fixture! fixture)
   ^String first-target (selector-target fixture)]
  (check! "serialized predecessor promotion emits a normal current generation" (and (= promoted (:generation selected)) (= promoted (:generation (read-selected-generation (fixture-environment fixture)))) (not (str/includes? (slurp (str (.resolve ^Path (:root selected) "generation.edn"))) (:beagle-revision old-jvm)))))
  (promote-fixture! fixture)
  (check! "re-promoting the current authority is selector-idempotent" (= first-target (selector-target fixture))))
  (with-published-selection fixture (fn [] (do
  (rollback! (fixture-environment fixture))
  (check! "the promoted generation rolls back to exact Native" (= "native" (manifest/runtime-member-kind (:current (:generation (read-selected-generation (fixture-environment fixture)))))))
  (restore! (fixture-environment fixture))
  (check! "Native rollback restores the exact current JVM generation" (= promoted (:generation (read-selected-generation (fixture-environment fixture))))))))
  (finally
    (delete-tree! (fixture-temp fixture)))))

(let [^Fixture fixture (seed-selected! old-reverse)]
  (try
  (check! "a Native-current serialized predecessor promotes to JVM current" (= promoted (:generation (promote-fixture! fixture))))
  (finally
    (delete-tree! (fixture-temp fixture)))))

(defn check-promotion-failure! [^String label failure]
  (let [^Fixture fixture (seed-selected! old-forward)]
  (try
  (check! (str label " promotion failure is reported") (denied? (fn [] (promote-fixture! fixture failure))))
  (check! (str label " promotion failure preserves the predecessor selector") (= "generations/source" (selector-target fixture)))
  (finally
    (delete-tree! (fixture-temp fixture))))))

(check-promotion-failure! "pre-selector" {(private-var 'write-generation!) (fn [& $beagle$rest$host] (let [_arguments (vec $beagle$rest$host)]
  (throw (ex-info "synthetic pre-selector failure" {}))))})

(check-promotion-failure! "post-selector" {(private-var '*after-selector-move!*) (fn [_selection] (throw (ex-info "synthetic post-selector failure" {})))})

(let [^Fixture fixture (seed-selected! old-forward)
   environment (fixture-environment fixture)
   commit-live-transition! (var-get (private-var 'commit-live-transition!))
   write-generation! (var-get (private-var 'write-generation!))
   write-file-fsynced! (var-get (private-var 'write-file-fsynced!))
   ^RecoverySnapshot original (recovery-snapshot fixture)
   original-cutover-error (ex-info "synthetic cutover restart deadline" {:phase :cutover})
   propagated-error (atom nil)
   restart-commands (atom [])
   recovery-attestations (atom 0)
   recovery-events (atom [])
   recovered-generation (atom nil)
   generation-writes (atom 0)
   generation-file-writes (atom 0)
   writes-at-cutover-failure (atom nil)
   file-writes-at-cutover-failure (atom nil)
   directories-at-cutover-failure (atom nil)]
  (try
  (with-redefs-fn {(private-var 'published-selection-path) (fn [] (fixture-published fixture)) (private-var 'write-generation!) (fn [runtime-environment generation selection] (do
  (swap! generation-writes inc)
  (write-generation! runtime-environment generation selection))) (private-var 'write-file-fsynced!) (fn [target bytes] (do
  (swap! generation-file-writes inc)
  (write-file-fsynced! target bytes))) (private-var 'run-command!) (fn [arguments] (let [attempt (count (swap! restart-commands conj arguments))]
  (swap! recovery-events conj (if (= 1 attempt) :cutover-restart :recovery-restart))
  (if (= 1 attempt) (do
  (reset! writes-at-cutover-failure (deref generation-writes))
  (reset! file-writes-at-cutover-failure (deref generation-file-writes))
  (reset! directories-at-cutover-failure (generation-directory-names fixture))
  (throw original-cutover-error)) ""))) (private-var 'attest-selected-live!) (fn [runtime-environment] (do
  (swap! recovery-attestations inc)
  (swap! recovery-events conj :recovery-attestation)
  (reset! recovered-generation (:generation (read-selected-generation runtime-environment)))
  {:attestation :synthetic-recovery :status "up|1|1|ready|jvm"}))} (fn [] (try
  (commit-live-transition! environment ((var-get (private-var 'read-selected-promotion-source)) environment) promoted base-selection)
  (catch Throwable error
    (reset! propagated-error error)))))
  (let [^RecoverySnapshot recovered (recovery-snapshot fixture)]
  (check! "failed cutover restores the exact predecessor selector" (= (recoverysnapshot-selector original) (recoverysnapshot-selector recovered)))
  (check! "failed cutover preserves the exact predecessor generation bytes" (= (recoverysnapshot-generation-bytes original) (recoverysnapshot-generation-bytes recovered)))
  (check! "failed cutover restores exact predecessor client bytes" (and (= (recoverysnapshot-client-bytes original) (recoverysnapshot-client-bytes recovered)) (= (recoverysnapshot-published-client-bytes original) (recoverysnapshot-published-client-bytes recovered))))
  (check! "failed cutover performs one recovery restart and attestation" (and (= [["systemctl" "--user" "restart" "north-store.service"] ["systemctl" "--user" "restart" "north-store.service"]] (deref restart-commands)) (= 1 (deref recovery-attestations)) (= [:cutover-restart :recovery-restart :recovery-attestation] (deref recovery-events)) (= old-forward (deref recovered-generation))))
  (check! "failed cutover recovery writes no generation or directory" (and (= 1 (deref writes-at-cutover-failure)) (= (deref writes-at-cutover-failure) (deref generation-writes)) (= (deref file-writes-at-cutover-failure) (deref generation-file-writes)) (= (deref directories-at-cutover-failure) (recoverysnapshot-generation-directories recovered))))
  (check! "failed cutover propagates the original error" (identical? original-cutover-error (deref propagated-error))))
  (finally
    (delete-tree! (fixture-temp fixture)))))

(let [results (deref checks)
   passed (count (filter second results))]
  (doseq [[label ok] results]
  (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nStore runtime authority transition: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
