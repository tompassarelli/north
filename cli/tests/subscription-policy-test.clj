#!/usr/bin/env bb
;; Regression for the subscription-entitlement cutover. Harness decisions and reports
;; use observed work facts; historical dollar facts may remain in a corpus but are inert.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_PATH")
      "/home/tom/code/beagle/main/store"))
(def runtime-classpath (str root "/out:" store "/out"))
(cp/add-classpath runtime-classpath)
(load-file (str root "/cli/coord.clj"))
(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(when-not (.exists (io/file store "out"))
  (check (str "compiled Beagle Store test dependency is required at " store "/out") false))

;; Exercise the public report against a throwaway coordinator.
(when (.exists (io/file store "out"))
  (defn port-free? [port]
    (try (with-open [s (java.net.Socket.)]
           (.connect s (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
           false)
         (catch Exception _ true)))
  (def port (or (some #(when (port-free? %) %) (range 7630 7650))
                (throw (ex-info "no free test port" {}))))
  (def tmp (.toFile (java.nio.file.Files/createTempDirectory
                      "north-subscription-policy" (make-array java.nio.file.attribute.FileAttribute 0))))
  (def log (io/file tmp "facts.storelog"))
  (def canonical-log (.getCanonicalPath log))
  (def daemon (proc/process {:dir store :out :string :err :string
                                         "BEAGLE_STORE_SERVER_QUIET" "1"
                                         "BEAGLE_STORE_SERVER_XMX" "1g"}}
                            (str store "/bin/beagle-store-server") "serve" (str port)
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
        (throw (ex-info "throwaway Beagle Store server did not start"
                        {:result (deref daemon 1000 nil)}))))
    (let [runs
          {"@run-exact-anthropic"
           [["kind" "run"] ["agent" "worker-a"] ["tokens" "350"]
            ["lifetime_input_tokens" "200"] ["lifetime_output_tokens" "50"]
            ["lifetime_cache_read_tokens" "80"] ["lifetime_cache_write_tokens" "20"]
            ["lifetime_reasoning_tokens" "0"] ["model_call_count" "1"]
            ["usage_terminal_count" "1"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "exact"] ["duration_ms" "1250"]
            ["num_turns" "3"] ["fallback_count" "1"]
            ["fallback_path" "anthropic -> openai"] ["provider" "anthropic"]
            ["model_tier" "frontier"] ["effort" "high"]
            ["at" "2026-07-16T00:00:00Z"]]
           "@run-exact-openai"
           [["kind" "run"] ["agent" "worker-b"] ["tokens" "120"]
            ["lifetime_input_tokens" "100"] ["lifetime_output_tokens" "20"]
            ["lifetime_cache_read_tokens" "60"] ["lifetime_cache_write_tokens" "0"]
            ["lifetime_reasoning_tokens" "7"] ["model_call_count" "1"]
            ["usage_terminal_count" "1"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "exact"] ["duration_ms" "800"]
            ["provider_turn_units" "2"] ["provider_tool_items" "5"]
            ["provider_turn_metric_comparable" "false"]
            ["provider" "openai"] ["model_tier" "senior"] ["effort" "medium"]
            ["at" "2026-07-16T00:01:00Z"]]
           "@run-exact-zero"
           [["kind" "run"] ["agent" "worker-zero"] ["tokens" "0"]
            ["lifetime_input_tokens" "0"] ["lifetime_output_tokens" "0"]
            ["lifetime_cache_read_tokens" "0"] ["lifetime_cache_write_tokens" "0"]
            ["lifetime_reasoning_tokens" "0"] ["model_call_count" "1"]
            ["usage_terminal_count" "1"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "exact"] ["provider" "openai"]
            ["model_tier" "standard"] ["effort" "low"]
            ["at" "2026-07-16T00:02:00Z"]]
           "@run-partial"
           [["kind" "run"] ["agent" "worker-partial"]
            ["lifetime_input_tokens" "100"] ["lifetime_output_tokens" "20"]
            ["lifetime_cache_read_tokens" "60"] ["lifetime_cache_write_tokens" "0"]
            ["lifetime_reasoning_tokens" "7"] ["model_call_count" "1"]
            ["usage_terminal_count" "0"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "partial"] ["provider" "openai"]
            ["model_tier" "senior"] ["effort" "medium"]
            ["at" "2026-07-16T00:03:00Z"]]
           "@run-incomplete-terminal"
           [["kind" "run"] ["agent" "worker-incomplete"]
            ["lifetime_input_tokens" "0"] ["lifetime_output_tokens" "0"]
            ["lifetime_cache_read_tokens" "0"] ["lifetime_cache_write_tokens" "0"]
            ["lifetime_reasoning_tokens" "0"] ["model_call_count" "1"]
            ["usage_terminal_count" "1"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "unknown_incomplete_terminal"] ["provider" "openai"]
            ["model_tier" "senior"] ["effort" "medium"]
            ["at" "2026-07-16T00:04:00Z"]]
           "@run-no-terminal"
           [["kind" "run"] ["agent" "worker-no-terminal"]
            ["lifetime_input_tokens" "0"] ["lifetime_output_tokens" "0"]
            ["lifetime_cache_read_tokens" "0"] ["lifetime_cache_write_tokens" "0"]
            ["lifetime_reasoning_tokens" "0"] ["model_call_count" "1"]
            ["usage_terminal_count" "0"] ["usage_scope" "wire_run_cumulative"]
            ["usage_total_status" "unknown_no_terminal"] ["provider" "anthropic"]
            ["model_tier" "frontier"] ["effort" "high"]
            ["at" "2026-07-16T00:05:00Z"]]
           "@run-legacy-exact"
           [["kind" "run"] ["agent" "worker-historical"] ["tokens" "7"]
            ["model_tier" "standard"] ["at" "2026-07-16T00:06:00Z"]]
           "@run-exact-bigint"
           [["kind" "run"] ["agent" "worker-bigint"] ["tokens" "9007199254740992"]
            ["lifetime_input_tokens" "9007199254740991"]
            ["lifetime_output_tokens" "1"] ["lifetime_cache_read_tokens" "0"]
            ["lifetime_cache_write_tokens" "0"] ["lifetime_reasoning_tokens" "0"]
            ["model_call_count" "1"] ["usage_terminal_count" "1"]
            ["usage_scope" "wire_run_cumulative"] ["usage_total_status" "exact"]
            ["provider" "openai"] ["model_tier" "frontier"] ["effort" "high"]
            ["at" "2026-07-16T00:07:00Z"]]}]
      (doseq [[run facts] runs [p r] facts]
        (fact! run p r)))
    ;; A historical dollar-only row remains readable in the graph but is not a run
    ;; identity and therefore cannot enter the report or influence a decision.
    (fact! "@run-historical" (str "cost" "_usd") "99.99")
    (let [full (proc/shell {:out :string :err :string :continue true
                            :extra-env {"BEAGLE_STORE_LOG" canonical-log
                                        "BEAGLE_STORE_SPACE_ID" "north-coordination"
                                        "NORTH_TELEMETRY_PARTITION" "0"}}
                           "bb" "-cp" runtime-classpath
                           (str root "/cli/north-reconcile.clj") (str port) "full")
          recent (proc/shell {:out :string :err :string :continue true
                              :extra-env {"BEAGLE_STORE_LOG" canonical-log
                                          "BEAGLE_STORE_SPACE_ID" "north-coordination"
                                          "NORTH_TELEMETRY_PARTITION" "0"}}
                             "bb" "-cp" runtime-classpath
                             (str root "/cli/north-reconcile.clj") (str port) "recent" "10")]
      (when-not (and (zero? (:exit full)) (zero? (:exit recent)))
        (println "full reconciliation diagnostic:" (pr-str full))
        (println "recent reconciliation diagnostic:" (pr-str recent)))
      (check "usage reconciliation exits successfully" (and (zero? (:exit full)) (zero? (:exit recent))))
      (check "summary separates exact aggregate, partial lower bound, and both unknown states"
             (every? #(re-find % (:out full))
                     [#"exact token subtotal\s+9007199254741469\s+\(5/8 exact runs\)"
                      #"partial lower bound\s+>=120\s+\(1 partial runs"
                      #"unknown terminals\s+1\b" #"no usage terminal\s+1\b"
                      #"total turns\s+3\b" #"provider fallbacks\s+1\b"]))
      (check "recent report distinguishes exact zero, partial, incomplete terminal, and no terminal"
             (every? #(str/includes? (:out recent) %)
                     ["@run-exact-zero" ">=120 partial" "unknown-terminal" "no-terminal"
                      "2pt/5it"]))
      (check "report groups by semantic tier, keeps cache/reasoning subsets non-additive, and ignores dollar row"
             (and (str/includes? (:out full) "MODEL_TIER")
                  (str/includes? (:out recent) "openai/senior/medium")
                  (str/includes? (:out recent) "1:anthropic -> openai")
                  (str/includes? (:out recent) "@run-exact-openai")
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
