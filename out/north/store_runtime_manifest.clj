(ns north.store-runtime-manifest
  (:require [clojure.string :as str]))

(def max-manifest-characters 4096)

(def ^String manifest-format "beagle-store-runtime/v1")

(def ^String manifest-engine "jvm-clojure")

(def ^String manifest-native-backend "experimental-non-production")

(def ^String manifest-heap-policy "fixed-xmx")

(def manifest-heap-max-bytes 2147483648)

(def ^String manifest-protocol "store-rpc")

(def ^String manifest-protocol-version "2.0")

(def ^String manifest-readiness "restore+listen+usable-rpc")

(def ^String manifest-stopping "before-drain")

(def ^String manifest-relative-path "libexec/store/runtime.manifest")

(def ^String north-user-state-root (str "/" "home" "/tom/code/north-data"))

(def ^String canonical-store-runtime-root (str north-user-state-root "/store-runtime"))

(def ^String store-artifacts-root (str north-user-state-root "/store-artifacts"))

(def ^String store-releases-root (str north-user-state-root "/beagle-store-releases"))

(defrecord StoreRuntimeManifest [format beagle-revision source-tree engine native-backend heap-policy heap-max-bytes protocol protocol-version readiness stopping])

(defn storeruntimemanifest-format [r] (:format r))

(defn storeruntimemanifest-beagle-revision [r] (:beagle-revision r))

(defn storeruntimemanifest-source-tree [r] (:source-tree r))

(defn storeruntimemanifest-engine [r] (:engine r))

(defn storeruntimemanifest-native-backend [r] (:native-backend r))

(defn storeruntimemanifest-heap-policy [r] (:heap-policy r))

(defn storeruntimemanifest-heap-max-bytes [r] (:heap-max-bytes r))

(defn storeruntimemanifest-protocol [r] (:protocol r))

(defn storeruntimemanifest-protocol-version [r] (:protocol-version r))

(defn storeruntimemanifest-readiness [r] (:readiness r))

(defn storeruntimemanifest-stopping [r] (:stopping r))

(defrecord JVMRuntimeAuthority [output package-nar-sha256 beagle-revision beagle-tree status-engine-token manifest-bytes manifest-sha256 manifest])

(defn jvmruntimeauthority-output [r] (:output r))

(defn jvmruntimeauthority-package-nar-sha256 [r] (:package-nar-sha256 r))

(defn jvmruntimeauthority-beagle-revision [r] (:beagle-revision r))

(defn jvmruntimeauthority-beagle-tree [r] (:beagle-tree r))

(defn jvmruntimeauthority-status-engine-token [r] (:status-engine-token r))

(defn jvmruntimeauthority-manifest-bytes [r] (:manifest-bytes r))

(defn jvmruntimeauthority-manifest-sha256 [r] (:manifest-sha256 r))

(defn jvmruntimeauthority-manifest [r] (:manifest r))

