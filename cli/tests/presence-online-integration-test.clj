#!/usr/bin/env bb
;; The live roster must scale with live leases, not the lifetime count of
;; historical sessions. Exercise the live-only projection against a throwaway
;; coordinator and prove a lapsed session remains visible historically but is
;; excluded from the bounded roster input.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_HOME")
      "/home/tom/code/beagle/main/store"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(when-not (.isFile (io/file store "bin/beagle-store-server"))
  (throw (ex-info "current Beagle store engine is required" {:store store})))
(load-file (str root "/cli/coord.clj"))
(def checks (atom []))
(def test-log (atom nil))

(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))
(defn await-port [port]
  (loop [attempt 0]
    (cond (port-open? port) true
          (>= attempt 200) false
          :else (do (Thread/sleep 25) (recur (inc attempt))))))
(defn run-presence [port & args]
  (apply proc/sh {:out :string :err :string :continue true
                  :extra-env {"BEAGLE_STORE_LOG" @test-log
                              "NORTH_TELEMETRY_PARTITION" "0"}}
         "bb" presence-cli (str port) args))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-presence-online" (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "coordination.storelog")
      log (.getCanonicalPath facts)
      daemon (do
               (proc/process
                {:dir store :out :string :err :string
                 :extra-env {"BEAGLE_STORE_SERVER_RUNTIME" "jvm-dev"
                             "BEAGLE_STORE_SERVER_QUIET" "1"
                             "BEAGLE_STORE_SERVER_XMX" "1g"}}
                (str store "/bin/beagle-store-server") "serve" (str port)
                log "north-coordination"))]
  (reset! test-log log)
  (try
    (check "throwaway current Beagle Store server starts"
           (and (await-port port)
                (= :ready (:state (north.coord/status port)))))
    (let [live-registration
          (run-presence port "register" "live-session" "/tmp/live" "live-session")
          lapsed-registration
          (run-presence port "register" "lapsed-session" "/tmp/lapsed" "lapsed-session")]
      (check "live session registers" (zero? (:exit live-registration)))
      (check "historical session registers" (zero? (:exit lapsed-registration)))
      (north.coord/release-lease!
       port (json/parse-string (str/trim (:out lapsed-registration)) true)))
    (let [live (:out (run-presence port "presence-online"))
          full (:out (run-presence port "presence"))]
      (check "live-only projection includes the unexpired session"
             (str/includes? live "live-session"))
      (check "live-only projection excludes historical lapsed sessions"
             (not (str/includes? live "lapsed-session")))
      (check "full historical projection remains available"
             (and (str/includes? full "live-session")
                  (str/includes? full "lapsed-session")
                  (str/includes? full "lapsed"))))
    (finally
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\npresence online integration: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
