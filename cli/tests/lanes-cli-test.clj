#!/usr/bin/env bb
;; bb -cp <fram-out> cli/tests/lanes-cli-test.clj — lanes-cli.clj loads
;; framrpc-client.clj unconditionally, so fram.types must be on the classpath.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[fram.types :as t])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/lanes-cli.clj"))
;; lanes-cli.clj loads framrpc-client.clj lazily (only when the native path
;; actually runs); the framrpc-path checks below need the alias up front.
(load-file (str root "/cli/framrpc-client.clj"))
(require '[north.framrpc-client :as rpc])

(def failures (atom 0))
(def checks (atom 0))
(defn check [label ok?]
  (swap! checks inc)
  (when-not ok?
    (swap! failures inc)
    (println "FAIL:" label)))

(def temp-dir (.toFile (java.nio.file.Files/createTempDirectory
                        "north-lanes-cli-"
                        (make-array java.nio.file.attribute.FileAttribute 0))))
(def now-ms (.toEpochMilli (java.time.Instant/parse "2026-08-03T00:00:00Z")))
(defn lane-file [id suffix] (io/file temp-dir (str id ".log" suffix)))
(defn fixture! [id thread last-line]
  (spit (lane-file id "") (str "header\n" last-line "\n"))
  (spit (io/file temp-dir (str id ".meta.json"))
        (json/generate-string
         {"thread" thread "startedAt" "2026-08-02T23:30:00Z"})))
(defn mtime! [file millis]
  (when-not (.setLastModified file millis)
    (throw (ex-info "could not set fixture mtime" {:file file}))))

(try
  (doseq [[id thread last-line]
          [["lane-done" "thread-done" "complete"]
           ["lane-failed" "thread-failed" "provider failed"]
           ["lane-working" "thread-working" "turn 8"]
           ["lane-killed" "thread-killed" "spawn header only"]
           ["lane-never-ack" "thread-never" "[north startup] NEVER-ACKNOWLEDGED agent lane-never-ack startup acknowledgement timed out after 100ms; missing identity: kind"]]]
    (fixture! id thread last-line))
  (spit (lane-file "lane-done" ".lane.exit") "0\n")
  (spit (lane-file "lane-failed" ".lane.exit") "23\n")
  (spit (lane-file "lane-working" ".lane.heartbeat") "")
  (mtime! (lane-file "lane-working" ".lane.heartbeat") (- now-ms 30000))
  (spit (lane-file "lane-killed" ".lane.heartbeat") "")
  (mtime! (lane-file "lane-killed" ".lane.heartbeat") (- now-ms 91000))
  ;; A wrapper exit receipt cannot disguise a failed startup acknowledgement.
  (spit (lane-file "lane-never-ack" ".lane.exit") "143\n")

  (let [titles {"thread-done" "Delivered lane"
                "thread-failed" "Failed lane"
                "thread-working" "Working lane"
                "thread-killed" "Killed lane"
                "thread-never" "Never acknowledged lane"}
        rows (north.lanes-cli/lane-rows temp-dir titles now-ms)
        by-id (into {} (map (juxt :id identity) rows))
        rendered (with-out-str (north.lanes-cli/render rows))]
    (check "done fixture is classified from exit 0"
           (= :done (get-in by-id ["lane-done" :status])))
    (check "failed fixture is classified from nonzero exit"
           (= :failed (get-in by-id ["lane-failed" :status])))
    (check "fresh heartbeat fixture is working"
           (= :working (get-in by-id ["lane-working" :status])))
    (check "heartbeat older than 90 seconds with no exit is killed"
           (= :killed (get-in by-id ["lane-killed" :status])))
    (check "startup diagnostic outranks an exit receipt"
           (= :never-acknowledged (get-in by-id ["lane-never-ack" :status])))
    (check "thread title and final durable line are retained"
           (and (= "Killed lane" (get-in by-id ["lane-killed" :title]))
                (= "spawn header only" (get-in by-id ["lane-killed" :last-line]))))
    (check "killed and never-acknowledged render loudly"
           (and (str/includes? rendered "!!! KILLED !!!")
                (str/includes? rendered "!!! NEVER-ACKNOWLEDGED !!!"))))

  (check "v03 path reads the live :resolved title projection"
         (with-redefs [north.coord/resolved
                       (fn [port entity predicate]
                         (when (and (= 7977 port) (= "title" predicate))
                           (get {"@thread-a" "Thread A" "@thread-b" ""} entity)))]
           (= {"thread-a" "Thread A"}
              (north.lanes-cli/v03-resolve-titles 7977 ["thread-a" "thread-b" "thread-c"]))))

  (check "v03 path degrades to no titles when the coordinator is unreachable"
         (with-redefs [north.coord/resolved
                       (fn [& _] (throw (ex-info "coordinator unreachable" {})))]
           (= {} (north.lanes-cli/v03-resolve-titles 7977 ["thread-a" "thread-b"]))))

  (let [stub-client (fn [_host port space _options] {:host "127.0.0.1" :port port :space-id space})]
    (check "framrpc path reads the native scan title projection"
           (with-redefs [rpc/connect stub-client
                         rpc/close! (fn [_] nil)
                         rpc/scan-all! (fn [client entity predicate _]
                                         {:rows (case entity
                                                  "@thread-a" [(t/triple entity predicate "Thread A")]
                                                  "@thread-b" [(t/triple entity predicate :not-a-string)]
                                                  [])})]
             (= {"thread-a" "Thread A"}
                (north.lanes-cli/native-resolve-titles 7977 ["thread-a" "thread-b" "thread-c"]))))

    (check "framrpc path degrades to no titles when connect fails"
           (with-redefs [rpc/connect (fn [& _] (throw (ex-info "coordinator unreachable" {})))]
             (= {} (north.lanes-cli/native-resolve-titles 7977 ["thread-a" "thread-b"]))))

    (check "framrpc path degrades a single failing thread without losing the rest"
           (with-redefs [rpc/connect stub-client
                         rpc/close! (fn [_] nil)
                         rpc/scan-all! (fn [_ entity _ _]
                                         (if (= entity "@thread-b")
                                           (throw (ex-info "read failed" {}))
                                           {:rows [(t/triple entity "title" "Thread A")]}))]
             (= {"thread-a" "Thread A"}
                (north.lanes-cli/native-resolve-titles 7977 ["thread-a" "thread-b"]))))

    (check "resolve-titles routes to the native path when framrpc-protocol? is true"
           (with-redefs [north.lanes-cli/framrpc-protocol? (fn [] true)
                         rpc/connect stub-client
                         rpc/close! (fn [_] nil)
                         rpc/scan-all! (fn [client entity _ _] {:rows [(t/triple entity "title" "Native title")]})
                         north.coord/resolved (fn [& _] (throw (ex-info "v03 path must not be called" {})))]
             (= {"thread-a" "Native title"}
                (north.lanes-cli/resolve-titles 7977 ["thread-a"]))))

    (check "resolve-titles routes to the v03 path when framrpc-protocol? is false"
           (with-redefs [north.lanes-cli/framrpc-protocol? (fn [] false)
                         north.coord/resolved (fn [_ _ _] "V03 title")
                         rpc/connect (fn [& _] (throw (ex-info "framrpc path must not be called" {})))]
             (= {"thread-a" "V03 title"}
                (north.lanes-cli/resolve-titles 7977 ["thread-a"])))))

  (finally
    (doseq [file (reverse (file-seq temp-dir))]
      (io/delete-file file true))))

(println (format "lanes cli: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
