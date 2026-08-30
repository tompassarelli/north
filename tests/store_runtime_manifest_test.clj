(require '[clojure.string :as str]
         '[north.store-runtime-manifest :as manifest])

(def output "/nix/store/11111111111111111111111111111111-beagle-store-jvm")
(def text manifest/accepted-runtime-manifest-text)
(def jvm
  (manifest/accepted-jvm-runtime!
   output
   manifest/accepted-jvm-nar-sha256
   manifest/accepted-jvm-manifest-sha256
   text))
(def native manifest/accepted-native-runtime)
(def generation (manifest/initial-promotion-transition! jvm))
(def rolled-back (manifest/rollback-transition! generation))
(def restored (manifest/restore-transition! rolled-back))
(def attestation (manifest/attest-runtime-manifest! text generation))

(defn throws? [operation]
  (try
    (operation)
    false
    (catch Exception _ true)))

(defn replace-once [source old new]
  (str/replace-first source old new))

(def other-sha (apply str (repeat 64 "c")))
(def other-oid (apply str (repeat 40 "c")))
(def other-nar (str "sha256-" (apply str (repeat 43 "A")) "="))

(def malformed-manifests
  [(replace-once text "format=" "unknown=")
   (replace-once text manifest/accepted-jvm-revision
                 (str "A" (subs manifest/accepted-jvm-revision 1)))
   (replace-once text "source_tree=" "source_tree=A")
   (replace-once text "heap_max_bytes=2147483648"
                 "heap_max_bytes=02147483648")
   (str/replace text "\n" "\r\n")
   (subs text 0 (dec (count text)))
   (str text "\n")])

(def checks
  [["accepted producer manifest remains exact and canonical"
    (and (= 349 (count text))
         (= text
            (manifest/canonical-manifest-text!
             manifest/accepted-current-runtime-manifest))
         (= manifest/accepted-current-runtime-manifest
            (manifest/parse-runtime-manifest! text)))]
   ["JVM member binds dynamic output to exact NAR revision tree and manifest"
    (and (= "jvm" (manifest/runtime-member-kind jvm))
         (= output (:output jvm))
         (= manifest/accepted-jvm-nar-sha256 (:package-nar-sha256 jvm))
         (= manifest/accepted-jvm-revision (:beagle-revision jvm))
         (= manifest/accepted-jvm-tree (:beagle-tree jvm))
         (= (manifest/manifest-path-for output) (:manifest-path jvm))
         (= manifest/accepted-jvm-manifest-sha256 (:manifest-sha256 jvm)))]
   ["JVM member derives one package-internal launcher environment"
    (and (= (str output "/libexec/bin/beagle")
            (manifest/jvm-dispatcher-path-for output))
         (= (str output "/libexec/store")
            (manifest/jvm-store-home-for output))
         (= (str output "/libexec/store/bin")
            (manifest/jvm-store-bin-for output))
         (= (str output "/libexec/store/out")
            (manifest/jvm-store-out-for output))
         (= (str output "/libexec/store/bin/beagle-store-server")
            (manifest/jvm-server-launcher-for output))
         (= (str output "/libexec/store/server.classpath")
            (manifest/jvm-server-classpath-file-for output)))]
   ["Native member derives its release-matched client"
    (and (= (str (:release-root native) "/out")
            (manifest/native-client-classpath-for (:release-root native)))
         (= (str (:release-root native) "/bin/beagle-store-cli.clj")
            (manifest/native-client-path-for (:release-root native))))]
   ["Native member is the exact retained recovery runtime"
    (and (= "native" (manifest/runtime-member-kind native))
         (= "48f38823e42694578587f5624d8be5db9f962a77"
            (:beagle-revision native))
         (= "7d4dd724e1ba4c107162a24d47aea0849be119a5"
            (:beagle-tree native))
         (= "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554"
            (:closure-sha256 native))
         (= "b3de9e5692ba73303da4f2e38432e6fe0debacd4cf46ac3033d059f713225b69"
            (:server-sha256 native)))]
   ["initial generation contains one JVM current and one Native previous"
    (and (= "jvm" (manifest/runtime-member-kind (:current generation)))
         (= "native" (manifest/runtime-member-kind (:previous generation))))]
   ["rollback swaps the complete pair"
    (and (= "native" (manifest/runtime-member-kind (:current rolled-back)))
         (= jvm (:previous rolled-back)))]
   ["restore deterministically returns the accepted JVM to current"
    (= generation restored)]
   ["promoting the selected JVM is idempotent"
    (= generation (manifest/promote-transition! generation jvm))]
   ["attestation retains exact generation and parsed fields"
    (and (= generation (:generation attestation))
         (= manifest/accepted-current-runtime-manifest
            (:manifest attestation)))]
   ["canonical environment derives the one state selector layout"
    (let [environment manifest/canonical-runtime-environment]
      (and (= "/home/tom/code/north-data/store-runtime"
              (:state-root environment))
           (= "/home/tom/code/north-data/store-runtime/generations"
              (:generations-root environment))
           (= "/home/tom/code/north-data/store-runtime/active"
              (:active-selector environment))))]
   ["malformed manifests fail closed"
    (every? true?
            (mapv #(throws? (fn [] (manifest/parse-runtime-manifest! %)))
                  malformed-manifests))]
   ["JVM producer substitutions fail closed"
    (every? true?
            [(throws? #(manifest/accepted-jvm-runtime!
                        output other-nar
                        manifest/accepted-jvm-manifest-sha256 text))
             (throws? #(manifest/accepted-jvm-runtime!
                        output manifest/accepted-jvm-nar-sha256 other-sha text))
             (throws? #(manifest/validate-runtime-member!
                        (assoc jvm :beagle-revision other-oid)))
             (throws? #(manifest/validate-runtime-member!
                        (assoc jvm :beagle-tree other-oid)))])]
   ["Native producer substitutions fail closed"
    (every? true?
            [(throws? #(manifest/validate-runtime-member!
                        (assoc native :server-sha256 other-sha)))
             (throws? #(manifest/validate-runtime-member!
                        (assoc native :closure-sha256 other-sha)))])]
   ["a generation cannot contain two members of one runtime kind"
    (and (throws? #(manifest/validate-runtime-generation!
                    (manifest/->StoreRuntimeGeneration jvm jvm)))
         (throws? #(manifest/validate-runtime-generation!
                    (manifest/->StoreRuntimeGeneration native native))))]
   ["status derives directly from the selected typed generation"
    (let [lines (manifest/generation-status-lines! generation)]
      (and (some #{"current.kind=jvm"} lines)
           (some #{"previous.kind=native"} lines)
           (some #{(str "current.output=" output)} lines)))]])

(let [failures (remove second checks)]
  (doseq [[name passed] checks]
    (println (if passed "  [PASS] " "  [FAIL] ") name))
  (if (empty? failures)
    (println "\nstore runtime manifest:" (count checks) "/" (count checks) "PASS")
    (do
      (println "\nstore runtime manifest:" (count failures) "FAILED")
      (System/exit 1))))
