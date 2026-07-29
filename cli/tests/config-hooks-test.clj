#!/usr/bin/env bb
;; End-to-end contract for `north config hooks`: the report and a real hook
;; must resolve the same scratch harness state.
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def registry (str root "/profiles/tom/hooks/registry.tsv"))
(def tmp-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-config-hooks-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def scratch-home (str tmp-dir "/home"))
(def state (str tmp-dir "/harness.conf"))
(def checks (atom []))

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn run-cli-with [registry-path & args]
  (apply p/shell
         {:out :string
          :err :string
          :continue true
          :extra-env {"HOME" scratch-home
                      "NORTH_HOME" root
                      "NORTH_HARNESS_STATE" state
                      "NORTH_HOOK_REGISTRY" registry-path}}
         (into ["bb" cli "hooks"] args)))

(defn run-cli [& args]
  (apply run-cli-with registry args))

(defn stored [key]
  (let [prefix (str key "=")]
    (some->> (when (.isFile (io/file state)) (slurp state))
             str/split-lines
             (filter #(str/starts-with? % prefix))
             last
             (#(subs % (count prefix))))))

(try
  (.mkdirs (io/file scratch-home))

  (let [listed (run-cli)]
    (check "list succeeds" (zero? (:exit listed)))
    (check "list renders every registered hook"
           (every? #(str/includes? (:out listed) %)
                   ["code-upstream-guard" "firn-guard"
                    "launch-critical-worktree-guard" "git-blind-stage-guard"
                    "tripwire-guard" "agent-spawn-guard" "north-clock-guard"
                    "racket-build-guard" "logcompress-hook"
                    "beagle-session-start" "north-session-end" "hook-detach"]))
    (check "list reports executable paths and decision provenance"
           (and (str/includes? (:out listed) "EXEC")
                (str/includes? (:out listed) "default"))))

  (let [explained (run-cli "explain" "tripwire-guard")]
    (check "explain succeeds" (zero? (:exit explained)))
    (check "explain renders the complete precedence trace"
           (every? #(str/includes? (:out explained) %)
                   ["item" "category" "all" "default" "effective"])))

  (let [bad (run-cli "off" "agent-spawn-guard" "--until" "tomorrow")]
    (check "invalid deny-hook TTL is rejected" (not (zero? (:exit bad))))
    (check "invalid TTL does not write state" (nil? (stored "hooks.hook.agent-spawn-guard"))))

  (let [defaulted (run-cli "off" "code-upstream-guard")]
    (check "deny hook disable defaults to a TTL" (zero? (:exit defaulted)))
    (check "default deny-hook TTL is stored canonically"
           (boolean
            (re-matches #"off:until=\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z"
                        (or (stored "hooks.hook.code-upstream-guard") "")))))

  (let [disabled (run-cli "off" "agent-spawn-guard" "--until" "2099-01-01T00:00:00Z")]
    (check "deny hook disable succeeds with TTL" (zero? (:exit disabled)))
    (check "deny hook stores the exact TTL"
           (= "off:until=2099-01-01T00:00:00Z"
              (stored "hooks.hook.agent-spawn-guard"))))

  ;; Discriminating runtime seam: this payload is denied while the dispatch
  ;; hook is live. Its item dial must silence exactly that hook.
  (let [payload "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"north spawn implementer work\"}}"
        guarded (p/shell
                 {:in payload :out :string :err :string :continue true
                  :extra-env {"HOME" scratch-home
                              "NORTH_HOME" root
                              "NORTH_HARNESS_STATE" state
                              "NORTH_HOOK_REGISTRY" registry
                              "AGENT_TOPOLOGY" "worker"}}
                 (str root "/profiles/tom/hooks/agent-spawn-guard.sh"))]
    (check "item-off makes the selected deny hook a silent allow"
           (and (zero? (:exit guarded)) (str/blank? (:out guarded)))))

  (let [payload "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"rm -rf /home/tom\"},\"cwd\":\"/tmp\"}"
        other (p/shell
               {:in payload :out :string :err :string :continue true
                :extra-env {"HOME" scratch-home
                            "NORTH_HOME" root
                            "NORTH_BIN" "/bin/true"
                            "NORTH_HARNESS_STATE" state
                            "NORTH_HOOK_REGISTRY" registry
                            "TRIPWIRE_LOG_DIR" (str tmp-dir "/tripwire")}}
               (str root "/profiles/tom/hooks/tripwire-guard.sh"))]
    (check "item-off leaves another deny hook live"
           (and (not (zero? (:exit other)))
                (not (str/blank? (str (:out other) (:err other)))))))

  (let [advisory (run-cli "off" "logcompress-hook")]
    (check "advisory hook may be disabled without TTL" (zero? (:exit advisory)))
    (check "advisory hook stores a permanent off"
           (= "off" (stored "hooks.hook.logcompress-hook"))))

  (let [category (run-cli "category" "off" "authoring" "--until" "2099-01-01T00:00:00Z")]
    (check "deny-bearing category disable succeeds with TTL" (zero? (:exit category)))
    (check "authoring category reuses the guards key"
           (= "off:until=2099-01-01T00:00:00Z" (stored "guards"))))

  (let [all-off (run-cli "all" "off" "--until" "2099-01-01T00:00:00Z")]
    (check "global disable succeeds with TTL" (zero? (:exit all-off)))
    (check "global disable stores the exact TTL"
           (= "off:until=2099-01-01T00:00:00Z" (stored "hooks"))))

  (let [listed (run-cli)]
    (check "global sweep disables an ordinary hook"
           (boolean (re-find #"(?m)^north-clock-guard\s+billing\s+deny\s+off\s+all\b"
                             (:out listed))))
    (check "global sweep excludes coordination hooks"
           (boolean (re-find #"(?m)^north-session-end\s+coordination\s+identity\s+on\s+default\b"
                             (:out listed)))))

  (let [item-on (run-cli "on" "tripwire-guard")
        listed (run-cli)]
    (check "item on succeeds over broader off" (zero? (:exit item-on)))
    (check "item on wins and reports item provenance"
           (boolean (re-find #"(?m)^tripwire-guard\s+authoring\s+deny\s+on\s+item\b"
                             (:out listed)))))

  (let [missing-registry (str tmp-dir "/missing-registry.tsv")]
    (spit missing-registry
          (str "id\tcategory\tkind\tin_all\tttl_req\tpath\tevents\n"
               "missing-hook\tauthoring\tdeny\tyes\tyes\tmissing.sh\tPreToolUse:Bash\n"))
    (let [listed (run-cli-with missing-registry)]
      (check "missing registry path still lists" (zero? (:exit listed)))
      (check "missing executable is reported MISSING"
             (str/includes? (:out listed) "MISSING"))))

  (finally
    (doseq [f (reverse (file-seq tmp-dir))]
      (io/delete-file f true))))

(let [results @checks
      pass (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig hooks: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
