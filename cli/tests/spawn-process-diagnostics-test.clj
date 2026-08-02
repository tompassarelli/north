#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/spawn-process.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn eventually [pred timeout-ms]
  (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
    (loop []
      (cond (pred) true
            (>= (System/currentTimeMillis) deadline) false
            :else (do (Thread/sleep 20) (recur))))))

(def temp-dir (.toFile (java.nio.file.Files/createTempDirectory
                        "north-spawn-diagnostics-"
                        (make-array java.nio.file.attribute.FileAttribute 0))))
(def log (io/file temp-dir "lane-startup.log"))
(def heartbeat (io/file (str log north.spawn-process/detached-heartbeat-suffix)))
(def process
  (binding [north.spawn-process/*heartbeat-interval-seconds* 1]
    (north.spawn-process/launch-detached!
     ["sleep" "10"] (into {} (System/getenv)) log)))

(try
  (check "production heartbeat cadence is 30 seconds"
         (= 30 north.spawn-process/*heartbeat-interval-seconds*))
  (check "lane wrapper publishes a heartbeat while alive"
         (eventually #(.isFile heartbeat) 1000))
  (let [first-touch (.lastModified heartbeat)]
    (check "lane wrapper refreshes the heartbeat for its full lifetime"
           (eventually #(> (.lastModified heartbeat) first-touch) 2500)))
  (let [startup (north.spawn-process/await-startup
                 process "lane-startup-diagnostic" log
                 (constantly {}) (constantly false)
                 :timeout-ms 100 :poll-ms 10)
        durable (slurp log)]
    (check "startup acknowledgement timeout remains the returned failure"
           (= :timeout (:status startup)))
    (check "startup acknowledgement reason is appended to the lane log"
           (and (str/includes? durable
                               "[north startup] NEVER-ACKNOWLEDGED agent lane-startup-diagnostic")
                (str/includes? durable "startup acknowledgement timed out after 100ms")
                (str/includes? durable "missing identity: kind"))))
  (finally
    (north.spawn-process/stop-process! process)
    (doseq [file (reverse (file-seq temp-dir))]
      (io/delete-file file true))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "spawn process diagnostics: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
