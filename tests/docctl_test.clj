(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[docctl :as docctl])

(defn check [label value]
  (when-not value
    (throw (ex-info (str "FAIL: " label) {})))
  (println "PASS" label))

(let [root (.toFile (java.nio.file.Files/createTempDirectory "docctl-test" (make-array java.nio.file.attribute.FileAttribute 0)))
      source (io/file root "docs/source.md")
      derived (io/file root "README.md")
      manifest (io/file root "README.md.doc.edn")
      state (io/file root "state")]
  (.mkdirs (.getParentFile source))
  (spit source "source v1\n")
  (spit derived "distilled\n")
  (spit manifest (pr-str {:kind :distilled
                           :sources [{:path "docs/source.md" :revision "content"}]
                           :refresh-policy :on-change
                           :source-digests {"docs/source.md" (docctl/sha256-file source)}}))
  (let [initial (docctl/scan-root (.getPath root))
        record (first (:manifests initial))]
    (check "initial manifest is current" (= :current (:status record)))
    (spit source "source v2\n")
    (let [stale (first (:manifests (docctl/scan-root (.getPath root))))]
      (check "changed source marks document stale" (= :stale (:status stale)))
      (let [result (docctl/invalidate! (.getPath root) (.getPath state))
            queued (first (:queued result))
            current (first (:manifests (docctl/scan-root (.getPath root))))]
        (check "invalidate queues the derived document" (= "README.md" (:document queued)))
        (check "queue records changed source" (= :changed (:reason (first (:mismatches current)))))
        (spit source "source v1\n")
        (check "invalidate clears a resolved document"
               (empty? (:queued (docctl/invalidate! (.getPath root) (.getPath state))))))))
  (println "docctl: all checks passed"))
