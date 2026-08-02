(load-file "cli/dashboard-cli.clj")
(require '[clojure.java.io :as io] '[clojure.edn :as edn])

(defn fail! [message]
  (binding [*out* *err*] (println "FAIL" message))
  (System/exit 1))

(cache-put! "probe.edn" {:ok true})
(def probe-file (io/file CACHE-DIR (str CACHE-SCOPE "-probe.edn")))

;; A future cache timestamp is invalid, never fresh.
(spit probe-file (pr-str {:ts (+ (System/currentTimeMillis) 60000) :val {:poison true}}))
(when (cache-get "probe.edn" 300000) (fail! "future cache timestamp was accepted"))

;; Nonzero concern probes must not become a cached empty-success result.
(let [concern-file (io/file CACHE-DIR (str CACHE-SCOPE "-concerns.edn"))]
  (.delete concern-file)
  (let [result (with-redefs [run (fn [& _] {:out "" :err "boom" :exit 1 :ok false})]
                 (concern-rows))]
    (when-not (:err result) (fail! "failed concern probe was treated as success"))
    (when (.exists concern-file) (fail! "failed concern probe was cached"))))

;; Java owner-only permission probes are stable across POSIX hosts.
(let [perms (java.nio.file.Files/getPosixFilePermissions
              (.toPath probe-file)
              (make-array java.nio.file.LinkOption 0))
      names (set (map str perms))]
  (when-not (= names #{"OWNER_READ" "OWNER_WRITE"})
    (fail! (str "cache permissions are not 0600: " names))))

;; Workload measurement must mirror the actual doctor inputs: log bytes count
;; toward scan volume, while only direct projected *.md files (except CLAUDE.md)
;; count toward parse overhead.
(let [root (.toFile (java.nio.file.Files/createTempDirectory
                      "north-dashboard-workload"
                      (make-array java.nio.file.attribute.FileAttribute 0)))
      threads (io/file root "threads")
      nested (io/file threads "nested")
      log (io/file root "coordination.log")]
  (try
    (.mkdirs nested)
    (spit log "12345")
    (spit (io/file threads "thread.md") "abc")
    (spit (io/file threads "CLAUDE.md") "ignored")
    (spit (io/file threads "not-a-thread.txt") "ignored")
    (spit (io/file nested "nested.md") "ignored")
    (when-not (= 5 (log-workload (.getPath log)))
      (fail! "log workload did not measure exact bytes"))
    (when-not (= {:bytes 3 :files 1} (thread-workload (.getPath threads)))
      (fail! "thread workload diverged from fram.rt/list-md semantics"))
    (finally
      (doseq [f (reverse (file-seq root))]
        (io/delete-file f true)))))

;; Coordinator doctor is a full-corpus fold. Its deadline must grow with both
;; byte volume and projection-file count, retain a finite ceiling, and never
;; regress to the old fixed 6s race at a production-sized workload.
(let [empty-budget (coord-doctor-timeout-ms {:bytes 0 :files 0})
      production-budget (coord-doctor-timeout-ms {:bytes (* 18 MIB) :files 1500})
      larger-budget (coord-doctor-timeout-ms {:bytes (* 36 MIB) :files 3000})
      capped-budget (coord-doctor-timeout-ms {:bytes (* 1024 MIB) :files 100000})]
  (when-not (= COORD-DOCTOR-BASE-MS empty-budget)
    (fail! (str "empty workload did not receive only base overhead: " empty-budget)))
  (when-not (> production-budget 6000)
    (fail! (str "production workload regressed to the old brittle deadline: " production-budget)))
  (when-not (> larger-budget production-budget)
    (fail! "coordinator deadline does not scale with workload"))
  (when-not (= COORD-DOCTOR-MAX-MS capped-budget)
    (fail! (str "coordinator deadline is not bounded: " capped-budget))))

;; The impure wrapper must pass the derived budget to the process runner and
;; retain workload evidence for an actionable timeout diagnostic.
(let [seen (atom nil)
      workload {:bytes (* 18 MIB) :files 1500}
      result (with-redefs [coord-doctor-workload (constantly workload)
                           run (fn [_ & options]
                                 (reset! seen (:timeout (apply hash-map options)))
                                 {:timeout true :ok false})]
               (coord-doctor-probe))]
  (when-not (= @seen (coord-doctor-timeout-ms workload))
    (fail! (str "coordinator probe ignored its derived deadline: " @seen)))
  (when-not (= workload (:workload result))
    (fail! "coordinator probe dropped workload evidence")))

(println "dashboard-cache: passed")
