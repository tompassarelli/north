#!/usr/bin/env bb
;; Regression for the subscription-entitlement cutover. Harness decisions and reports
;; use observed work facts; historical dollar facts may remain in a corpus but are inert.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (System/getenv "FRAM_PATH")
      "/home/tom/code/fram/main"))
(def runtime-classpath (str root "/out:" fram "/out"))
(cp/add-classpath runtime-classpath)
(load-file (str root "/cli/coord.clj"))
(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn read-source [path] (slurp (io/file root path)))

(let [listener (read-source "cli/north-listen.clj")
      predicates (read-source "cli/pred-cli.clj")
      presence (read-source "cli/presence-cli.clj")
      reconcile (read-source "cli/north-reconcile.clj")
      retired [(str "budget" "_total") (str "cost" "_usd") "NORTH_BUDGET" "BUDGET SPENT"]]
  (check "listener has no retired dollar gate and fails closed peer child operations"
         (and (not-any? #(str/includes? listener %) retired)
              (str/includes? listener "peer spawn is unsupported")
              (str/includes? listener "peer dispatch is unsupported")))
  (check "predicate registry omits retired policy facts"
         (and (not-any? #(str/includes? predicates %) retired)
              (str/includes? predicates "[\"tokens\"")))
  (check "presence has no standalone cost command and stamps run kind"
         (and (nil? (re-find #"(?m)^\s*\"cost\"" presence))
              (str/includes? presence "(put! port re \"kind\" \"run\")")))
  (check "reconciliation is usage-only and keeps exact operational columns"
         (and (not-any? #(str/includes? reconcile %) retired)
              (every? #(str/includes? reconcile %)
                      ["\"tokens\"" "\"duration_ms\"" "\"num_turns\"" "\"fallback_count\""
                       "\"usage_terminal_count\"" "\"usage_scope\"" "\"usage_total_status\""
                       "\"cached_input_tokens\"" "\"reasoning_output_tokens\""]))))

;; Exercise the report against a throwaway coordinator when Fram's compiled daemon is
;; available. The static checks above still run in source-only environments.
(when (.exists (io/file fram "out"))
  (defn port-free? [port]
    (try (with-open [s (java.net.Socket.)]
           (.connect s (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
           false)
         (catch Exception _ true)))
  (def port (or (some #(when (port-free? %) %) (range 7630 7650))
                (throw (ex-info "no free test port" {}))))
  (def tmp (.toFile (java.nio.file.Files/createTempDirectory
                      "north-subscription-policy" (make-array java.nio.file.attribute.FileAttribute 0))))
  (def log (io/file tmp "facts.framlog"))
  (def canonical-log (.getCanonicalPath log))
  (def daemon (proc/process {:dir fram :out :string :err :string
                             :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                                         "FRAM_SERVER_QUIET" "1"
                                         "FRAM_SERVER_XMX" "1g"}}
                            (str fram "/bin/fram-server") "serve" (str port)
                            canonical-log "north-coordination"))
  (defn await-up []
    (loop [n 0]
      (let [status (try (north.coord/status port) (catch Throwable _ nil))]
        (cond (and (= :ready (:state status))
                   (= "north-coordination" (:space-id status))) true
              (>= n 800) false
              :else (do (Thread/sleep 25) (recur (inc n)))))))
  (defn fact! [l p r]
    (let [result (north.coord/append! port l p r)]
      (when (:reject result)
        (throw (ex-info "fact write failed" result)))
      result))
  (try
    (let [started? (await-up)]
      (check "throwaway telemetry coordinator starts" started?)
      (when-not started?
        (throw (ex-info "throwaway Fram server did not start"
                        {:result (deref daemon 1000 nil)}))))
    (doseq [[p r] [["kind" "run"] ["agent" "worker-a"] ["tokens" "350"]
                   ["duration_ms" "1250"] ["num_turns" "3"] ["fallback_count" "1"]
                   ["fallback_path" "anthropic -> openai"]
                   ["provider" "openai"] ["model" "terra"] ["effort" "medium"]
                   ["at" "2026-07-16T00:00:00Z"]]]
      (fact! "@run-current" p r))
    (doseq [[p r] [["kind" "run"] ["agent" "worker-unknown"]
                   ["usage_terminal_count" "0"] ["usage_scope" "anthropic_result_terminal"]
                   ["usage_total_status" "unknown_no_terminal"]
                   ["provider" "anthropic"] ["model" "opus"] ["effort" "high"]
                   ["at" "2026-07-16T00:01:00Z"]]]
      (fact! "@run-unknown" p r))
    ;; A historical dollar-only row remains readable in the graph but is not a run
    ;; identity and therefore cannot enter the report or influence a decision.
    (fact! "@run-historical" (str "cost" "_usd") "99.99")
    (let [full (proc/shell {:out :string :err :string :continue true
                            :extra-env {"FRAM_LOG" canonical-log
                                        "FRAM_SPACE_ID" "north-coordination"
                                        "NORTH_TELEMETRY_PARTITION" "0"}}
                           "bb" "-cp" runtime-classpath
                           (str root "/cli/north-reconcile.clj") (str port) "full")
          recent (proc/shell {:out :string :err :string :continue true
                              :extra-env {"FRAM_LOG" canonical-log
                                          "FRAM_SPACE_ID" "north-coordination"
                                          "NORTH_TELEMETRY_PARTITION" "0"}}
                             "bb" "-cp" runtime-classpath
                             (str root "/cli/north-reconcile.clj") (str port) "recent" "10")]
      (when-not (and (zero? (:exit full)) (zero? (:exit recent)))
        (println "full reconciliation diagnostic:" (pr-str full))
        (println "recent reconciliation diagnostic:" (pr-str recent)))
      (check "usage reconciliation exits successfully" (and (zero? (:exit full)) (zero? (:exit recent))))
      (check "summary reports exact tokens, duration, turns, and fallbacks"
             (every? #(re-find % (:out full))
                     [#"total tokens\s+350\b" #"total duration ms\s+1250\b"
                      #"total turns\s+3\b" #"provider fallbacks\s+1\b"]))
      (check "unknown usage remains unreported rather than becoming a zero-token run"
             (and (re-find #"1/2 runs reported" (:out full))
                  (str/includes? (:out recent) "@run-unknown")))
      (check "recent report exposes provider/model/effort and ignores historical dollar row"
             (and (str/includes? (:out recent) "openai/terra/medium")
                  (str/includes? (:out recent) "1:anthropic -> openai")
                  (str/includes? (:out recent) "@run-current")
                  (not (str/includes? (:out recent) "@run-historical"))
                  (not (str/includes? (:out recent) "$")))))
    (finally
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))] (io/delete-file file true)))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nsubscription policy: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
