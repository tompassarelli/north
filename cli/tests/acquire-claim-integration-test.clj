#!/usr/bin/env bb
;; Integration regression for the canonical dispatch-driver claim protocol. Every
;; assertion runs against an isolated Fram coordinator rather than a mocked command.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (System/getenv "FRAM_PATH")
      "/home/tom/code/beagle/main/branch-core"))
(def runtime-classpath (str root "/out:" fram "/out"))
(cp/add-classpath runtime-classpath)
(def acquire-cli (str root "/cli/acquire-cli.clj"))
(def checks (atom []))
(def test-log (atom nil))

(load-file (str root "/cli/coord.clj"))

(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn await-up [port]
  (loop [attempt 0]
    (let [status (try (north.coord/status port) (catch Throwable _ nil))]
      (cond
        (and (= :ready (:state status))
             (= "north-coordination" (:space-id status))) true
        (>= attempt 800) false
        :else (do (Thread/sleep 25) (recur (inc attempt)))))))

(defn assert-fact! [port subject predicate value]
  (let [result (north.coord/append! port subject predicate value)]
    (when (:reject result)
      (throw (ex-info "fixture fact write failed" result)))
    result))

(defn resolved [port subject predicate]
  (north.coord/resolved port subject predicate))

(defn acquire [port verb thread holder]
  (proc/shell {:continue true :out :string :err :string
               :extra-env {"FRAM_LOG" @test-log
                           "FRAM_SPACE_ID" "north-coordination"
                           "NORTH_TELEMETRY_PARTITION" "0"}}
              "bb" "-cp" runtime-classpath acquire-cli
              (str port) verb thread holder))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-acquire-claim" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "facts.framlog")
      thread-id "019f75a8-032c-741a-b65d-e4af097e3837"
      thread (str "@" thread-id)
      unknown-id "019f75a8-032c-741a-b65d-e4af097e3838"
      unknown (str "@" unknown-id)
      first-holder "agent:first"
      second-holder "agent:second"
      daemon-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                  "FRAM_SERVER_QUIET" "1"
                  "FRAM_SERVER_XMX" "1g"
                  "FRAM_SINGLE_VALUED" "title driver"}
      daemon (proc/process {:dir fram
                            :out :string
                            :err :string
                            :extra-env daemon-env}
                           (str fram "/bin/fram-server") "serve" (str port)
                           (.getCanonicalPath log) "north-coordination")]
  (reset! test-log (.getCanonicalPath log))
  (try
    (let [started? (await-up port)]
      (check "throwaway Fram coordinator starts" started?)
      (when-not started?
        (throw (ex-info "throwaway Fram coordinator did not start"
                        {:stdout (deref (:out daemon))
                         :stderr (deref (:err daemon))})))

      (let [missing (acquire port "claim" unknown-id first-holder)]
        (check "claiming an unknown thread exits 4"
               (and (= 4 (:exit missing))
                    (str/includes? (:out missing) "thread does not exist")))
        (check "unknown-thread claim creates no driver"
               (nil? (resolved port unknown "driver"))))

      (assert-fact! port thread "title" "Claim integration fixture")
      (let [first-claim (acquire port "claim" thread-id first-holder)]
        (check "a bare UUID claim resolves the canonical @UUID subject"
               (and (zero? (:exit first-claim))
                    (str/includes? (:out first-claim) "CLAIMED")
                    (str/includes? (:out first-claim) thread)
                    (= (str "@" first-holder) (resolved port thread "driver")))))

      (let [duplicate (acquire port "claim" thread first-holder)]
        (check "an @UUID duplicate reaches the same subject and exits 3"
               (and (= 3 (:exit duplicate))
                    (str/includes? (:out duplicate) "already driven")
                    (= (str "@" first-holder) (resolved port thread "driver")))))

      (let [exact (acquire port "verify" thread-id first-holder)
            wrong (acquire port "verify" thread second-holder)]
        (check "verify succeeds only for the exact holder"
               (and (zero? (:exit exact))
                    (str/includes? (:out exact) "VERIFIED")
                    (= 7 (:exit wrong))
                    (str/includes? (:out wrong) "preclaimed driver is @agent:first")
                    (str/includes? (:out wrong) "expected @agent:second")
                    (= (str "@" first-holder) (resolved port thread "driver")))))

      (let [first-release (acquire port "release" thread first-holder)
            absent (acquire port "verify" thread-id first-holder)
            second-release (acquire port "release" thread-id first-holder)]
        (check "release is idempotent and leaves no driver"
               (and (zero? (:exit first-release))
                    (str/includes? (:out first-release) "released")
                    (= 6 (:exit absent))
                    (str/includes? (:out absent) "preclaimed driver is absent")
                    (zero? (:exit second-release))
                    (str/includes? (:out second-release) "noop")
                    (nil? (resolved port thread "driver")))))

      (let [reclaim (acquire port "claim" thread-id second-holder)]
        (check "a second holder can claim after release"
               (and (zero? (:exit reclaim))
                    (str/includes? (:out reclaim) "CLAIMED")
                    (= (str "@" second-holder) (resolved port thread "driver")))))

      (let [malformed (acquire port "claim" (str "@" thread) first-holder)]
        (check "double-@ input is rejected before any graph mutation"
               (and (= 2 (:exit malformed))
                    (str/includes? (:err malformed) "invalid thread id")
                    (= (str "@" second-holder) (resolved port thread "driver"))))))
    (finally
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [port (free-port)
      unavailable (acquire port "release"
                           "019f75a8-032c-741a-b65d-e4af097e3837"
                           "agent:first")]
  (check "release exits nonzero when safe ownership verification is unavailable"
         (and (= 5 (:exit unavailable))
              (str/includes? (:err unavailable) "safe release unavailable"))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nacquire claim integration: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
