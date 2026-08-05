#!/usr/bin/env bb
;; Malformed concern maturity commands must be usage errors and must not publish
;; even a partial fact to the coordinator.
(require '[babashka.classpath :as cp] '[clojure.java.io :as io]
         '[clojure.java.shell :as shell] '[clojure.string :as str]
         '[cheshire.core :as json]
         '[babashka.process :as p])

(def root (-> (io/file (System/getProperty "babashka.file"))
              .getParentFile .getParentFile .getParentFile .getPath))
(def fram
  (or (System/getenv "FRAM_PATH")
      "/home/tom/code/fram/main"))
(def runtime-classpath (str root "/out:" fram "/out"))
(cp/add-classpath runtime-classpath)
(load-file (str root "/cli/coord.clj"))
(defn port-free? [port]
  (try
    (with-open [s (java.net.Socket.)]
      (.connect s (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      false)
    (catch Exception _ true)))
(def port (or (some #(when (port-free? %) %) [7630 7631 7632])
              (throw (ex-info "no test port available" {}))))
(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-concern-cli-validation"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def log (.getCanonicalPath (io/file tmp "facts.framlog")))
(def telemetry-log (.getCanonicalPath (io/file tmp "telemetry.framlog")))
(def candidate-repo (.getCanonicalPath (io/file tmp "candidate-repo")))
(doseq [result
        [(shell/sh "git" "init" "-q" "-b" "feature" candidate-repo)
         (shell/sh "git" "-C" candidate-repo
                   "-c" "user.name=North Test"
                   "-c" "user.email=north-test@example.invalid"
                   "commit" "-q" "--allow-empty" "-m" "candidate fixture")]]
  (when-not (zero? (:exit result))
    (throw (ex-info "candidate Git fixture failed" {:result result}))))
(def isolated-env
  {"FRAM_LOG" log
   "FRAM_SPACE_ID" "north-coordination"
   "FRAM_TELEMETRY_LOG" telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})
(def daemon
  (p/process {:dir fram :out :string :err :string
              :extra-env (assoc isolated-env
                                "FRAM_SERVER_RUNTIME" "jvm-dev"
                                "FRAM_SERVER_QUIET" "1"
                                "FRAM_SERVER_XMX" "1g")}
             (str fram "/bin/fram-server") "serve" (str port)
             log "north-coordination"))
(defn cleanup []
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (doseq [file (reverse (file-seq tmp))]
    (io/delete-file file true)))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))
(defn await-up []
  (loop [n 0]
    (let [status (try (north.coord/status port) (catch Throwable _ nil))]
      (cond
        (and (= :ready (:state status))
             (= "north-coordination" (:space-id status))) true
        (>= n 800) false
        :else (do (Thread/sleep 25) (recur (inc n)))))))
(when-not (await-up)
  (try (p/destroy-tree daemon) (catch Throwable _ nil))
  (let [result (deref daemon 5000 nil)]
    (binding [*out* *err*]
      (println "test coordinator failed to start")
      (println "exit:" (:exit result))
      (println "stdout:" (or (:out result) "<unavailable>"))
      (println "stderr:" (or (:err result) "<unavailable>"))))
  (cleanup)
  (System/exit 1))

(defn run-concern-in [directory & args]
  @(apply p/process {:dir directory :out :string :err :string
                     :extra-env isolated-env}
          "bb" "-cp" runtime-classpath
          (str root "/cli/concern-cli.clj") (str port) args))
(defn run-concern [& args]
  (apply run-concern-in root args))
(defn reached-rows []
  (north.coord/query-rows
   port
   {:find "row"
    :rules [{:head {:rel "row"
                    :args [{:var "e"} {:var "r"}]}
             :body [{:rel "triple"
                     :args [{:var "e"} "reached" {:var "r"}]}]}]}))
(defn values-of [subject predicate]
  (set (north.coord/many port subject predicate)))

(defn fact! [subject predicate value]
  (north.coord/append! port subject predicate value))

(def fails (atom 0))
(defn check [label ok?]
  (println (str "  " (if ok? "PASS" "FAIL") " — " label))
  (when-not ok? (swap! fails inc)))

(def declared (run-concern "declare" "validation-agent" "/tmp"
                           "validation fixture" "src/example.clj"))
(def cid (second (re-find #"(concern-\d+-[a-f0-9]+)" (:out declared))))
(check "fixture concern declares successfully" (and (zero? (:exit declared)) cid))
(check "bare concern owner is stored as one canonical principal ref"
       (= #{"@validation-agent"}
          (values-of (str "@" cid) "agent")))
(def ref-declared
  (run-concern "declare" "@validation-ref-owner" "/tmp"
               "ref owner fixture" "src/ref-example.clj"))
(def ref-cid
  (second (re-find #"(concern-\d+-[a-f0-9]+)" (:out ref-declared))))
(check "already-referenced concern owner is not double-prefixed"
       (and
        (zero? (:exit ref-declared))
        ref-cid
        (= #{"@validation-ref-owner"}
           (values-of (str "@" ref-cid) "agent"))))
(def before (set (reached-rows)))
(doseq [[label argv]
        [["status without arguments" ["status"]]
         ["status without maturity" ["status" cid]]
         ["status with unknown maturity" ["status" cid "almost-done"]]
         ["candidate without id" ["candidate"]]
         ["candidate with unknown revision" ["candidate" cid "not-a-real-revision"]]
         ["done without id" ["done"]]
         ["done for unknown concern" ["done" "concern-9999999999999-dead"]]]]
  (let [result (apply run-concern argv)]
    (check (str label " exits 2") (= 2 (:exit result)))))
(check "malformed commands publish no reached facts" (= before (set (reached-rows))))
(def valid (run-concern-in candidate-repo "status" cid "likely-to-land"))
(def expected-candidate
  (str/trim (:out (shell/sh "git" "-C" candidate-repo "rev-parse" "HEAD"))))
(def expected-git-dir
  (str/trim
   (:out
    (shell/sh "git" "-C" candidate-repo "rev-parse"
              "--path-format=absolute" "--git-common-dir"))))
(check "valid status still succeeds" (zero? (:exit valid)))
(check "valid status publishes its maturity"
       (contains? (set (reached-rows)) [(str "@" cid) "likely-to-land"]))
(check "likely-to-land records the exact current commit"
       (= #{expected-candidate}
          (values-of (str "@" cid) "candidate_rev")))
(check "candidate stores the durable Git identity used for landing derivation"
       (= #{expected-git-dir}
          (values-of (str "@" cid) "candidate_git_dir")))
(check "status output names the exact candidate commit"
       (str/includes? (:out valid) expected-candidate))
(let [explicit (run-concern-in candidate-repo "candidate" cid expected-candidate)]
  (check "explicit candidate command is idempotent and actionable"
         (and (zero? (:exit explicit))
              (= #{expected-candidate}
                 (values-of (str "@" cid) "candidate_rev"))
              (str/includes? (:out explicit) expected-candidate))))

;; Listing cost is bounded by predicate count, not historical concern count.
;; Seed enough rows to exercise the batched live view within the UI budget.
(doseq [index (range 250)
        :let [id (str "@concern-1700000000000-bulk" index)]
        [predicate value] [["kind" "concern"] ["reached" "building"]]]
  (fact! id predicate value))
(let [proc (p/process {:dir root :out :string :err :string
                       :extra-env isolated-env}
                      "bb" "-cp" runtime-classpath
                      "cli/concern-cli.clj" (str port) "ls")
      started (System/nanoTime)
      result (deref proc 2000 ::timeout)
      elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
  (when (= result ::timeout) (p/destroy-tree proc))
  (check "concern ls is history-size bounded and returns within 2s"
         (and (not= result ::timeout)
              (zero? (:exit result))
              (re-find #"ACTIVE CONCERNS — 252" (:out result))
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

(let [landed (shell/sh "git" "-C" candidate-repo
                       "branch" "main" expected-candidate)
      projection (run-concern "list-json")
      rows (:concerns (json/parse-string (:out projection) true))]
  (check "candidate landing is derived from Git main without a done fact"
         (and (zero? (:exit landed))
              (zero? (:exit projection))
              (not-any? #(= (str "@" cid) (:id %)) rows)
              (not (contains? (set (reached-rows))
                              [(str "@" cid) "landed"])))))

(cleanup)
(if (zero? @fails)
  (do (println "\nconcern CLI validation: ALL PASS") (System/exit 0))
  (do (println (str "\nconcern CLI validation: " @fails " FAIL")) (System/exit 1)))
