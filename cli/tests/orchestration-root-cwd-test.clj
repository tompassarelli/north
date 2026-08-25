;; orchestration-root-cwd-test.clj — pins the invariant behind thread
;; 019f9ac4-f49e-7912-8181-486449386ce5: the checkout north CLI must resolve
;; its portable package root from its explicit owner location, never the
;; caller's inherited process cwd.
;;
;; Regression: orchestration-project-cli.clj's orchestration-root() fell back
;; to (System/getProperty "user.dir") when no owner root was set. A `bb`
;; subprocess spawned by execFileSync (e.g.
;; sdk/src/orchestration-policy-pin.ts) inherits the CALLER's cwd, so
;; dispatching `north delegate` from any directory other than the north
;; checkout (e.g. ~/code/beagle/main/store) walked to a caller-owned scripts/
;; selection-assessment.mjs, which does not exist, and died at admission with
;; ERR_MODULE_NOT_FOUND before any provider call.
;;
;; Daemon-free: spawns a child `bb` process with :dir set to an UNRELATED cwd
;; (a fresh /tmp dir, never a checkout ancestor) and AGENT_MACHINERY_HOME
;; stripped from its env, then asserts the printed root resolves from HOME.
;;
;; Scoped to the main-guarded CLI libraries that are safe to load without a
;; coordinator. Other entry points execute their verb dispatch at load time.
;;
;;   bb cli/tests/orchestration-root-cwd-test.clj
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[babashka.process :as p]
         '[babashka.fs :as fs])

(def tests-dir (.getParentFile (io/file (System/getProperty "babashka.file"))))
(def cli-dir (.getParentFile tests-dir))
(def expected-orchestration
  (str (io/file (System/getenv "HOME") "code/agent-machinery/main")))

(def results (atom []))
(defn check [label pass?]
  (swap! results conj (boolean pass?))
  (println (format "  %s %s" (if pass? "✓" "✗") label)))

(println "orchestration-root cwd-independence — daemon-free")

;; An unrelated cwd: NOT a descendant of the north checkout, so a bare
;; user.dir fallback would resolve to a nonexistent <tmp>/orchestration.
(def unrelated-cwd (str (fs/create-temp-dir {:prefix "north-cwd-test-"})))

(defn probe [target-file root-form]
  (let [script (str "(load-file \"" target-file "\")"
                     "(println (" root-form "))")
        env (-> (into {} (System/getenv))
                (dissoc "NORTH_HOME" "AGENT_MACHINERY_HOME"))
        {:keys [exit out err]}
        (p/sh {:dir unrelated-cwd :env env :out :string :err :string}
              "bb" "--eval" script)]
    {:exit exit :out (str/trim out) :err err}))

;; --- orchestration-project-cli.clj ---
(let [{:keys [exit out err]} (probe (str (io/file cli-dir "orchestration-project-cli.clj"))
                                    "orchestration-root")]
  (check (str "orchestration-project-cli.clj: exits 0 from unrelated cwd (stderr: " err ")")
         (zero? exit))
  (check (str "orchestration-project-cli.clj: resolves from the package owner, not " unrelated-cwd
              " (got " out ")")
         (= expected-orchestration out)))

(fs/delete-tree unrelated-cwd)

(let [rs @results passed (count (filter true? rs))]
  (println (format "\n%d/%d cwd-independence checks passed" passed (count rs)))
  (System/exit (if (every? true? rs) 0 1)))
