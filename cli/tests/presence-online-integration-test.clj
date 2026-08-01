#!/usr/bin/env bb
;; The live roster must scale with live leases, not the lifetime count of
;; historical sessions. Exercise the live-only projection against a throwaway
;; coordinator and prove a lapsed session remains visible historically but is
;; excluded from the bounded roster input.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (System/getenv "FRAM_HOME")
      (str (System/getProperty "user.home") "/code/fram/main")))
(def presence-cli (str root "/cli/presence-cli.clj"))
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
(defn coordinator-op [port request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write writer
              (.getBytes
               (str (pr-str {:op :for-log
                             :expected-log @test-log
                             :request request})
                    "\n")))
      (.flush writer)
      (edn/read-string (.readLine reader)))))
(defn run-presence [port & args]
  (apply proc/sh {:out :string :err :string :continue true
                  :extra-env {"FRAM_LOG" @test-log
                              "NORTH_TELEMETRY_PARTITION" "0"}}
         "bb" presence-cli (str port) args))
(defn run-against-response [response & args]
  (with-open [server (java.net.ServerSocket. 0)]
    (let [served
          (future
            (with-open [socket (.accept server)
                        reader (io/reader (.getInputStream socket))]
              (.readLine reader)
              (let [writer (.getOutputStream socket)]
                (.write writer
                        (.getBytes (str (pr-str response) "\n")
                                   java.nio.charset.StandardCharsets/UTF_8))
                (.flush writer))))
          result (apply run-presence (.getLocalPort server) args)]
      @served
      result)))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-presence-online" (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "facts.log")
      _ (spit facts "")
      log (.getCanonicalPath facts)
      telemetry (io/file tmp "telemetry.log")
      _ (spit telemetry "")
      daemon (do
               (proc/process {:dir fram :out :string :err :string
                              :extra-env {"FRAM_LOG" log
                                          "FRAM_TELEMETRY_LOG"
                                          (.getCanonicalPath telemetry)
                                          "NORTH_TELEMETRY_PARTITION" "0"
                                          "NORTH_TELEMETRY_PORT" (str port)
                                          "FRAM_REQUIRE_LOG_FENCE" "1"
                                          "FRAM_SINGLE_VALUED" "agent dir session_id started_at"}}
                             "bb" "-cp" "out" "coord_daemon.clj"
                             "serve-flat" (str port) log))]
  (reset! test-log log)
  (try
    (check "throwaway Fram coordinator starts" (await-port port))
    (check "live session registers"
           (zero? (:exit (run-presence port "register" "live-session" "/tmp/live" "live-session"))))
    (check "historical session registers"
           (zero? (:exit (run-presence port "register" "lapsed-session" "/tmp/lapsed" "lapsed-session"))))
    (coordinator-op port {:op :release-lease :res "session:lapsed-session" :holder "lapsed-session"})
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
    (let [error-result
          (run-against-response
           {:error ["coordinator unavailable"] :version 1 :engine "index"}
           "presence-online-json")
          malformed-row-result
          (run-against-response
           {:ok [["@lease:session:broken"]] :version 1 :engine "index"}
           "presence-online-json")
          unsafe-version-result
          (run-against-response
           {:ok [] :version 9007199254740992 :engine "index"}
           "presence-online-json")
          malformed-lease-result
          (run-against-response
           {:ok [["@lease:session:broken" "not-a-lease"]]
            :version 1 :engine "index"}
           "presence-online-json")
          wrong-holder-result
          (run-against-response
           {:ok [["@lease:session:broken" "someone-else|9999999999999|1"]]
            :version 1 :engine "index"}
           "presence-online-json")
          zero-epoch-result
          (run-against-response
           {:ok [["@lease:session:broken" "broken|9999999999999|0"]]
            :version 1 :engine "index"}
           "presence-online-json")
          overflow-result
          (run-against-response
           {:ok [["@lease:session:broken"
                  "broken|9007199254740992|1"]]
            :version 1 :engine "index"}
           "presence-online-json")
          duplicate-distinct-result
          (run-against-response
           {:ok [["@lease:session:duplicate" "duplicate|9999999999999|1"]
                 ["@lease:session:duplicate" "duplicate|9999999999998|2"]]
            :version 1 :engine "index"}
           "presence-online-json")
          duplicate-exact-result
          (run-against-response
           {:ok [["@lease:session:duplicate" "duplicate|9999999999999|1"]
                 ["@lease:session:duplicate" "duplicate|9999999999999|1"]]
            :version 1 :engine "index"}
           "presence-online-json")]
      (check "coordinator error cannot become a successful empty JSON roster"
             (and (not (zero? (:exit error-result)))
                  (not (str/includes? (:out error-result)
                                      "north:presence-online:v1"))))
      (check "malformed coordinator rows fail the JSON roster closed"
             (and (not (zero? (:exit malformed-row-result)))
                  (not (str/includes? (:out malformed-row-result)
                                      "north:presence-online:v1"))))
      (check "unsafe coordinator versions fail the JSON roster closed"
             (not (zero? (:exit unsafe-version-result))))
      (check "malformed lease values fail the JSON roster closed"
             (and (not (zero? (:exit malformed-lease-result)))
                  (not (str/includes? (:out malformed-lease-result)
                                      "north:presence-online:v1"))))
      (check "session lease holder mismatch fails the JSON roster closed"
             (not (zero? (:exit wrong-holder-result))))
      (check "zero-epoch session lease fails the JSON roster closed"
             (not (zero? (:exit zero-epoch-result))))
      (check "overflowing lease integers fail the JSON roster closed"
             (not (zero? (:exit overflow-result))))
      (check "distinct duplicate session leases fail the JSON roster closed"
             (not (zero? (:exit duplicate-distinct-result))))
      (check "exact duplicate session leases fail the JSON roster closed"
             (not (zero? (:exit duplicate-exact-result)))))
    (let [rejected-renewal
          (run-against-response
           {:reject [:held "session:rejected" "other-holder"]}
           "renew" "rejected")
          malformed-renewal
          (run-against-response
           {:ok true :holder "malformed" :exp "not-an-integer" :epoch 1}
           "renew" "malformed")
          wrong-holder-renewal
          (run-against-response
           {:ok 7
            :holder "different-session"
            :exp (+ (System/currentTimeMillis) 60000)
            :epoch 7}
           "renew" "expected-session")]
      (check "a rejected session renewal exits nonzero"
             (not (zero? (:exit rejected-renewal))))
      (check "a malformed session lease grant exits nonzero"
             (not (zero? (:exit malformed-renewal))))
      (check "a mismatched session lease holder exits nonzero"
             (not (zero? (:exit wrong-holder-renewal)))))
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
