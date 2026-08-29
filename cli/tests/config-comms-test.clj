#!/usr/bin/env bb
;; End-to-end contract for `north config comms` against isolated state.
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-config-comms-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def scratch-home (str tmp-dir "/home"))
(def state (str tmp-dir "/harness.conf"))
(def mail-root (str tmp-dir "/mail"))
(def doctor (str tmp-dir "/doctor"))
(def runtime-bb
  (.getCanonicalPath
   (io/file (System/getenv "HOME") ".local/state/north/runtime-profile/bin/bb")))
(def checks (atom []))
(def captured-output (atom []))
(def forbidden-online-token
  #"(?i)(^|[^A-Za-z0-9_])presence([^A-Za-z0-9_]|$)")

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn capture-result [result]
  (swap! captured-output conj (str (:out result) (:err result)))
  result)

(defn run-config [& args]
  (capture-result
   (apply p/shell
          {:out :string
           :err :string
           :continue true
           :extra-env {"HOME" scratch-home
                       "NORTH_HOME" root
                       "NORTH_HARNESS_STATE" state
                       "NORTH_COMMS_BIN" doctor}}
          (into [runtime-bb cli "comms"] args))))

(defn run-status []
  (capture-result
   (p/shell
    {:out :string
     :err :string
     :continue true
     :extra-env {"HOME" scratch-home
                 "NORTH_HOME" root
                 "NORTH_HARNESS_STATE" state
                 "NORTH_REPO_ROOTS" (str "{\"north\":\"" root
                                         "\",\"beagle\":\"/home/tom/code/beagle/main\","
                                         "\"agent-machinery\":\"/home/tom/code/agent-machinery/main\","
                                         "\"nixos-config\":\"/home/tom/code/nixos-config/main\"}")}}
    runtime-bb cli "status")))

(defn stored [key]
  (let [prefix (str key "=")]
    (some->> (when (.isFile (io/file state)) (slurp state))
             str/split-lines
             (filter #(str/starts-with? % prefix))
             last
             (#(subs % (count prefix))))))

(try
  (.mkdirs (io/file scratch-home))
  (spit doctor "#!/usr/bin/env bash\nprintf 'doctor scratch round-trip: PASS\\n'\n")
  (.setExecutable (io/file doctor) true)

  (let [shown (run-config)]
    (check "default show succeeds" (zero? (:exit shown)))
    (check "default remains exact db/forced"
           (every? #(str/includes? (:out shown) %)
                   ["base         db"
                    "native       db"
                    "managed      db"
                    "enforcement  forced"])))

  (let [off (run-config "off" "--native" "--forced")
        shown (run-config "show")]
    (check "off is an explicit surface protocol"
           (and (zero? (:exit off))
                (zero? (:exit shown))
                (re-find #"(?m)^\s+native\s+off\b" (:out shown)))))

  (let [native (run-config "db" "--native")
        managed (run-config "file" "--managed")
        off (run-config "off")]
    (check "base off normalizes provider overrides to inherit"
           (and (every? zero? (map :exit [native managed off]))
                (= "inherit" (stored "comms.native"))
                (= "inherit" (stored "comms.managed"))))
    (check "base off prints all effective communication surfaces"
           (every? #(str/includes? (:out off) %)
                   ["base         off" "native       off"
                    "managed      off" "enforcement  forced"])))

  (let [base (run-config "file" "--biased")
        native (run-config "db" "--native" "--forced")
        managed (run-config "both" "--managed")
        shown (run-config "show")]
    (check "base/native/managed writes succeed"
           (every? zero? (map :exit [base native managed shown])))
    (check "base and enforcement are stored"
           (and (= "file" (stored "comms"))
                (= "forced" (stored "comms.enforcement"))))
    (check "surface overrides are stored"
           (and (= "db" (stored "comms.native"))
                (= "both" (stored "comms.managed"))))
    (check "shared selection reports effective surfaces"
           (and (re-find #"(?m)^\s+native\s+db\b" (:out shown))
                (re-find #"(?m)^\s+managed\s+both\b" (:out shown)))))

  (let [db-poll (run-config "set" "db.poll" "listener")
        budget (run-config "set" "db.budget-ms" "900")
        root-set (run-config "set" "file.root" mail-root)
        file-poll (run-config "set" "file.poll" "inotify")
        retain (run-config "set" "file.retain-hours" "12")]
    (check "validated sub-key writes succeed"
           (every? zero? (map :exit [db-poll budget root-set file-poll retain])))
    (check "validated sub-keys are stored exactly"
           (= {"comms.db.poll" "listener"
               "comms.db.budget-ms" "900"
               "comms.file.root" mail-root
               "comms.file.poll" "inotify"
               "comms.file.retain-hours" "12"}
              (into {}
                    (map (fn [key] [key (stored key)]))
                    ["comms.db.poll" "comms.db.budget-ms"
                     "comms.file.root" "comms.file.poll"
                     "comms.file.retain-hours"]))))

  (let [bad-root (run-config "set" "file.root" "relative")
        bad-budget (run-config "set" "db.budget-ms" "zero")
        bad-key (run-config "set" "unknown" "value")]
    (check "invalid sub-key values fail"
           (every? #(not (zero? %))
                   (map :exit [bad-root bad-budget bad-key]))))

  (let [doctor-result (run-config "doctor")
        status-result (run-status)]
    (check "doctor delegates to the executable seam"
           (and (zero? (:exit doctor-result))
                (= "doctor scratch round-trip: PASS\n" (:out doctor-result))))
    (check "full config report exposes comms"
           (and (zero? (:exit status-result))
                (str/includes? (:out status-result) "7  COMMS")
                (str/includes? (:out status-result)
                               "configure → north config comms"))))

  (check "rendered communication configuration uses only current online vocabulary"
         (not (re-find forbidden-online-token (str/join "\n" @captured-output))))

  (finally
    (doseq [file (reverse (file-seq tmp-dir))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig comms: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
