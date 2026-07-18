;; validate_process_test.clj — the user-facing `north validate` process contract:
;; clean graph exits 0; any reported violation exits nonzero. Both probes use an
;; isolated fact log and an unreachable coordinator port, so no live North state
;; can influence the result.
;;   bb ~/code/north/validate_process_test.clj
(require '[babashka.fs :as fs]
         '[babashka.process :as proc]
         '[clojure.string :as str])

(def repo
  (.getCanonicalPath
   (.getParentFile
    (.getCanonicalFile (java.io.File. *file*)))))
(def fram (.getCanonicalPath (java.io.File. repo "../fram")))
(def root
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-validate-process-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def clean-log (str root "/clean.log"))
(def dirty-log (str root "/dirty.log"))
(def threads-dir (str root "/threads"))

(defn fact-line [tx l p r]
  (pr-str {:tx tx :op "assert" :l l :p p :r r :frame "fixture"}))

(spit clean-log
      (str (fact-line 1 "@clean" "title" "Clean validation fixture") "\n"))
(spit dirty-log
      (str (fact-line 1 "@dirty" "title" "Dirty validation fixture") "\n"
           (fact-line 2 "@dirty" "depends_on" "@missing") "\n"))

(defn validate-process [log]
  (proc/shell
   {:continue true
    :dir repo
    :out :string
    :err :string
    :extra-env {"FRAM_HOME" fram
                "FRAM_LOG" log
                "FRAM_TELEMETRY_LOG" ""
                "FRAM_THREADS" threads-dir
                "FRAM_PORT" "1"}}
   (str repo "/bin/north") "validate"))

(def clean-result (validate-process clean-log))
(def dirty-result (validate-process dirty-log))

(def checks
  [["clean graph exits 0" (= 0 (:exit clean-result))]
   ["clean graph reports no violations"
    (str/includes? (:out clean-result) "threads, no violations")]
   ["violating graph exits nonzero" (not= 0 (:exit dirty-result))]
   ["violating graph exits with the stable failure status" (= 1 (:exit dirty-result))]
   ["violating graph reports the missing subject"
    (str/includes? (:out dirty-result)
                   "depends_on references missing entity @missing")]
   ["violating graph reports its count"
    (str/includes? (:out dirty-result) "1 violation(s).")]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (fs/delete-tree root)
  (if (empty? fails)
    (println "\nnorth validate process:" (count checks) "/" (count checks) "PASS")
    (do
      (println "\nnorth validate process:" (count fails) "FAILED")
      (System/exit 1))))
