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

(defrecord StoreRuntimeBinding [beagle-tree package-root package-nar-sha256 manifest-path manifest-bytes manifest-sha256 receipt-path receipt-sha256 expected])

(defn storeruntimebinding-beagle-tree [r] (:beagle-tree r))

(defn storeruntimebinding-package-root [r] (:package-root r))

(defn storeruntimebinding-package-nar-sha256 [r] (:package-nar-sha256 r))

(defn storeruntimebinding-manifest-path [r] (:manifest-path r))

(defn storeruntimebinding-manifest-bytes [r] (:manifest-bytes r))

(defn storeruntimebinding-manifest-sha256 [r] (:manifest-sha256 r))

(defn storeruntimebinding-receipt-path [r] (:receipt-path r))

(defn storeruntimebinding-receipt-sha256 [r] (:receipt-sha256 r))

(defn storeruntimebinding-expected [r] (:expected r))

(defrecord StorePreviousRuntimeIdentity [release-root beagle-revision beagle-tree native-artifact-root native-closure-sha256 server-artifact server-artifact-sha256])

(defn storepreviousruntimeidentity-release-root [r] (:release-root r))

(defn storepreviousruntimeidentity-beagle-revision [r] (:beagle-revision r))

(defn storepreviousruntimeidentity-beagle-tree [r] (:beagle-tree r))

(defn storepreviousruntimeidentity-native-artifact-root [r] (:native-artifact-root r))

(defn storepreviousruntimeidentity-native-closure-sha256 [r] (:native-closure-sha256 r))

(defn storepreviousruntimeidentity-server-artifact [r] (:server-artifact r))

(defn storepreviousruntimeidentity-server-artifact-sha256 [r] (:server-artifact-sha256 r))

(defrecord StoreRuntimeGeneration [current previous])

(defn storeruntimegeneration-current [r] (:current r))

(defn storeruntimegeneration-previous [r] (:previous r))

(defrecord StoreRuntimeAttestation [generation manifest])

(defn storeruntimeattestation-generation [r] (:generation r))

(defn storeruntimeattestation-manifest [r] (:manifest r))

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

(defn ^String manifest-path-for [^String package-root]
  (str package-root "/" manifest-relative-path))

(defn ^String receipt-path-for [^String receipt-sha256]
  (str store-artifacts-root "/" receipt-sha256 ".json"))

(defn ^String release-path-for [^String beagle-revision]
  (str store-releases-root "/" beagle-revision))

(defn ^String native-artifact-path-for [^String closure-sha256]
  (str store-artifacts-root "/" closure-sha256))

(defn ^String native-server-path-for [^String artifact-root]
  (str artifact-root "/bin/beagle-store-server-native"))

(defn- ^String require-text! [^String field ^String actual ^String expected]
  (if (= actual expected) actual (fail (str "Store runtime manifest " field " mismatch") :north.store-runtime-manifest/field-mismatch {:field field :expected expected :actual actual})))

