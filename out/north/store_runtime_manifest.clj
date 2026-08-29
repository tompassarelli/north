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

(defrecord StoreRuntimeBinding [package-root manifest-path expected])

(defn storeruntimebinding-package-root [r] (:package-root r))

(defn storeruntimebinding-manifest-path [r] (:manifest-path r))

(defn storeruntimebinding-expected [r] (:expected r))

(defrecord StoreRuntimeAttestation [package-root manifest-path manifest])

(defn storeruntimeattestation-package-root [r] (:package-root r))

(defn storeruntimeattestation-manifest-path [r] (:manifest-path r))

(defn storeruntimeattestation-manifest [r] (:manifest r))

(defn- fail [^String message code data]
  (throw (ex-info message (assoc data :type code))))

(defn- ^Boolean exact-lower-oid? [^String value]
  (some? (re-matches #"[0-9a-f]{40}" value)))

(defn ^Boolean canonical-package-root? [^String value]
  (some? (re-matches #"/nix/store/[0-9abcdfghijklmnpqrsvwxyz]{32}-[A-Za-z0-9][A-Za-z0-9+._?=-]*" value)))

(defn ^String manifest-path-for [^String package-root]
  (str package-root "/" manifest-relative-path))

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

(defn- ^String line-value [^String line ^String field]
  (let [^String prefix (str field "=")]
  (if (str/starts-with? line prefix) (subs line (count prefix)) (fail (str "Store runtime manifest expected ordered field " field) :north.store-runtime-manifest/noncanonical-fields {:field field :line line}))))

(defn ^StoreRuntimeManifest parse-runtime-manifest! [^String text]
  (if (> (count text) max-manifest-characters) (fail "Store runtime manifest exceeds the bounded input limit" :north.store-runtime-manifest/input-too-large {:maximum max-manifest-characters :actual (count text)}) (if (str/includes? text "\r") (fail "Store runtime manifest must contain LF line endings and no CR characters" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [lines (vec (str/split text #"\n" -1))]
  (if (not (= (count lines) 12)) (fail "Store runtime manifest must contain exactly eleven ordered LF-terminated fields" :north.store-runtime-manifest/noncanonical-fields {:line-count (count lines)}) (if (not (= (nth lines 11) "")) (fail "Store runtime manifest must end with one LF" :north.store-runtime-manifest/noncanonical-line-endings {}) (let [^String heap-text (line-value (nth lines 6) "heap_max_bytes")
   ^StoreRuntimeManifest facts (->StoreRuntimeManifest (line-value (nth lines 0) "format") (line-value (nth lines 1) "beagle_revision") (line-value (nth lines 2) "source_tree") (line-value (nth lines 3) "engine") (line-value (nth lines 4) "native_backend") (line-value (nth lines 5) "heap_policy") (if (= heap-text "2147483648") manifest-heap-max-bytes (fail "Store runtime manifest heap_max_bytes is not canonical" :north.store-runtime-manifest/noncanonical-integer {:actual heap-text})) (line-value (nth lines 7) "protocol") (line-value (nth lines 8) "protocol_version") (line-value (nth lines 9) "readiness") (line-value (nth lines 10) "stopping"))]
  (validate-manifest-facts! facts))))))))

(defn ^StoreRuntimeAttestation attest-runtime-manifest! [^String text ^StoreRuntimeBinding binding]
  (let [^String package-root (:package-root binding)
   ^String manifest-path (:manifest-path binding)
   ^StoreRuntimeManifest expected (validate-manifest-facts! (:expected binding))]
  (if (not (canonical-package-root? package-root)) (fail "Store runtime binding package root is not a canonical immutable /nix/store package root" :north.store-runtime-manifest/invalid-package-root {:actual package-root}) (let [^String derived-path (manifest-path-for package-root)]
  (if (not (= manifest-path derived-path)) (fail "Store runtime binding manifest path does not derive from its package root" :north.store-runtime-manifest/manifest-path-substitution {:expected derived-path :actual manifest-path}) (let [^StoreRuntimeManifest actual (parse-runtime-manifest! text)]
  (if (= actual expected) (->StoreRuntimeAttestation package-root manifest-path actual) (fail "Store runtime manifest facts do not equal the expected immutable binding" :north.store-runtime-manifest/binding-mismatch {:expected expected :actual actual}))))))))
