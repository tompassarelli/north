#!/usr/bin/env bb
(load-file "cli/dashboard-cli.clj")
(require '[clojure.java.io :as io] '[clojure.edn :as edn])

(defn fail! [message]
  (binding [*out* *err*] (println "FAIL" message))
  (System/exit 1))

(def legacy-cache-root (.getPath (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-legacy-cache" (make-array java.nio.file.attribute.FileAttribute 0)))))
(alter-var-root #'CACHE-DIR (constantly legacy-cache-root))
(cache-put! "probe.edn" {:ok true})
(def probe-file (io/file CACHE-DIR (str CACHE-SCOPE "-probe.edn")))

(spit probe-file (pr-str {:ts (+ (System/currentTimeMillis) 60000) :val {:poison true}}))
(when (cache-get "probe.edn" 300000) (fail! "future cache timestamp was accepted"))

(let [concern-file (io/file CACHE-DIR (str CACHE-SCOPE "-concerns.edn"))]
  (.delete concern-file)
  (let [result (with-redefs [run (fn [& _] {:out "" :err "boom" :exit 1 :ok false})]
                 (concern-rows))]
    (when-not (:err result) (fail! "failed concern probe was treated as success"))
    (when (.exists concern-file) (fail! "failed concern probe was cached"))))

(let [perms (java.nio.file.Files/getPosixFilePermissions
              (.toPath probe-file) (make-array java.nio.file.LinkOption 0))
      names (set (map str perms))]
  (when-not (= names #{"OWNER_READ" "OWNER_WRITE"})
    (fail! (str "cache permissions are not 0600: " names))))

(let [log-workload (resolve 'log-workload)
      thread-workload (resolve 'thread-workload)
      root (.toFile (java.nio.file.Files/createTempDirectory
                      "north-dashboard-workload"
                      (make-array java.nio.file.attribute.FileAttribute 0)))
      threads (io/file root "threads") nested (io/file threads "nested")
      log (io/file root "coordination.log")]
  (try
    (when (and log-workload thread-workload)
      (.mkdirs nested)
    (spit log "12345")
    (spit (io/file threads "thread.md") "abc")
    (spit (io/file threads "CLAUDE.md") "ignored")
    (spit (io/file threads "not-a-thread.txt") "ignored")
    (spit (io/file nested "nested.md") "ignored")
      (when-not (= 5 (log-workload (.getPath log))) (fail! "log workload did not measure exact bytes"))
      (when-not (= {:bytes 3 :files 1} (thread-workload (.getPath threads)))
        (fail! "thread workload diverged from fram.rt/list-md semantics")))
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))

(let [coord-doctor-timeout-ms (resolve 'coord-doctor-timeout-ms)
      coord-doctor-workload (resolve 'coord-doctor-workload)
      coord-doctor-probe (resolve 'coord-doctor-probe)
      mib (resolve 'MIB) base (resolve 'COORD-DOCTOR-BASE-MS) maximum (resolve 'COORD-DOCTOR-MAX-MS)]
  (when (and coord-doctor-timeout-ms coord-doctor-workload coord-doctor-probe mib base maximum)
    (let [empty-budget (coord-doctor-timeout-ms {:bytes 0 :files 0})
          production-budget (coord-doctor-timeout-ms {:bytes (* 18 @mib) :files 1500})
          larger-budget (coord-doctor-timeout-ms {:bytes (* 36 @mib) :files 3000})
          capped-budget (coord-doctor-timeout-ms {:bytes (* 1024 @mib) :files 100000})]
      (when-not (= @base empty-budget) (fail! (str "empty workload did not receive only base overhead: " empty-budget)))
      (when-not (> production-budget 6000) (fail! (str "production workload regressed to the old brittle deadline: " production-budget)))
      (when-not (> larger-budget production-budget) (fail! "coordinator deadline does not scale with workload"))
      (when-not (= @maximum capped-budget) (fail! (str "coordinator deadline is not bounded: " capped-budget)))
      (let [seen (atom nil) workload {:bytes (* 18 @mib) :files 1500}
            result (with-redefs-fn {coord-doctor-workload (constantly workload)
                                    #'run (fn [_ & options]
                                            (reset! seen (:timeout (apply hash-map options)))
                                            {:timeout true :ok false})}
                     #(coord-doctor-probe))]
        (when-not (= @seen (coord-doctor-timeout-ms workload)) (fail! (str "coordinator probe ignored its derived deadline: " @seen)))
        (when-not (= workload (:workload result)) (fail! "coordinator probe dropped workload evidence"))))))

(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-envelope" (make-array java.nio.file.attribute.FileAttribute 0)))]
  (try
    (with-redefs [north.dashboard.state/cache-dir (constantly (.getPath root))]
      (north.dashboard.state/record! :board {:status :ok :data {:open 3}})
      (let [before (north.dashboard.state/read-panel :board)]
        (north.dashboard.state/record! :board {:status :timeout :detail "deadline"})
        (let [panel (north.dashboard.state/read-panel :board)
              dir-perms (set (map str (java.nio.file.Files/getPosixFilePermissions (.toPath root) (make-array java.nio.file.LinkOption 0))))
              file-perms (set (map str (java.nio.file.Files/getPosixFilePermissions (.toPath (north.dashboard.state/panel-file :board)) (make-array java.nio.file.LinkOption 0))))]
          (when-not (= "north.dashboard/panel-v1" (:schema panel)) (fail! "panel schema missing"))
          (when-not (= {:open 3} (get-in panel [:last-good :data])) (fail! "failed attempt destroyed last-good"))
          (when-not (= (dissoc before :last-attempt) (dissoc panel :last-attempt)) (fail! "failed attempt changed more than last-attempt"))
          (when-not (= "timeout" (get-in panel [:last-attempt :status])) (fail! "attempt status missing"))
          (when-not (= #{"OWNER_READ" "OWNER_WRITE" "OWNER_EXECUTE"} dir-perms) (fail! (str "cache directory is not 0700: " dir-perms)))
          (when-not (= #{"OWNER_READ" "OWNER_WRITE"} file-perms) (fail! (str "panel file is not 0600: " file-perms)))
          (when (seq (filter #(.contains (.getName %) ".tmp") (file-seq root))) (fail! "atomic replacement left a temp file")))))
    (println "dashboard-cache: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