(defn- ^StoreRuntimeManifest validate-manifest-facts! [^StoreRuntimeManifest facts]
  (do
  (require-text! "format" (:format facts) manifest-format)
  (if (exact-lower-oid? (:beagle-revision facts)) (:beagle-revision facts) (fail "Store runtime manifest beagle_revision must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-beagle-revision {:actual (:beagle-revision facts)}))
  (if (exact-lower-oid? (:source-tree facts)) (:source-tree facts) (fail "Store runtime manifest source_tree must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-source-tree {:actual (:source-tree facts)}))
  (require-text! "engine" (:engine facts) manifest-engine)
  (require-text! "native_backend" (:native-backend facts) manifest-native-backend)
  (require-text! "heap_policy" (:heap-policy facts) manifest-heap-policy)
  (if (= (:heap-max-bytes facts) manifest-heap-max-bytes) (:heap-max-bytes facts) (fail "Store runtime manifest heap_max_bytes mismatch" :north.store-runtime-manifest/field-mismatch {:field "heap_max_bytes" :expected manifest-heap-max-bytes :actual (:heap-max-bytes facts)}))
  (require-text! "protocol" (:protocol facts) manifest-protocol)
  (require-text! "protocol_version" (:protocol-version facts) manifest-protocol-version)
  (require-text! "readiness" (:readiness facts) manifest-readiness)
  (require-text! "stopping" (:stopping facts) manifest-stopping)
  facts))

(defn ^String canonical-manifest-text! [^StoreRuntimeManifest facts]
  (let [^StoreRuntimeManifest checked (validate-manifest-facts! facts)]
  (str "format=" (:format checked) "\n" "beagle_revision=" (:beagle-revision checked) "\n" "source_tree=" (:source-tree checked) "\n" "engine=" (:engine checked) "\n" "native_backend=" (:native-backend checked) "\n" "heap_policy=" (:heap-policy checked) "\n" "heap_max_bytes=" (:heap-max-bytes checked) "\n" "protocol=" (:protocol checked) "\n" "protocol_version=" (:protocol-version checked) "\n" "readiness=" (:readiness checked) "\n" "stopping=" (:stopping checked) "\n")))

(def ^StoreRuntimeManifest accepted-current-runtime-manifest (->StoreRuntimeManifest manifest-format "11db5dc955c75cbc28baa9c42490e40b554c143e" "170aab59fdce52869f0e1feda6b29e891524c242" manifest-engine manifest-native-backend manifest-heap-policy manifest-heap-max-bytes manifest-protocol manifest-protocol-version manifest-readiness manifest-stopping))

(def ^StoreRuntimeBinding accepted-current-runtime (->StoreRuntimeBinding "eea49f32085c266e5f396fbe2d3b64bcb574af33" "/nix/store/kglv2v4fcrrdnslx9qsfq46iyy0psdi9-beagle-store-0-unstable-2026-08-29-11db5dc" "sha256-qBekOj7929oDuXh682GtKBEpvMTRDh+SexvHoQqQJHI=" "/nix/store/kglv2v4fcrrdnslx9qsfq46iyy0psdi9-beagle-store-0-unstable-2026-08-29-11db5dc/libexec/store/runtime.manifest" 349 "39f7e3617bc3f34426e22ee092f4da3e4d1f2da9219028fa41ace6a818921f17" (receipt-path-for "8d1d337eb28001af6315b328890ab5fe31dc06dec7efa467ae3e0b76fce9c267") "8d1d337eb28001af6315b328890ab5fe31dc06dec7efa467ae3e0b76fce9c267" accepted-current-runtime-manifest))

(def ^StorePreviousRuntimeIdentity accepted-previous-runtime (->StorePreviousRuntimeIdentity (release-path-for "48f38823e42694578587f5624d8be5db9f962a77") "48f38823e42694578587f5624d8be5db9f962a77" "7d4dd724e1ba4c107162a24d47aea0849be119a5" (native-artifact-path-for "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554") "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554" (native-server-path-for (native-artifact-path-for "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554")) "b3de9e5692ba73303da4f2e38432e6fe0debacd4cf46ac3033d059f713225b69"))

(def ^StoreRuntimeGeneration accepted-runtime-generation (->StoreRuntimeGeneration accepted-current-runtime accepted-previous-runtime))

(def ^String accepted-runtime-manifest-text (canonical-manifest-text! accepted-current-runtime-manifest))

(defn- ^String line-value [^String line ^String field]
  (let [^String prefix (str field "=")]
  (if (str/starts-with? line prefix) (subs line (count prefix)) (fail (str "Store runtime manifest expected ordered field " field) :north.store-runtime-manifest/noncanonical-fields {:field field :line line}))))

(defn ^StoreRuntimeManifest parse-runtime-manifest! [^String text]
  (if (> (count text) max-manifest-characters) (fail "Store runtime manifest exceeds the bounded input limit" :north.store-runtime-manifest/input-too-large {:maximum max-manifest-characters :actual (count text)}) (if (str/includes? text "\r") (fail "Store runtime manifest must contain LF line endings and no CR characters" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [lines (vec (str/split text #"\n" -1))]
  (if (not (= (count lines) 12)) (fail "Store runtime manifest must contain exactly eleven ordered LF-terminated fields" :north.store-runtime-manifest/noncanonical-fields {:line-count (count lines)}) (if (not (= (nth lines 11) "")) (fail "Store runtime manifest must end with one LF" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [^String heap-text (line-value (nth lines 6) "heap_max_bytes")
   ^StoreRuntimeManifest facts (->StoreRuntimeManifest (line-value (nth lines 0) "format") (line-value (nth lines 1) "beagle_revision") (line-value (nth lines 2) "source_tree") (line-value (nth lines 3) "engine") (line-value (nth lines 4) "native_backend") (line-value (nth lines 5) "heap_policy") (if (= heap-text "2147483648") manifest-heap-max-bytes (fail "Store runtime manifest heap_max_bytes is not canonical" :north.store-runtime-manifest/noncanonical-integer {:actual heap-text})) (line-value (nth lines 7) "protocol") (line-value (nth lines 8) "protocol_version") (line-value (nth lines 9) "readiness") (line-value (nth lines 10) "stopping"))]
  (validate-manifest-facts! facts))))))))

(defn- ^StoreRuntimeBinding validate-current-runtime! [^StoreRuntimeBinding binding]
  (do
  (if (exact-lower-oid? (:beagle-tree binding)) (:beagle-tree binding) (fail "Store runtime binding beagle tree must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-beagle-tree {:actual (:beagle-tree binding)}))
  (if (canonical-package-root? (:package-root binding)) (:package-root binding) (fail "Store runtime binding package root is not a canonical immutable /nix/store package root" :north.store-runtime-manifest/invalid-package-root {:actual (:package-root binding)}))
  (if (exact-nar-sha256? (:package-nar-sha256 binding)) (:package-nar-sha256 binding) (fail "Store runtime binding package NAR hash is not a canonical SHA-256 SRI" :north.store-runtime-manifest/invalid-package-nar {:actual (:package-nar-sha256 binding)}))
  (require-text! "manifest_path" (:manifest-path binding) (manifest-path-for (:package-root binding)))
  (if (> (:manifest-bytes binding) 0) (:manifest-bytes binding) (fail "Store runtime binding manifest byte count must be positive" :north.store-runtime-manifest/invalid-manifest-size {:actual (:manifest-bytes binding)}))
  (if (exact-sha256? (:manifest-sha256 binding)) (:manifest-sha256 binding) (fail "Store runtime binding manifest SHA-256 is not canonical" :north.store-runtime-manifest/invalid-manifest-sha256 {:actual (:manifest-sha256 binding)}))
  (if (exact-sha256? (:receipt-sha256 binding)) (:receipt-sha256 binding) (fail "Store runtime binding receipt SHA-256 is not canonical" :north.store-runtime-manifest/invalid-receipt-sha256 {:actual (:receipt-sha256 binding)}))
  (require-text! "receipt_path" (:receipt-path binding) (receipt-path-for (:receipt-sha256 binding)))
  (validate-manifest-facts! (:expected binding))
  binding))

(defn- ^StorePreviousRuntimeIdentity validate-previous-runtime! [^StorePreviousRuntimeIdentity identity]
  (do
  (if (exact-lower-oid? (:beagle-revision identity)) (:beagle-revision identity) (fail "Previous Store runtime revision must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-previous-revision {:actual (:beagle-revision identity)}))
  (if (exact-lower-oid? (:beagle-tree identity)) (:beagle-tree identity) (fail "Previous Store runtime tree must be exactly 40 lowercase hexadecimal characters" :north.store-runtime-manifest/invalid-previous-tree {:actual (:beagle-tree identity)}))
  (require-text! "previous_release_root" (:release-root identity) (release-path-for (:beagle-revision identity)))
  (if (exact-sha256? (:native-closure-sha256 identity)) (:native-closure-sha256 identity) (fail "Previous Store runtime Native closure SHA-256 is not canonical" :north.store-runtime-manifest/invalid-previous-closure {:actual (:native-closure-sha256 identity)}))
  (require-text! "previous_native_artifact_root" (:native-artifact-root identity) (native-artifact-path-for (:native-closure-sha256 identity)))
  (require-text! "previous_server_artifact" (:server-artifact identity) (native-server-path-for (:native-artifact-root identity)))
  (if (exact-sha256? (:server-artifact-sha256 identity)) (:server-artifact-sha256 identity) (fail "Previous Store runtime server SHA-256 is not canonical" :north.store-runtime-manifest/invalid-previous-server {:actual (:server-artifact-sha256 identity)}))
  identity))

(defn- ^StoreRuntimeGeneration validate-runtime-generation! [^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeBinding current (validate-current-runtime! (:current generation))
   ^StorePreviousRuntimeIdentity previous (validate-previous-runtime! (:previous generation))]
  (if (= (->StoreRuntimeGeneration current previous) accepted-runtime-generation) generation (fail "Store project runtime generation does not equal the accepted current and previous identities" :north.store-runtime-manifest/generation-mismatch {:expected accepted-runtime-generation :actual generation}))))

(defn ^StoreRuntimeAttestation attest-runtime-manifest! [^String text ^StoreRuntimeGeneration generation]
  (let [^StoreRuntimeGeneration checked (validate-runtime-generation! generation)
   ^StoreRuntimeBinding current (:current checked)
   ^StoreRuntimeManifest expected (:expected current)]
  (if (not (= (count text) (:manifest-bytes current))) (fail "Store runtime manifest byte count does not equal the accepted binding" :north.store-runtime-manifest/manifest-size-mismatch {:expected (:manifest-bytes current) :actual (count text)}) (if (not (= text accepted-runtime-manifest-text)) (fail "Store runtime manifest bytes do not equal the accepted binding" :north.store-runtime-manifest/manifest-bytes-mismatch {}) (let [^StoreRuntimeManifest actual (parse-runtime-manifest! text)]
  (if (= actual expected) (->StoreRuntimeAttestation checked actual) (fail "Store runtime manifest facts do not equal the expected immutable binding" :north.store-runtime-manifest/binding-mismatch {:expected expected :actual actual})))))))
