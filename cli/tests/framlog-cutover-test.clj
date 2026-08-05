#!/usr/bin/env bb
;; Service-free contract checks for the exact forward-only Fram cutover.
(require '[clojure.string :as str])

(import '[java.nio ByteBuffer ByteOrder]
        '[java.nio.charset StandardCharsets]
        '[java.nio.file Files]
        '[java.nio.file.attribute FileAttribute])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root (-> test-script java.io.File. .getCanonicalFile
              .getParentFile .getParentFile .getParent str))
(def source-path (str root "/scripts/framlog-cutover.clj"))
(def source-text (slurp source-path))
(def main-marker "\n(try\n  (if")
(def main-offset (str/last-index-of source-text main-marker))
(when-not main-offset
  (throw (ex-info "framlog cutover main form marker not found" {})))

(System/setProperty "babashka.file" source-path)
(try
  (load-string (subs source-text 0 main-offset))
  (finally (System/setProperty "babashka.file" test-script)))

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label passed?]
  (swap! checks inc)
  (println (str (if passed? "PASS" "FAIL") " — " label))
  (when-not passed? (swap! failures inc)))

(def sha-a (apply str (repeat 64 "a")))
(def sha-b (apply str (repeat 64 "b")))

(defn temp-directory [prefix]
  (str (Files/createTempDirectory prefix (make-array FileAttribute 0))))

(defn framlog-bytes [space flags]
  (let [space-bytes (.getBytes space StandardCharsets/UTF_8)
        buffer (doto (ByteBuffer/allocate (+ 16 (alength space-bytes)))
                 (.order ByteOrder/LITTLE_ENDIAN))]
    (.put buffer (.getBytes "FRAMLOG\u0000" StandardCharsets/UTF_8))
    (.putShort buffer (short 1))
    (.putShort buffer (short flags))
    (.putInt buffer (alength space-bytes))
    (.put buffer space-bytes)
    (.array buffer)))

(let [directory (temp-directory "fram-forward-header-")
      path (str directory "/history.framlog")]
  (Files/write (.toPath (java.io.File. path))
               (framlog-bytes "north-coordination" 1)
               (make-array java.nio.file.OpenOption 0))
  (check! "FRAMLOG header verifies exact version, flags=1, and SpaceId"
          (= {:version 1 :flags 1 :space-id "north-coordination"}
             (framlog-header! path))))

(let [normalized {:path "/sealed/coordination.normalized.log"
                  :bytes 10 :sha256 sha-a}
      output {:path "/sealed/coordination.framlog" :bytes 20 :sha256 sha-b}
      header {:version 1 :flags 1 :space-id "north-coordination"}
      manifest {:format "fram-triple-log-migration-manifest/v1"
                :space-id "north-coordination"
                :source normalized
                :output (merge migration-encoding
                               (select-keys output [:bytes :sha256]))
                :summary {:diagnostic-count 0}
                :torn-tail nil}]
  (check! "sealed manifest binds input hash, output hash, flags, and SpaceId"
          (valid-migration? "north-coordination" manifest normalized output header))
  (check! "flags=0 is rejected"
          (not (valid-migration? "north-coordination"
                                 (assoc-in manifest [:output :framlog-flags] 0)
                                 normalized output header)))
  (check! "a different SpaceId is rejected"
          (not (valid-migration? "north-telemetry" manifest normalized output header)))
  (check! "a different FRAMLOG hash is rejected"
          (not (valid-migration? "north-coordination" manifest normalized
                                 (assoc output :sha256 sha-a) header)))
  (check! "migration diagnostics are rejected"
          (not (valid-migration? "north-coordination"
                                 (assoc-in manifest [:summary :diagnostic-count] 1)
                                 normalized output header))))

(let [directory (temp-directory "fram-forward-copy-")
      source (str directory "/source.log")
      target-directory (str directory "/sealed")
      target (str target-directory "/source.log")]
  (spit source "{:tx 1}\n")
  (Files/createDirectories (.toPath (java.io.File. target-directory))
                           (make-array FileAttribute 0))
  (let [receipt (seal-copy! "fixture" source target)]
    (check! "sealed preimage records the source and its byte-identical copy"
            (and (= (fingerprint! "fixture source" source) (:source receipt))
                 (= (select-keys (:source receipt) [:bytes :sha256])
                    (select-keys (:sealed receipt) [:bytes :sha256]))))))

(let [directory (temp-directory "fram-forward-selector-")
      selector (str directory "/state/framrpc.env")
      config {:selector-path selector
              :ports {:coordination 17977 :telemetry 17978}}
      contract {:source {:path exact-fram-source
                         :out (str exact-fram-source "/out")}
                :native-artifact {:directory exact-native-artifact-dir
                                  :closure-sha256
                                  exact-native-closure-sha256
                                  :server {:path (str exact-native-artifact-dir
                                                      "/bin/fram-server-native")
                                           :sha256 sha-a}}}
      migrations {:coordination {:output {:path (str directory "/coordination.framlog")}}
                  :telemetry {:output {:path (str directory "/telemetry.framlog")}}}
      content (selector-content config contract migrations)]
  (check! "selector chooses only the exact Native READY artifact"
          (and (str/includes? content exact-native-artifact-dir)
               (str/includes? content "FRAM_SERVER_RUNTIME='native'")
               (str/includes? content
                              (str "FRAM_NATIVE_CLOSURE_SHA256='"
                                   exact-native-closure-sha256 "'"))
               (str/includes? content "FRAM_SERVER_ARTIFACT_SHA256='aaaaaaaa")
               (not (str/includes? content "GRAAL"))
               (= 1 (count (re-seq #"export FRAM_SERVER_RUNTIME=" content)))))
  (check! "selector fences both canonical FRAMLOG paths"
          (and (str/includes? content (get-in migrations [:coordination :output :path]))
               (str/includes? content (get-in migrations [:telemetry :output :path]))))
  (with-redefs [port-open? (constantly false)]
    (require-fenced! (assoc config :sources {}))
    (durable-atomic-write! selector content))
  (check! "selector publication creates one regular file with exact bytes"
          (and (.isFile (java.io.File. selector))
               (= content (slurp selector))))
  (check! "selector is valid shell environment syntax"
          (zero? (:exit (process-result ["bash" "-n" selector] root {}))))
  (let [error (with-redefs [port-open? (constantly false)]
                (try (require-fenced! (assoc config :sources {})) nil
                     (catch clojure.lang.ExceptionInfo value value)))]
    (check! "one-shot selection refuses a second publication"
            (and error (str/includes? (.getMessage error) "one-shot operation refuses")))))

(let [error (with-redefs [port-open? (constantly true)]
              (try (require-fenced! {:selector-path "/nonexistent/selector"
                                     :ports {:coordination 7977}
                                     :sources {}})
                   nil
                   (catch clojure.lang.ExceptionInfo value value)))]
  (check! "conversion refuses an unfenced writer"
          (and error (str/includes? (.getMessage error) "must already be fenced"))))

(check! "production contract is the exact frozen Fram Native artifact"
        (and (= exact-fram-revision (:fram-revision production-config))
             (= exact-fram-tree (:fram-tree production-config))
             (= exact-fram-source (:fram-source production-config))
             (not (str/ends-with? exact-fram-source "/main"))
             (= exact-native-artifact-dir
                (:native-artifact-dir production-config))
             (str/ends-with? exact-native-artifact-dir
                             exact-native-closure-sha256)
             (= exact-native-closure-sha256
                (:native-closure-sha256 production-config))))

(println (format "fram forward cutover: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