(def JVMRuntimeAuthorityPurpose-values #{::selected ::promotion-candidate})

;; RuntimeMember = JVM | Native
(defrecord JVM [output package-nar-sha256 beagle-revision beagle-tree manifest-path manifest-bytes manifest-sha256 manifest])

(defn jvm-output [r] (:output r))

(defn jvm-package-nar-sha256 [r] (:package-nar-sha256 r))

(defn jvm-beagle-revision [r] (:beagle-revision r))

(defn jvm-beagle-tree [r] (:beagle-tree r))

(defn jvm-manifest-path [r] (:manifest-path r))

(defn jvm-manifest-bytes [r] (:manifest-bytes r))

(defn jvm-manifest-sha256 [r] (:manifest-sha256 r))

(defn jvm-manifest [r] (:manifest r))
(defrecord Native [release-root beagle-revision beagle-tree artifact-root closure-sha256 server-artifact server-sha256])

(defn native-release-root [r] (:release-root r))

(defn native-beagle-revision [r] (:beagle-revision r))

(defn native-beagle-tree [r] (:beagle-tree r))

(defn native-artifact-root [r] (:artifact-root r))

(defn native-closure-sha256 [r] (:closure-sha256 r))

(defn native-server-artifact [r] (:server-artifact r))

(defn native-server-sha256 [r] (:server-sha256 r))

(defrecord StoreRuntimeGeneration [current previous])

(defn storeruntimegeneration-current [r] (:current r))

(defn storeruntimegeneration-previous [r] (:previous r))

(defrecord StoreRuntimeAttestation [generation manifest])

(defn storeruntimeattestation-generation [r] (:generation r))

(defn storeruntimeattestation-manifest [r] (:manifest r))

(defrecord StoreRuntimeEnvironment [state-root generations-root active-selector selector-lock])

(defn storeruntimeenvironment-state-root [r] (:state-root r))

(defn storeruntimeenvironment-generations-root [r] (:generations-root r))

(defn storeruntimeenvironment-active-selector [r] (:active-selector r))

(defn storeruntimeenvironment-selector-lock [r] (:selector-lock r))

(defn- fail [^String message code data]
  (throw (ex-info message (assoc data :type code))))

(defn- ^Boolean exact-lower-oid? [^String value]
  (some? (re-matches #"[0-9a-f]{40}" value)))

(defn- ^Boolean exact-sha256? [^String value]
  (some? (re-matches #"[0-9a-f]{64}" value)))

(defn- ^Boolean exact-nar-sha256? [^String value]
  (some? (re-matches #"sha256-[A-Za-z0-9+/]{43}=" value)))

(defn ^Boolean canonical-package-root? [^String value]
  (some? (re-matches #"/nix/store/[0-9abcdfghijklmnpqrsvwxyz]{32}-[A-Za-z0-9][A-Za-z0-9+._?=-]*" value)))

(defn ^String manifest-path-for [^String output]
  (str output "/" manifest-relative-path))

(defn ^String jvm-dispatcher-path-for [^String output]
  (str output "/libexec/bin/beagle"))

(defn ^String jvm-store-home-for [^String output]
  (str output "/libexec/store"))

(defn ^String jvm-store-bin-for [^String output]
  (str (jvm-store-home-for output) "/bin"))

(defn ^String jvm-store-out-for [^String output]
  (str (jvm-store-home-for output) "/out"))

(defn ^String jvm-server-launcher-for [^String output]
  (str (jvm-store-bin-for output) "/beagle-store-server"))

(defn ^String jvm-server-classpath-file-for [^String output]
  (str (jvm-store-home-for output) "/server.classpath"))

(defn ^String native-client-classpath-for [^String release-root]
  (str release-root "/out"))

(defn ^String native-client-path-for [^String release-root]
  (str release-root "/bin/beagle-store-cli.clj"))

(defn ^String release-path-for [^String beagle-revision]
  (str store-releases-root "/" beagle-revision))

(defn ^String native-artifact-path-for [^String closure-sha256]
  (str store-artifacts-root "/" closure-sha256))

(defn ^String native-server-path-for [^String artifact-root]
  (str artifact-root "/bin/beagle-store-server-native"))

(defn ^StoreRuntimeEnvironment derive-runtime-environment! [^String state-root]
  (if (or (str/blank? state-root) (not (str/starts-with? state-root "/")) (str/ends-with? state-root "/") (str/includes? state-root "\n") (str/includes? state-root "\r") (str/includes? state-root "/../") (str/ends-with? state-root "/..")) (fail "Store runtime state root must be a canonical absolute path" :north.store-runtime-manifest/invalid-state-root {:actual state-root}) (->StoreRuntimeEnvironment state-root (str state-root "/generations") (str state-root "/active") (str state-root "/.selector.lock"))))

(def ^StoreRuntimeEnvironment canonical-runtime-environment (derive-runtime-environment! canonical-store-runtime-root))

(defn- ^String require-text! [^String field ^String actual ^String expected]
  (if (= actual expected) actual (fail (str "Store runtime " field " mismatch") :north.store-runtime-manifest/field-mismatch {:field field :expected expected :actual actual})))

(defn- ^StoreRuntimeManifest validate-manifest-facts! [^StoreRuntimeManifest facts]
  (do
  (require-text! "manifest format" (:format facts) manifest-format)
  (if (exact-lower-oid? (:beagle-revision facts)) (:beagle-revision facts) (fail "Store runtime manifest beagle_revision must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-beagle-revision {:actual (:beagle-revision facts)}))
  (if (exact-lower-oid? (:source-tree facts)) (:source-tree facts) (fail "Store runtime manifest source_tree must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-source-tree {:actual (:source-tree facts)}))
  (require-text! "manifest engine" (:engine facts) manifest-engine)
  (require-text! "manifest native_backend" (:native-backend facts) manifest-native-backend)
  (require-text! "manifest heap_policy" (:heap-policy facts) manifest-heap-policy)
  (if (= (:heap-max-bytes facts) manifest-heap-max-bytes) (:heap-max-bytes facts) (fail "Store runtime manifest heap_max_bytes mismatch" :north.store-runtime-manifest/field-mismatch {:field "heap_max_bytes" :expected manifest-heap-max-bytes :actual (:heap-max-bytes facts)}))
  (require-text! "manifest protocol" (:protocol facts) manifest-protocol)
  (require-text! "manifest protocol_version" (:protocol-version facts) manifest-protocol-version)
  (require-text! "manifest readiness" (:readiness facts) manifest-readiness)
  (require-text! "manifest stopping" (:stopping facts) manifest-stopping)
  facts))

(defn ^String canonical-manifest-text! [^StoreRuntimeManifest facts]
  (let [^StoreRuntimeManifest checked (validate-manifest-facts! facts)]
  (str "format=" (:format checked) "\n" "beagle_revision=" (:beagle-revision checked) "\n" "source_tree=" (:source-tree checked) "\n" "engine=" (:engine checked) "\n" "native_backend=" (:native-backend checked) "\n" "heap_policy=" (:heap-policy checked) "\n" "heap_max_bytes=" (:heap-max-bytes checked) "\n" "protocol=" (:protocol checked) "\n" "protocol_version=" (:protocol-version checked) "\n" "readiness=" (:readiness checked) "\n" "stopping=" (:stopping checked) "\n")))

(defn- ^JVMRuntimeAuthority jvm-runtime-authority [output ^String package-nar-sha256 ^String beagle-revision ^String beagle-tree ^String source-tree ^String status-engine-token manifest-bytes ^String manifest-sha256]
  (->JVMRuntimeAuthority output package-nar-sha256 beagle-revision beagle-tree status-engine-token manifest-bytes manifest-sha256 (->StoreRuntimeManifest manifest-format beagle-revision source-tree manifest-engine manifest-native-backend manifest-heap-policy manifest-heap-max-bytes manifest-protocol manifest-protocol-version manifest-readiness manifest-stopping)))

(def ^:private ^JVMRuntimeAuthority current-jvm-authority (jvm-runtime-authority nil "sha256-RzF/vIttkiP84N27jQ5G0jCEuOhFcbzEawCguYfWZoE=" "4ca0e98ef21bd295d94a094a44d828d99695e3f5" "61816b7840479ada3983036a0239e18d2b2da47f" "49e178decedf97675dff19dcd7cfaac1881d35d0" "rpc/jvm" 349 "a1d0249d272b935887945c532fde61da28f754e46a71dd84bd0070d9d48c9188"))

(def ^String accepted-jvm-revision (:beagle-revision current-jvm-authority))

(def ^String accepted-jvm-tree (:beagle-tree current-jvm-authority))

(def ^String accepted-jvm-nar-sha256 (:package-nar-sha256 current-jvm-authority))

(def accepted-jvm-manifest-bytes (:manifest-bytes current-jvm-authority))

(def ^String accepted-jvm-manifest-sha256 (:manifest-sha256 current-jvm-authority))

(def ^StoreRuntimeManifest accepted-current-runtime-manifest (:manifest current-jvm-authority))

(def ^String accepted-runtime-manifest-text (canonical-manifest-text! accepted-current-runtime-manifest))

(def accepted-native-runtime (->Native (release-path-for "48f38823e42694578587f5624d8be5db9f962a77") "48f38823e42694578587f5624d8be5db9f962a77" "7d4dd724e1ba4c107162a24d47aea0849be119a5" (native-artifact-path-for "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554") "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554" (native-server-path-for (native-artifact-path-for "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554")) "b3de9e5692ba73303da4f2e38432e6fe0debacd4cf46ac3033d059f713225b69"))

(def ^:private ^String promotion-source-output "/nix/store/gdlwjf2ih8cvk3piibk5c40lach3l3dc-beagle-store-jvm-composite-1-e5ce5d1c70b96bdd04fc346687df1fc677ea4a88")

(def ^:private ^JVMRuntimeAuthority retained-jvm-authority (jvm-runtime-authority promotion-source-output "sha256-TtoGeKe1JaARj0LZzc34i2GeaPs2TFjgHtGBpcmqFW4=" "e5ce5d1c70b96bdd04fc346687df1fc677ea4a88" "737e306b48f3021d40e5e88e2a8177c477f980a3" "e3dfdddd02969deafab75acd7f01497a5f000a26" "rpc/jvm" 349 "4486eeef08fd538b526a3322abaa31707dbb3bdb7eb8d41c33b42b9670c63fd6"))

(def ^:private jvm-authorities-by-purpose {:selected [current-jvm-authority retained-jvm-authority] :promotion-candidate [current-jvm-authority]})

(def ^:private ^StoreRuntimeManifest promotion-source-manifest (:manifest retained-jvm-authority))

(def ^:private promotion-source-jvm (->JVM promotion-source-output (:package-nar-sha256 retained-jvm-authority) (:beagle-revision retained-jvm-authority) (:beagle-tree retained-jvm-authority) (manifest-path-for promotion-source-output) (:manifest-bytes retained-jvm-authority) (:manifest-sha256 retained-jvm-authority) promotion-source-manifest))

(defn- ^String line-value [^String line ^String field]
  (let [^String prefix (str field "=")]
  (if (str/starts-with? line prefix) (subs line (count prefix)) (fail (str "Store runtime manifest expected ordered field " field) :north.store-runtime-manifest/noncanonical-fields {:field field :line line}))))

(defn ^StoreRuntimeManifest parse-runtime-manifest! [^String text]
  (if (> (count text) max-manifest-characters) (fail "Store runtime manifest exceeds the bounded input limit" :north.store-runtime-manifest/input-too-large {:maximum max-manifest-characters :actual (count text)}) (if (str/includes? text "\r") (fail "Store runtime manifest must contain LF line endings and no CR characters" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [lines (vec (str/split text #"\n" -1))]
  (if (not (= (count lines) 12)) (fail "Store runtime manifest must contain exactly eleven ordered LF-terminated fields" :north.store-runtime-manifest/noncanonical-fields {:line-count (count lines)}) (if (not (= (nth lines 11) "")) (fail "Store runtime manifest must end with one LF" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [^String heap-text (line-value (nth lines 6) "heap_max_bytes")
   ^StoreRuntimeManifest facts (->StoreRuntimeManifest (line-value (nth lines 0) "format") (line-value (nth lines 1) "beagle_revision") (line-value (nth lines 2) "source_tree") (line-value (nth lines 3) "engine") (line-value (nth lines 4) "native_backend") (line-value (nth lines 5) "heap_policy") (if (= heap-text "2147483648") manifest-heap-max-bytes (fail "Store runtime manifest heap_max_bytes is not canonical" :north.store-runtime-manifest/noncanonical-integer {:actual heap-text})) (line-value (nth lines 7) "protocol") (line-value (nth lines 8) "protocol_version") (line-value (nth lines 9) "readiness") (line-value (nth lines 10) "stopping"))]
  (validate-manifest-facts! facts))))))))

(defn- validate-jvm-authority! [member ^JVMRuntimeAuthority authority]
  (let [match__0 member]
  (cond
    (instance? JVM match__0) (let [output (:output match__0) package-nar-sha256 (:package-nar-sha256 match__0) beagle-revision (:beagle-revision match__0) beagle-tree (:beagle-tree match__0) manifest-path (:manifest-path match__0) manifest-bytes (:manifest-bytes match__0) manifest-sha256 (:manifest-sha256 match__0) manifest (:manifest match__0)] (do
  (if (canonical-package-root? output) output (fail "Store JVM output is not a canonical immutable /nix/store root" :north.store-runtime-manifest/invalid-package-root {:actual output}))
  (if (exact-nar-sha256? package-nar-sha256) package-nar-sha256 (fail "Store JVM NAR is not a canonical SHA-256 SRI" :north.store-runtime-manifest/invalid-package-nar {:actual package-nar-sha256}))
  (if (or (not (some? (:output authority))) (= output (:output authority))) output (fail "Store JVM output does not equal the exact runtime authority" :north.store-runtime-manifest/field-mismatch {:field "JVM output" :expected (:output authority) :actual output}))
  (require-text! "JVM NAR" package-nar-sha256 (:package-nar-sha256 authority))
  (require-text! "JVM revision" beagle-revision (:beagle-revision authority))
  (require-text! "JVM tree" beagle-tree (:beagle-tree authority))
  (require-text! "JVM manifest path" manifest-path (manifest-path-for output))
  (if (= manifest-bytes (:manifest-bytes authority)) manifest-bytes (fail "Store JVM manifest byte count mismatch" :north.store-runtime-manifest/manifest-size-mismatch {:expected (:manifest-bytes authority) :actual manifest-bytes}))
  (if (exact-sha256? manifest-sha256) manifest-sha256 (fail "Store JVM manifest SHA-256 is not canonical" :north.store-runtime-manifest/invalid-manifest-sha256 {:actual manifest-sha256}))
  (require-text! "JVM manifest SHA-256" manifest-sha256 (:manifest-sha256 authority))
  (if (= (validate-manifest-facts! manifest) (:manifest authority)) member (fail "Store JVM manifest facts do not equal the exact runtime authority" :north.store-runtime-manifest/binding-mismatch {:expected (:manifest authority) :actual manifest}))))
    (instance? Native match__0) (let [_ (:release-root match__0) _ (:beagle-revision match__0) _ (:beagle-tree match__0) _ (:artifact-root match__0) _ (:closure-sha256 match__0) _ (:server-artifact match__0) _ (:server-sha256 match__0)] (fail "Store JVM authority cannot validate a Native member" :north.store-runtime-manifest/invalid-generation-shape {})))))

(defn- ^JVMRuntimeAuthority jvm-authority-for-revision! [purpose ^String beagle-revision]
  (let [authorities (get jvm-authorities-by-purpose purpose [])
   matches (filterv (fn [^JVMRuntimeAuthority authority] (= beagle-revision (:beagle-revision authority))) authorities)]
  (if (= (count matches) 1) (nth matches 0) (fail "Store JVM revision is not admitted for this runtime purpose" :north.store-runtime-manifest/jvm-authority-mismatch {:purpose purpose :actual beagle-revision}))))

(defn- validate-jvm-member-for-purpose! [member purpose]
  (let [match__1 member]
  (cond
    (instance? JVM match__1) (let [_ (:output match__1) _ (:package-nar-sha256 match__1) beagle-revision (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:manifest-path match__1) _ (:manifest-bytes match__1) _ (:manifest-sha256 match__1) _ (:manifest match__1)] (validate-jvm-authority! member (jvm-authority-for-revision! purpose beagle-revision)))
    (instance? Native match__1) (let [_ (:release-root match__1) _ (:beagle-revision match__1) _ (:beagle-tree match__1) _ (:artifact-root match__1) _ (:closure-sha256 match__1) _ (:server-artifact match__1) _ (:server-sha256 match__1)] (fail "Store JVM purpose cannot validate a Native member" :north.store-runtime-manifest/invalid-generation-shape {:purpose purpose})))))

(defn validate-runtime-member! [member]
  (let [match__2 member]
  (cond
    (instance? JVM match__2) (let [_ (:output match__2) _ (:package-nar-sha256 match__2) _ (:beagle-revision match__2) _ (:beagle-tree match__2) _ (:manifest-path match__2) _ (:manifest-bytes match__2) _ (:manifest-sha256 match__2) _ (:manifest match__2)] (validate-jvm-member-for-purpose! member :selected))
    (instance? Native match__2) (let [release-root (:release-root match__2) beagle-revision (:beagle-revision match__2) beagle-tree (:beagle-tree match__2) artifact-root (:artifact-root match__2) closure-sha256 (:closure-sha256 match__2) server-artifact (:server-artifact match__2) server-sha256 (:server-sha256 match__2)] (do
  (if (exact-lower-oid? beagle-revision) beagle-revision (fail "Store Native revision is not an exact lowercase Git object id" :north.store-runtime-manifest/invalid-native-revision {:actual beagle-revision}))
  (if (exact-lower-oid? beagle-tree) beagle-tree (fail "Store Native tree is not an exact lowercase Git object id" :north.store-runtime-manifest/invalid-native-tree {:actual beagle-tree}))
  (if (exact-sha256? closure-sha256) closure-sha256 (fail "Store Native closure SHA-256 is not canonical" :north.store-runtime-manifest/invalid-native-closure {:actual closure-sha256}))
  (if (exact-sha256? server-sha256) server-sha256 (fail "Store Native server SHA-256 is not canonical" :north.store-runtime-manifest/invalid-native-server {:actual server-sha256}))
  (require-text! "Native release root" release-root (release-path-for beagle-revision))
  (require-text! "Native artifact root" artifact-root (native-artifact-path-for closure-sha256))
  (require-text! "Native server artifact" server-artifact (native-server-path-for artifact-root))
  (if (= member accepted-native-runtime) member (fail "Store Native member does not equal the accepted recovery runtime" :north.store-runtime-manifest/native-mismatch {:expected accepted-native-runtime :actual member})))))))

(defn ^String runtime-member-kind [member]
  (let [match__3 member]
  (cond
    (instance? JVM match__3) (let [_ (:output match__3) _ (:package-nar-sha256 match__3) _ (:beagle-revision match__3) _ (:beagle-tree match__3) _ (:manifest-path match__3) _ (:manifest-bytes match__3) _ (:manifest-sha256 match__3) _ (:manifest match__3)] "jvm")
    (instance? Native match__3) (let [_ (:release-root match__3) _ (:beagle-revision match__3) _ (:beagle-tree match__3) _ (:artifact-root match__3) _ (:closure-sha256 match__3) _ (:server-artifact match__3) _ (:server-sha256 match__3)] "native"))))

(defn ^String expected-store-status-engine-token! [member]
  (let [checked (validate-runtime-member! member)]
  (let [match__4 checked]
  (cond
    (instance? JVM match__4) (let [_ (:output match__4) _ (:package-nar-sha256 match__4) beagle-revision (:beagle-revision match__4) _ (:beagle-tree match__4) _ (:manifest-path match__4) _ (:manifest-bytes match__4) _ (:manifest-sha256 match__4) _ (:manifest match__4)] (:status-engine-token (jvm-authority-for-revision! :selected beagle-revision)))
    (instance? Native match__4) (let [_ (:release-root match__4) _ (:beagle-revision match__4) _ (:beagle-tree match__4) _ (:artifact-root match__4) _ (:closure-sha256 match__4) _ (:server-artifact match__4) _ (:server-sha256 match__4)] "native")))))

(defn ^StoreRuntimeGeneration validate-runtime-generation! [^StoreRuntimeGeneration generation]
  (let [current (validate-runtime-member! (:current generation))
   previous (validate-runtime-member! (:previous generation))]
  (let [match__5 current]
  (cond
    (instance? JVM match__5) (let [_ (:output match__5) _ (:package-nar-sha256 match__5) _ (:beagle-revision match__5) _ (:beagle-tree match__5) _ (:manifest-path match__5) _ (:manifest-bytes match__5) _ (:manifest-sha256 match__5) _ (:manifest match__5)] (let [match__6 previous]
  (cond
    (instance? Native match__6) (let [_ (:release-root match__6) _ (:beagle-revision match__6) _ (:beagle-tree match__6) _ (:artifact-root match__6) _ (:closure-sha256 match__6) _ (:server-artifact match__6) _ (:server-sha256 match__6)] generation)
    (instance? JVM match__6) (let [_ (:output match__6) _ (:package-nar-sha256 match__6) _ (:beagle-revision match__6) _ (:beagle-tree match__6) _ (:manifest-path match__6) _ (:manifest-bytes match__6) _ (:manifest-sha256 match__6) _ (:manifest match__6)] (fail "Store runtime generation must contain one JVM and one Native member" :north.store-runtime-manifest/invalid-generation-shape {})))))
    (instance? Native match__5) (let [_ (:release-root match__5) _ (:beagle-revision match__5) _ (:beagle-tree match__5) _ (:artifact-root match__5) _ (:closure-sha256 match__5) _ (:server-artifact match__5) _ (:server-sha256 match__5)] (let [match__7 previous]
  (cond
    (instance? JVM match__7) (let [_ (:output match__7) _ (:package-nar-sha256 match__7) _ (:beagle-revision match__7) _ (:beagle-tree match__7) _ (:manifest-path match__7) _ (:manifest-bytes match__7) _ (:manifest-sha256 match__7) _ (:manifest match__7)] generation)
    (instance? Native match__7) (let [_ (:release-root match__7) _ (:beagle-revision match__7) _ (:beagle-tree match__7) _ (:artifact-root match__7) _ (:closure-sha256 match__7) _ (:server-artifact match__7) _ (:server-sha256 match__7)] (fail "Store runtime generation must contain one JVM and one Native member" :north.store-runtime-manifest/invalid-generation-shape {})))))))))

(defn ^Boolean promotion-source-generation? [^StoreRuntimeGeneration generation]
  (or (and (= (:current generation) promotion-source-jvm) (= (:previous generation) accepted-native-runtime)) (and (= (:current generation) accepted-native-runtime) (= (:previous generation) promotion-source-jvm))))

(defn validate-promotion-source-member! [member]
  (validate-runtime-member! member))

(defn ^StoreRuntimeGeneration validate-promotion-source-generation! [^StoreRuntimeGeneration generation]
  (validate-runtime-generation! generation))

(defn attest-promotion-source-runtime! [member ^String observed-nar-sha256 ^String observed-manifest-sha256 ^String manifest-text]
  (let [checked (validate-jvm-authority! member retained-jvm-authority)
   ^StoreRuntimeManifest actual (parse-runtime-manifest! manifest-text)
   ^String expected-text (canonical-manifest-text! promotion-source-manifest)]
  (if (and (= checked promotion-source-jvm) (= observed-nar-sha256 (:package-nar-sha256 retained-jvm-authority)) (= observed-manifest-sha256 (:manifest-sha256 retained-jvm-authority)) (= (count manifest-text) (:manifest-bytes retained-jvm-authority)) (= manifest-text expected-text) (= actual promotion-source-manifest)) checked (fail "Selected Store promotion source differs from its exact package authority" :north.store-runtime-manifest/promotion-source-mismatch {}))))

(defn- jvm-runtime-for-purpose! [purpose ^String output ^String observed-nar-sha256 ^String observed-manifest-sha256 ^String manifest-text]
  (let [^StoreRuntimeManifest facts (parse-runtime-manifest! manifest-text)
   ^JVMRuntimeAuthority authority (jvm-authority-for-revision! purpose (:beagle-revision facts))
   ^String expected-text (canonical-manifest-text! (:manifest authority))
   member (->JVM output observed-nar-sha256 (:beagle-revision authority) (:beagle-tree authority) (manifest-path-for output) (count manifest-text) observed-manifest-sha256 facts)]
  (if (= manifest-text expected-text) (validate-jvm-member-for-purpose! member purpose) (fail "Store JVM manifest bytes do not equal the admitted producer text" :north.store-runtime-manifest/manifest-bytes-mismatch {:purpose purpose}))))

(defn accepted-jvm-runtime! [^String output ^String observed-nar-sha256 ^String observed-manifest-sha256 ^String manifest-text]
  (jvm-runtime-for-purpose! :selected output observed-nar-sha256 observed-manifest-sha256 manifest-text))

(defn promotion-candidate-jvm-runtime! [^String output ^String observed-nar-sha256 ^String observed-manifest-sha256 ^String manifest-text]
  (jvm-runtime-for-purpose! :promotion-candidate output observed-nar-sha256 observed-manifest-sha256 manifest-text))

(defn ^StoreRuntimeGeneration accepted-runtime-generation-for! [^String output ^String observed-nar-sha256 ^String observed-manifest-sha256 ^String manifest-text]
  (validate-runtime-generation! (->StoreRuntimeGeneration (accepted-jvm-runtime! output observed-nar-sha256 observed-manifest-sha256 manifest-text) accepted-native-runtime)))

(defn- jvm-member! [^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! generation)]
  (let [match__8 (:current checked)]
  (cond
    (instance? JVM match__8) (let [_ (:output match__8) _ (:package-nar-sha256 match__8) _ (:beagle-revision match__8) _ (:beagle-tree match__8) _ (:manifest-path match__8) _ (:manifest-bytes match__8) _ (:manifest-sha256 match__8) _ (:manifest match__8)] (:current checked))
    (instance? Native match__8) (let [_ (:release-root match__8) _ (:beagle-revision match__8) _ (:beagle-tree match__8) _ (:artifact-root match__8) _ (:closure-sha256 match__8) _ (:server-artifact match__8) _ (:server-sha256 match__8)] (:previous checked))))))

(defn- native-member! [^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! generation)]
  (let [match__9 (:current checked)]
  (cond
    (instance? JVM match__9) (let [_ (:output match__9) _ (:package-nar-sha256 match__9) _ (:beagle-revision match__9) _ (:beagle-tree match__9) _ (:manifest-path match__9) _ (:manifest-bytes match__9) _ (:manifest-sha256 match__9) _ (:manifest match__9)] (:previous checked))
    (instance? Native match__9) (let [_ (:release-root match__9) _ (:beagle-revision match__9) _ (:beagle-tree match__9) _ (:artifact-root match__9) _ (:closure-sha256 match__9) _ (:server-artifact match__9) _ (:server-sha256 match__9)] (:current checked))))))

(defn ^StoreRuntimeAttestation attest-runtime-manifest! [^String text ^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! generation)
   member (jvm-member! checked)
   ^StoreRuntimeManifest actual (parse-runtime-manifest! text)]
  (let [match__10 member]
  (cond
    (instance? JVM match__10) (let [_ (:output match__10) _ (:package-nar-sha256 match__10) _ (:beagle-revision match__10) _ (:beagle-tree match__10) _ (:manifest-path match__10) manifest-bytes (:manifest-bytes match__10) _ (:manifest-sha256 match__10) expected (:manifest match__10)] (let [^String expected-text (canonical-manifest-text! expected)]
  (if (not (= (count text) manifest-bytes)) (fail "Store runtime manifest byte count does not equal the accepted binding" :north.store-runtime-manifest/manifest-size-mismatch {:expected manifest-bytes :actual (count text)}) (if (not (= text expected-text)) (fail "Store runtime manifest bytes do not equal the accepted binding" :north.store-runtime-manifest/manifest-bytes-mismatch {}) (if (= actual expected) (->StoreRuntimeAttestation checked actual) (fail "Store runtime manifest facts do not equal the expected immutable binding" :north.store-runtime-manifest/binding-mismatch {:expected expected :actual actual}))))))
    (instance? Native match__10) (let [_ (:release-root match__10) _ (:beagle-revision match__10) _ (:beagle-tree match__10) _ (:artifact-root match__10) _ (:closure-sha256 match__10) _ (:server-artifact match__10) _ (:server-sha256 match__10)] (fail "Store runtime generation has no JVM member" :north.store-runtime-manifest/missing-jvm {}))))))

(defn ^StoreRuntimeGeneration promote-transition! [^StoreRuntimeGeneration selected candidate]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! selected)
   promoted (validate-jvm-member-for-purpose! candidate :promotion-candidate)]
  (let [match__11 promoted]
  (cond
    (instance? JVM match__11) (let [_ (:output match__11) _ (:package-nar-sha256 match__11) _ (:beagle-revision match__11) _ (:beagle-tree match__11) _ (:manifest-path match__11) _ (:manifest-bytes match__11) _ (:manifest-sha256 match__11) _ (:manifest match__11)] (if (= promoted (:current checked)) checked (validate-runtime-generation! (->StoreRuntimeGeneration promoted (native-member! checked)))))
    (instance? Native match__11) (let [_ (:release-root match__11) _ (:beagle-revision match__11) _ (:beagle-tree match__11) _ (:artifact-root match__11) _ (:closure-sha256 match__11) _ (:server-artifact match__11) _ (:server-sha256 match__11)] (fail "Only an accepted JVM member can be promoted" :north.store-runtime-manifest/invalid-promotion {}))))))

(defn ^StoreRuntimeGeneration promote-authority-transition! [^StoreRuntimeGeneration selected candidate]
  (if (promotion-source-generation? selected) (let [^StoreRuntimeGeneration checked (validate-promotion-source-generation! selected)
   promoted (validate-jvm-member-for-purpose! candidate :promotion-candidate)
   recovery (let [match__12 (:current checked)]
  (cond
    (instance? JVM match__12) (let [_ (:output match__12) _ (:package-nar-sha256 match__12) _ (:beagle-revision match__12) _ (:beagle-tree match__12) _ (:manifest-path match__12) _ (:manifest-bytes match__12) _ (:manifest-sha256 match__12) _ (:manifest match__12)] (:previous checked))
    (instance? Native match__12) (let [_ (:release-root match__12) _ (:beagle-revision match__12) _ (:beagle-tree match__12) _ (:artifact-root match__12) _ (:closure-sha256 match__12) _ (:server-artifact match__12) _ (:server-sha256 match__12)] (:current checked))))]
  (let [match__13 promoted]
  (cond
    (instance? JVM match__13) (let [_ (:output match__13) _ (:package-nar-sha256 match__13) _ (:beagle-revision match__13) _ (:beagle-tree match__13) _ (:manifest-path match__13) _ (:manifest-bytes match__13) _ (:manifest-sha256 match__13) _ (:manifest match__13)] (validate-runtime-generation! (->StoreRuntimeGeneration promoted recovery)))
    (instance? Native match__13) (let [_ (:release-root match__13) _ (:beagle-revision match__13) _ (:beagle-tree match__13) _ (:artifact-root match__13) _ (:closure-sha256 match__13) _ (:server-artifact match__13) _ (:server-sha256 match__13)] (fail "Only an accepted JVM member can advance runtime authority" :north.store-runtime-manifest/invalid-promotion {}))))) (promote-transition! selected candidate)))

(defn ^StoreRuntimeGeneration initial-promotion-transition! [candidate]
  (let [promoted (validate-jvm-member-for-purpose! candidate :promotion-candidate)]
  (let [match__14 promoted]
  (cond
    (instance? JVM match__14) (let [_ (:output match__14) _ (:package-nar-sha256 match__14) _ (:beagle-revision match__14) _ (:beagle-tree match__14) _ (:manifest-path match__14) _ (:manifest-bytes match__14) _ (:manifest-sha256 match__14) _ (:manifest match__14)] (validate-runtime-generation! (->StoreRuntimeGeneration promoted accepted-native-runtime)))
    (instance? Native match__14) (let [_ (:release-root match__14) _ (:beagle-revision match__14) _ (:beagle-tree match__14) _ (:artifact-root match__14) _ (:closure-sha256 match__14) _ (:server-artifact match__14) _ (:server-sha256 match__14)] (fail "Only an accepted JVM member can initialize promotion" :north.store-runtime-manifest/invalid-promotion {}))))))

(defn ^StoreRuntimeGeneration rollback-transition! [^StoreRuntimeGeneration selected]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! selected)]
  (validate-runtime-generation! (->StoreRuntimeGeneration (:previous checked) (:current checked)))))

(defn ^StoreRuntimeGeneration restore-transition! [^StoreRuntimeGeneration selected]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! selected)]
  (let [match__15 (:current checked)]
  (cond
    (instance? JVM match__15) (let [_ (:output match__15) _ (:package-nar-sha256 match__15) _ (:beagle-revision match__15) _ (:beagle-tree match__15) _ (:manifest-path match__15) _ (:manifest-bytes match__15) _ (:manifest-sha256 match__15) _ (:manifest match__15)] checked)
    (instance? Native match__15) (let [_ (:release-root match__15) _ (:beagle-revision match__15) _ (:beagle-tree match__15) _ (:artifact-root match__15) _ (:closure-sha256 match__15) _ (:server-artifact match__15) _ (:server-sha256 match__15)] (validate-runtime-generation! (->StoreRuntimeGeneration (jvm-member! checked) (native-member! checked))))))))

(defn runtime-member-status-lines [^String prefix member]
  (let [match__16 member]
  (cond
    (instance? JVM match__16) (let [output (:output match__16) package-nar-sha256 (:package-nar-sha256 match__16) beagle-revision (:beagle-revision match__16) beagle-tree (:beagle-tree match__16) manifest-path (:manifest-path match__16) manifest-bytes (:manifest-bytes match__16) manifest-sha256 (:manifest-sha256 match__16) _ (:manifest match__16)] [(str prefix ".kind=jvm") (str prefix ".output=" output) (str prefix ".nar=" package-nar-sha256) (str prefix ".revision=" beagle-revision) (str prefix ".tree=" beagle-tree) (str prefix ".manifest_path=" manifest-path) (str prefix ".manifest_bytes=" manifest-bytes) (str prefix ".manifest_sha256=" manifest-sha256)])
    (instance? Native match__16) (let [release-root (:release-root match__16) beagle-revision (:beagle-revision match__16) beagle-tree (:beagle-tree match__16) artifact-root (:artifact-root match__16) closure-sha256 (:closure-sha256 match__16) server-artifact (:server-artifact match__16) server-sha256 (:server-sha256 match__16)] [(str prefix ".kind=native") (str prefix ".release_root=" release-root) (str prefix ".revision=" beagle-revision) (str prefix ".tree=" beagle-tree) (str prefix ".artifact_root=" artifact-root) (str prefix ".closure_sha256=" closure-sha256) (str prefix ".server=" server-artifact) (str prefix ".server_sha256=" server-sha256)]))))

(defn generation-status-lines! [^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! generation)]
  (vec (concat (runtime-member-status-lines "current" (:current checked)) (runtime-member-status-lines "previous" (:previous checked))))))
