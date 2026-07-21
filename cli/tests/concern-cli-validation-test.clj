#!/usr/bin/env bb
;; Malformed concern maturity commands must be usage errors and must not publish
;; even a partial fact to the coordinator.
(require '[clojure.edn :as edn] '[clojure.java.io :as io]
         '[cheshire.core :as json]
         '[babashka.process :as p])

(def root (-> (io/file (System/getProperty "babashka.file"))
              .getParentFile .getParentFile .getParentFile .getPath))
(def fram "/home/tom/code/fram")
(defn port-free? [port]
  (try
    (with-open [s (java.net.Socket.)]
      (.connect s (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      false)
    (catch Exception _ true)))
(def port (or (some #(when (port-free? %) %) [7630 7631 7632])
              (throw (ex-info "no test port available" {}))))
(def log (.getCanonicalPath
          (io/file (System/getProperty "java.io.tmpdir")
                   (str "concern-cli-validation-" (System/nanoTime) ".log"))))
(spit log "")
(def daemon
  (p/process {:dir fram :out :string :err :string
              :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
             "bb" "-cp" "out" "coord_daemon.clj" "serve-flat" (str port) log))
(defn cleanup []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (.delete (io/file log)))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))
(defn await-up []
  (loop [n 0]
    (cond
      (not (port-free? port)) true
      (>= n 100) false
      :else (do (Thread/sleep 50) (recur (inc n))))))
(when-not (await-up)
  (binding [*out* *err*]
    (println "test coordinator failed to start"))
  (cleanup)
  (System/exit 1))

(defn op [request]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w
              (.getBytes
               (str (pr-str {:op :for-log
                             :expected-log log
                             :request request})
                    "\n")))
      (.flush w)
      (edn/read-string (.readLine r)))))
(defn run-concern [& args]
  @(apply p/process {:dir root :out :string :err :string
                     :extra-env {"FRAM_LOG" log}}
          "bb" "cli/concern-cli.clj" (str port) args))
(defn reached-rows []
  (:ok (op {:op :query
            :query {:find "row"
                    :rules [{:head {:rel "row"
                                    :args [{:var "e"} {:var "r"}]}
                             :body [{:rel "triple"
                                     :args [{:var "e"} "reached" {:var "r"}]}]}]}})))

(def fails (atom 0))
(defn check [label ok?]
  (println (str "  " (if ok? "PASS" "FAIL") " — " label))
  (when-not ok? (swap! fails inc)))

(def declared (run-concern "declare" "validation-agent" "/tmp"
                           "validation fixture" "src/example.clj"))
(def cid (second (re-find #"(concern-\d+-[a-f0-9]+)" (:out declared))))
(check "fixture concern declares successfully" (and (zero? (:exit declared)) cid))
(def before (set (reached-rows)))
(doseq [[label argv]
        [["status without arguments" ["status"]]
         ["status without maturity" ["status" cid]]
         ["status with unknown maturity" ["status" cid "almost-done"]]
         ["done without id" ["done"]]
         ["done for unknown concern" ["done" "concern-9999999999999-dead"]]]]
  (let [result (apply run-concern argv)]
    (check (str label " exits 2") (= 2 (:exit result)))))
(check "malformed commands publish no reached facts" (= before (set (reached-rows))))
(def valid (run-concern "status" cid "likely-to-land"))
(check "valid status still succeeds" (zero? (:exit valid)))
(check "valid status publishes its maturity"
       (contains? (set (reached-rows)) [(str "@" cid) "likely-to-land"]))

;; Listing cost is bounded by predicate count, not historical concern count.
;; Seed enough rows that the former seven-reads-per-concern implementation
;; exceeded the UI budget, then require the batched live view to return quickly.
(doseq [index (range 250)
        :let [id (str "@concern-1700000000000-bulk" index)]
        [predicate value] [["kind" "concern"] ["reached" "building"]]]
  (op {:op :assert :te id :p predicate :r value}))
(let [proc (p/process {:dir root :out :string :err :string
                       :extra-env {"FRAM_LOG" log}}
                      "bb" "cli/concern-cli.clj" (str port) "ls")
      started (System/nanoTime)
      result (deref proc 2000 ::timeout)
      elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
  (when (= result ::timeout) (p/destroy-tree proc))
  (check "concern ls is history-size bounded and returns within 2s"
         (and (not= result ::timeout)
              (zero? (:exit result))
              (re-find #"ACTIVE CONCERNS — 251" (:out result))
              (< elapsed-ms 2000))))

;; Strict versioned MACHINE projection: `list-json` enumerates structured rows so a
;; consumer never scrapes rendered text. Every row carries a closed-set liveness
;; class + an explicit retired boolean; the lapsed likely-to-land fixture (cid, set
;; to likely-to-land above, owner holds no live lease) is ORPHANED, never HANDOFF.
(let [proj    (run-concern "list-json")
      parsed  (try (json/parse-string (:out proj) true) (catch Exception _ nil))
      rows    (:concerns parsed)
      by-id   (into {} (map (juxt :id identity)) rows)
      classes #{"live" "stale" "orphaned" "retired"}
      cid-row (get by-id (str "@" cid))]
  (check "list-json exits 0" (zero? (:exit proj)))
  (check "list-json is a version-1 document" (= 1 (:version parsed)))
  (check "list-json concerns is a vector" (vector? rows))
  (check "every projection row carries a closed-set liveness class"
         (and (seq rows) (every? #(contains? classes (:classification %)) rows)))
  (check "every projection row carries an explicit retired boolean"
         (every? #(contains? #{true false} (:retired %)) rows))
  (check "lapsed likely-to-land fixture is classified ORPHANED, not handoff"
         (and cid-row
              (= "likely-to-land" (:maturity cid-row))
              (= "orphaned" (:classification cid-row))
              (false? (:retired cid-row))))
  (check "list-json emits no HANDOFF label"
         (not (re-find #"(?i)handoff" (:out proj)))))

(cleanup)
(if (zero? @fails)
  (do (println "\nconcern CLI validation: ALL PASS") (System/exit 0))
  (do (println (str "\nconcern CLI validation: " @fails " FAIL")) (System/exit 1)))
