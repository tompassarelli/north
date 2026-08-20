#!/usr/bin/env bb
;; The terminal transition's read phase is guarded by a global served version, so a
;; read that grows with concern count loses the CAS under ordinary traffic.
;;   bb cli/tests/concern-terminal-cas-test.clj
(require '[babashka.classpath :as cp]
         '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root
  (-> (io/file test-script) .getParentFile .getParentFile .getParentFile
      .getCanonicalPath))
(def seed-count
  (or (some-> (System/getenv "NORTH_CAS_TEST_CONCERNS") parse-long) 300))
;; Slow enough that a millisecond-scale read phase can win a window on a loaded
;; runner; a read that grows with the corpus blows the 30s deadline regardless.
(def write-period-ms 1000)

(def checks (atom []))
(defn check
  ([label value] (check label value nil))
  ([label value detail]
   (let [passed (boolean value)]
     (swap! checks conj [label passed])
     (println (str (if passed "PASS" "FAIL") " — " label))
     (when (and (not passed) detail) (println (str "       " (pr-str detail)))))))

(defn port-free? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      false)
    (catch Exception _ true)))

(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_PATH")
      (.getCanonicalPath
       (io/file (System/getProperty "user.home") "code" "store" "main"))))
(def runtime-classpath (str root "/out:" store "/out"))
(cp/add-classpath runtime-classpath)
(load-file (str root "/cli/coord.clj"))
;; A silent skip in CI is a gate that can never fail; CI must set BEAGLE_STORE_TEST_CHECKOUT
;; (or BEAGLE_STORE_PATH) so this always runs there — an absent Beagle Store under CI is a hard error.
(when-not (.isDirectory (io/file store "out"))
  (if (System/getenv "CI")
    (do (println "FAIL — compiled Beagle Store out/ is absent under CI (BEAGLE_STORE_TEST_CHECKOUT/BEAGLE_STORE_PATH unset or wrong)")
        (System/exit 1))
    (do (println "SKIP — compiled Beagle Store out/ is absent")
        (System/exit 0))))

(def port
  (or (some #(when (port-free? %) %) (range 7690 7740))
      (throw (ex-info "no test port available" {}))))
(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-concern-cas" (make-array java.nio.file.attribute.FileAttribute 0))))
(def log (io/file tmp "facts.storelog"))
(def canonical-log (.getCanonicalPath log))
(def isolated-env
  {"BEAGLE_STORE_LOG" canonical-log
   "BEAGLE_STORE_SPACE_ID" "north-coordination"
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})
(def daemon
  (p/process {:dir store :out :string :err :string
              :extra-env (assoc isolated-env
                                "BEAGLE_STORE_SERVER_QUIET" "1"
                                "BEAGLE_STORE_SERVER_XMX" "1g")}
             (str store "/bin/beagle-store-server") "serve" (str port)
             canonical-log "north-coordination"))

(defn cleanup []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (doseq [file (reverse (file-seq tmp))] (io/delete-file file true)))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))

(defn fact! [subject predicate object]
  (let [result (north.coord/append! port subject predicate object)]
    (when (:reject result) (throw (ex-info "fixture fact write failed" result)))
    result))

(when-not (loop [attempt 0]
            (let [status (try (north.coord/status port) (catch Throwable _ nil))]
              (cond (and (= :ready (:state status))
                         (= "north-coordination" (:space-id status))) true
                    (>= attempt 800) false
                    :else (do (Thread/sleep 25) (recur (inc attempt))))))
  (throw (ex-info "throwaway coordinator failed to start"
                  {:err (:err (deref daemon 2000 nil))})))

(def base (System/currentTimeMillis))
(defn seed-concern! [id agent touches]
  (fact! id "kind" "concern")
  (fact! id "agent" (str "@" agent))
  (fact! id "repo" "probe")
  (fact! id "intent" (str "seeded " agent))
  (fact! id "reached" "building")
  (doseq [t touches] (fact! id "touches" t)))

(doseq [i (range seed-count)]
  (seed-concern! (str "@concern-" (+ base i) "-f" (format "%04x" i))
                 (str "filler-" i)
                 [(str "src/file-" i ".clj")]))
(def peer (str "@concern-" (+ base seed-count) "-peer"))
(def subject (str "@concern-" (+ base seed-count 1) "-cas"))
(seed-concern! peer "peer-agent" ["src/shared.clj"])
(seed-concern! subject "cas-agent" ["src/shared.clj"])

;; Concurrent coordination traffic: every write invalidates a global base.
(def writing? (atom true))
(def writes (atom 0))
(def writer
  (future
    (loop [i 0]
      (when @writing?
        (try (north.coord/append! port "@cmd:cas-test-writer" "probe_note" (str i))
             (swap! writes inc)
             (catch Throwable _ nil))
        (Thread/sleep write-period-ms)
        (recur (inc i))))))
(Thread/sleep 500)

(def transition
  (let [started (System/nanoTime)
        result @(p/process {:dir root :out :string :err :string
                            :extra-env isolated-env}
                           "bb" "-cp" runtime-classpath
                           "cli/concern-cli.clj" (str port) "done" subject)]
    (assoc result :ms (long (/ (- (System/nanoTime) started) 1000000)))))
(def writes-during (deref writes))
(reset! writing? false)
@writer

(defn reached [concern]
  (->> (north.coord/query-rows
        port
        {:find "value"
         :rules [{:head {:rel "value" :args [{:var "value"}]}
                  :body [{:rel "triple"
                          :args [concern "reached" {:var "value"}]}]}]})
       (map first)
       set))

(check "terminal transition commits under concurrent coordination writes"
       (zero? (:exit transition))
       (select-keys transition [:exit :ms :err]))
(check "the concern durably reached landed"
       (contains? (reached subject) "landed")
       (reached subject))
(check "the probe actually held the coordinator under write load"
       (pos? writes-during)
       {:writes writes-during :elapsed-ms (:ms transition)})

(println (str "\nconcerns=" (+ seed-count 2)
              " transition_ms=" (:ms transition)
              " concurrent_writes=" writes-during))
(let [results @checks
      passed (count (filter second results))]
  (println (format "concern terminal CAS: %d / %d PASS" passed (count results)))
  (cleanup)
  (System/exit (if (= passed (count results)) 0 1)))
