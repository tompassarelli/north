#!/usr/bin/env bb
;; Canonical FRAMRPC gate for north.coord/assert-after-read!.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file *file*)) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                "/home/tom/code/beagle/main/branch-core"))))
(when-not (.isFile (io/file fram "bin/fram-server"))
  (throw (ex-info "current Beagle branch-core engine is required" {:fram fram})))
(load-file (str root "/cli/coord.clj"))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn eventually [f]
  (loop [remaining 1500]
    (cond
      (try (f) (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 50) (recur (dec remaining))))))

(let [port (free-port)
      dir (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-assert-after-read"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "coordination.framlog")
      explicit-log-probe
      (proc/shell
       {:out :string :err :string :continue true
        :extra-env {"FRAM_LOG" (.getCanonicalPath log)}}
       "bb" "-cp" (str fram "/out") "-e"
       (str "(load-file " (pr-str (str root "/cli/coord.clj")) ")"
            "(print (north.coord/expected-log))"))
      daemon
      (proc/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                    "FRAM_SERVER_QUIET" "1"
                    "FRAM_SERVER_XMX" "1g"}}
       (str fram "/bin/fram-server") "serve" (str port)
       (.getCanonicalPath log) "north-coordination")
      checks (atom [])
      check! (fn [label value]
               (swap! checks conj [label (boolean value)]))]
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check! "explicit FRAM_LOG remains the canonical migration identity"
            (and (zero? (:exit explicit-log-probe))
                 (= (.getCanonicalPath log)
                    (str/trim (:out explicit-log-probe)))))
    (check! "current Fram server is ready on the expected SpaceId"
            (eventually
             #(let [status (north.coord/status port)]
                (and (= :ready (:state status))
                     (= "north-coordination" (:space-id status))))))

    (let [validations (atom 0)
          result
          (north.coord/assert-after-read!
           port "@run-revalidated" "run_bar_evidence" "record"
           (fn []
             (when (= 1 (swap! validations inc))
               (north.coord/append!
                port "@unrelated" "unrelated_predicate" "moved"))))]
      (check! "an intervening transaction forces callback revalidation"
              (= 2 @validations))
      (check! "the uncontested retry commits through canonical FRAMRPC"
              (and (:ok result)
                   (= #{"record"}
                      (set (north.coord/many
                            port "@run-revalidated"
                            "run_bar_evidence"))))))

    (let [attempts (atom 0)
          result
          (north.coord/retry-conflicts-until!
           (north.coord/retry-deadline-ns 1000) 4
           #(if (< (swap! attempts inc) 3)
              {:reject :conflict}
              {:ok true}))]
      (check! "conflicts retry until one canonical transaction succeeds"
              (and (:ok result) (= 3 @attempts))))

    (let [running? (atom true)
          churn-writes (atom 0)
          writer
          (future
            (while @running?
              (north.coord/append!
               port "@unrelated-churn" "noise"
               (str (swap! churn-writes inc)))
              (Thread/sleep 10)))
          results
          (try
            (Thread/sleep 50)
            (mapv
             (fn [index]
               (north.coord/assert-after-read!
                port (str "@run-churn-" index)
                "run_bar_evidence" "record" (fn [] nil)))
             (range 24))
            (finally
              (reset! running? false)
              @writer))]
      (check! "unrelated canonical transactions race the fixture"
              (pos? @churn-writes))
      (check! "bounded marker publications converge under unrelated churn"
              (every? :ok results)))

    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label ok?] @checks]
        (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed"
                         (- (count @checks) (count failed))
                         (count @checks)))
        (when (seq failed) (System/exit 1))))))
