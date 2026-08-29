(require '[clojure.string :as str]
         '[north.store-runtime-manifest :as manifest])

(def revision (apply str (repeat 40 "a")))
(def source-tree (apply str (repeat 40 "b")))
(def package-root "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-store-runtime")

(def facts
  (manifest/->StoreRuntimeManifest
   manifest/manifest-format revision source-tree manifest/manifest-engine
   manifest/manifest-native-backend manifest/manifest-heap-policy
   manifest/manifest-heap-max-bytes manifest/manifest-protocol
   manifest/manifest-protocol-version manifest/manifest-readiness
   manifest/manifest-stopping))

(def text (manifest/canonical-manifest-text! facts))
(def manifest-path (manifest/manifest-path-for package-root))
(def binding (manifest/->StoreRuntimeBinding package-root manifest-path facts))

(defn throws? [f]
  (try (f) false (catch Exception _ true)))

(defn replace-once [source old new]
  (str/replace-first source old new))

(def success (manifest/attest-runtime-manifest! text binding))

(def malformed-texts
  [(str/replace-first text "format=" "unknown=")
   (str "format=" manifest/manifest-format "\n" text)
   (str/replace-first text (str "format=" manifest/manifest-format "\n") "")
   (str/replace-first text
                      (str "format=" manifest/manifest-format "\nbeagle_revision=" revision "\n")
                      (str "beagle_revision=" revision "\nformat=" manifest/manifest-format "\n"))
   (str/replace text "\n" "\r\n")
   (subs text 0 (dec (count text)))
   (str text "\n")
   (replace-once text revision (str "A" (subs revision 1)))
   (replace-once text source-tree (subs source-tree 1))
   (replace-once text "heap_max_bytes=2147483648" "heap_max_bytes=02147483648")
   (replace-once text "engine=jvm-clojure" "engine=native")
   (replace-once text "native_backend=experimental-non-production" "native_backend=production")
   (replace-once text "protocol=store-rpc" "protocol=other-rpc")
   (str text (apply str (repeat 4096 "x")))])

(def mismatched-facts
  [(assoc facts :beagle-revision (apply str (repeat 40 "c")))
   (assoc facts :source-tree (apply str (repeat 40 "d")))
   (assoc facts :engine "native")
   (assoc facts :native-backend "production")
   (assoc facts :heap-policy "dynamic")
   (assoc facts :heap-max-bytes 1)
   (assoc facts :protocol "other-rpc")
   (assoc facts :protocol-version "3.0")
   (assoc facts :readiness "listen")
   (assoc facts :stopping "after-drain")])

(def checks
  [["canonical parser returns all structured producer facts"
    (= facts (manifest/parse-runtime-manifest! text))]
   ["successful equality returns immutable package root"
    (= package-root (:package-root success))]
   ["successful equality returns the derived manifest path"
    (= manifest-path (:manifest-path success))]
   ["successful equality retains parsed facts directly"
    (= facts (:manifest success))]
   ["manifest path is derived under the package"
    (= (str package-root "/libexec/store/runtime.manifest") manifest-path)]
   ["canonical Nix package root is accepted"
    (manifest/canonical-package-root? package-root)]
   ["mutable package path is rejected"
    (not (manifest/canonical-package-root? "/run/current-system/sw"))]
   ["nested Nix path is rejected as a package root"
    (not (manifest/canonical-package-root? (str package-root "/bin/store")))]
   ["malformed duplicate unknown missing reordered and noncanonical text all fail closed"
    (every? true? (mapv (fn [candidate] (throws? #(manifest/parse-runtime-manifest! candidate)))
                        malformed-texts))]
   ["every expected identity and capability mismatch fails closed"
    (every? true?
            (mapv (fn [expected]
                    (throws? #(manifest/attest-runtime-manifest!
                               text
                               (manifest/->StoreRuntimeBinding
                                package-root manifest-path expected))))
                  mismatched-facts))]
   ["package substitution fails closed"
    (throws? #(manifest/attest-runtime-manifest!
               text
               (manifest/->StoreRuntimeBinding
                "/nix/store/11111111111111111111111111111111-other"
                manifest-path facts)))]
   ["manifest path substitution fails closed"
    (throws? #(manifest/attest-runtime-manifest!
               text
               (manifest/->StoreRuntimeBinding
                package-root (str package-root "/runtime.manifest") facts)))]] )

(let [fails (remove second checks)]
  (doseq [[name ok] checks]
    (println (if ok "  [PASS] " "  [FAIL] ") name))
  (if (empty? fails)
    (println "\nstore runtime manifest:" (count checks) "/" (count checks) "PASS")
    (do
      (println "\nstore runtime manifest:" (count fails) "FAILED")
      (System/exit 1))))
