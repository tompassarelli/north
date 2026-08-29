(require '[clojure.string :as str]
         '[north.store-runtime-manifest :as manifest])

(def generation manifest/accepted-runtime-generation)
(def current (:current generation))
(def previous (:previous generation))
(def facts (:expected current))
(def text manifest/accepted-runtime-manifest-text)
(def package-root (:package-root current))
(def manifest-path (:manifest-path current))

(defn repeated [value length]
  (apply str (repeat length value)))

(defn throws? [f]
  (try (f) false (catch Exception _ true)))

(defn replace-once [source old new]
  (str/replace-first source old new))

(def success (manifest/attest-runtime-manifest! text generation))

(def malformed-texts
  [(str/replace-first text "format=" "unknown=")
   (str "format=" manifest/manifest-format "\n" text)
   (str/replace-first text (str "format=" manifest/manifest-format "\n") "")
   (str/replace-first text
                      (str "format=" manifest/manifest-format
                           "\nbeagle_revision=" (:beagle-revision facts) "\n")
                      (str "beagle_revision=" (:beagle-revision facts)
                           "\nformat=" manifest/manifest-format "\n"))
   (str/replace text "\n" "\r\n")
   (subs text 0 (dec (count text)))
   (str text "\n")
   (replace-once text (:beagle-revision facts)
                 (str "A" (subs (:beagle-revision facts) 1)))
   (replace-once text (:source-tree facts) (subs (:source-tree facts) 1))
   (replace-once text "heap_max_bytes=2147483648" "heap_max_bytes=02147483648")
   (replace-once text "engine=jvm-clojure" "engine=native")
   (replace-once text "native_backend=experimental-non-production" "native_backend=production")
   (replace-once text "protocol=store-rpc" "protocol=other-rpc")
   (str text (repeated "x" 4096))])

(def mismatched-facts
  [(assoc facts :beagle-revision (repeated "c" 40))
   (assoc facts :source-tree (repeated "d" 40))
   (assoc facts :engine "native")
   (assoc facts :native-backend "production")
   (assoc facts :heap-policy "dynamic")
   (assoc facts :heap-max-bytes 1)
   (assoc facts :protocol "other-rpc")
   (assoc facts :protocol-version "3.0")
   (assoc facts :readiness "listen")
   (assoc facts :stopping "after-drain")])

(def other-package "/nix/store/11111111111111111111111111111111-other")
(def other-sha (repeated "c" 64))
(def other-oid (repeated "c" 40))
(def other-nar (str "sha256-" (repeated "A" 43) "="))

(def current-substitutions
  [(assoc current :beagle-tree other-oid)
   (assoc current :package-root other-package
                  :manifest-path (manifest/manifest-path-for other-package))
   (assoc current :package-nar-sha256 other-nar)
   (assoc current :manifest-bytes 350)
   (assoc current :manifest-sha256 other-sha)
   (assoc current :receipt-path (manifest/receipt-path-for other-sha)
                  :receipt-sha256 other-sha)])

(def other-previous-root (manifest/release-path-for other-oid))
(def other-native-root (manifest/native-artifact-path-for other-sha))
(def previous-substitutions
  [(assoc previous :beagle-revision other-oid
                   :release-root other-previous-root)
   (assoc previous :beagle-tree other-oid)
   (assoc previous :native-closure-sha256 other-sha
                   :native-artifact-root other-native-root
                   :server-artifact (manifest/native-server-path-for other-native-root))
   (assoc previous :server-artifact-sha256 other-sha)])

(def checks
  [["accepted manifest is the exact 349-byte canonical producer text"
    (and (= 349 (count text))
         (= text (manifest/canonical-manifest-text! facts)))]
   ["canonical parser returns all exact current producer facts"
    (= facts (manifest/parse-runtime-manifest! text))]
   ["successful equality retains the exact Store current and previous generation"
    (= generation (:generation success))]
   ["successful equality retains parsed manifest facts directly"
    (= facts (:manifest success))]
   ["current binds the accepted immutable package and package NAR"
    (and (= "/nix/store/kglv2v4fcrrdnslx9qsfq46iyy0psdi9-beagle-store-0-unstable-2026-08-29-11db5dc"
            package-root)
         (= "sha256-qBekOj7929oDuXh682GtKBEpvMTRDh+SexvHoQqQJHI="
            (:package-nar-sha256 current)))]
   ["current binds exact Git tree source identity manifest and receipt digests"
    (and (= "eea49f32085c266e5f396fbe2d3b64bcb574af33" (:beagle-tree current))
         (= "170aab59fdce52869f0e1feda6b29e891524c242" (:source-tree facts))
         (= "39f7e3617bc3f34426e22ee092f4da3e4d1f2da9219028fa41ace6a818921f17"
            (:manifest-sha256 current))
         (= "8d1d337eb28001af6315b328890ab5fe31dc06dec7efa467ae3e0b76fce9c267"
            (:receipt-sha256 current)))]
   ["current manifest and receipt paths derive from their immutable identities"
    (and (= manifest-path (manifest/manifest-path-for package-root))
         (= (:receipt-path current)
            (manifest/receipt-path-for (:receipt-sha256 current))))]
   ["previous is the exact retained legacy Store rollback identity"
    (and (= "48f38823e42694578587f5624d8be5db9f962a77"
            (:beagle-revision previous))
         (= "7d4dd724e1ba4c107162a24d47aea0849be119a5"
            (:beagle-tree previous))
         (= "ec53c8a717424bec0f6d8212401632e3da0860f80abc6ad062500f68ea0ab554"
            (:native-closure-sha256 previous))
         (= "b3de9e5692ba73303da4f2e38432e6fe0debacd4cf46ac3033d059f713225b69"
            (:server-artifact-sha256 previous)))]
   ["previous release artifact and server paths derive from retained identities"
    (and (= (:release-root previous)
            (manifest/release-path-for (:beagle-revision previous)))
         (= (:native-artifact-root previous)
            (manifest/native-artifact-path-for (:native-closure-sha256 previous)))
         (= (:server-artifact previous)
            (manifest/native-server-path-for (:native-artifact-root previous))))]
   ["canonical Nix package root is accepted"
    (manifest/canonical-package-root? package-root)]
   ["mutable package path is rejected"
    (not (manifest/canonical-package-root? "/run/current-system/sw"))]
   ["nested Nix path is rejected as a package root"
    (not (manifest/canonical-package-root? (str package-root "/bin/store")))]
   ["malformed duplicate unknown missing reordered and noncanonical text all fail closed"
    (every? true? (mapv (fn [candidate]
                          (throws? #(manifest/parse-runtime-manifest! candidate)))
                        malformed-texts))]
   ["every expected identity and capability mismatch fails closed"
    (every? true?
            (mapv (fn [expected]
                    (throws? #(manifest/attest-runtime-manifest!
                               text
                               (assoc generation :current
                                      (assoc current :expected expected)))))
                  mismatched-facts))]
   ["every exact current artifact substitution fails closed"
    (every? true?
            (mapv (fn [candidate]
                    (throws? #(manifest/attest-runtime-manifest!
                               text (assoc generation :current candidate))))
                  current-substitutions))]
   ["every exact previous rollback artifact substitution fails closed"
    (every? true?
            (mapv (fn [candidate]
                    (throws? #(manifest/attest-runtime-manifest!
                               text (assoc generation :previous candidate))))
                  previous-substitutions))]])

(let [fails (remove second checks)]
  (doseq [[name ok] checks]
    (println (if ok "  [PASS] " "  [FAIL] ") name))
  (if (empty? fails)
    (println "\nstore runtime manifest:" (count checks) "/" (count checks) "PASS")
    (do
      (println "\nstore runtime manifest:" (count fails) "FAILED")
      (System/exit 1))))
