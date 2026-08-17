#!/usr/bin/env bb
;; Production worktree-janitor regression. A throwaway Git repository and a
;; separately fenced Beagle Store coordinator exercise the scheduled worktree task
;; twice; no janitor function is called directly.
(require '[babashka.fs :as fs]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram-source
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_PATH")
      "/home/tom/code/beagle/main/store"))
(def fram
  (.getCanonicalPath (io/file fram-source)))
(when-not (.isFile (io/file fram "bin/beagle-store-server"))
  (throw (ex-info "current Beagle store engine is required" {:fram fram})))
(def maintenance-host (str root "/cli/coordination-maintenance-task-host.clj"))
(def lander (str root "/cli/worktree-lander.clj"))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/terminal-projection.clj"))

(def checks (atom []))
(def test-log (atom nil))
(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(let [source (slurp maintenance-host)]
  (check "sweep lifecycle lookup is indexed, capped, and never scans all subject facts"
         (and (str/includes? source "north.coord/bounded-query-in-domain")
              (str/includes? source "lane_run_candidate")
              (str/includes? source "north.coord/many port subject predicate")
              (not (str/includes? source "north.coord/query-page"))
              (not (str/includes? source ":find \"terminal_fact\"")))))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))

(defn await-up [port]
  (loop [attempt 0]
    (let [ready? (try
                   (= :ready (:state (north.coord/status port)))
                   (catch Exception _ false))]
      (cond
        ready? true
        (>= attempt 1500) false
        :else (do (Thread/sleep 50) (recur (inc attempt)))))))

(defn assert-fact! [port subject predicate value]
  (north.coord/append! port subject predicate value))

(defn many [port subject predicate]
  (north.coord/many port subject predicate))

(defn run-git [& args]
  (apply proc/shell {:out :string :err :string :continue true} "git" args))

(defn git! [& args]
  (let [result (apply run-git args)]
    (when-not (zero? (:exit result))
      (throw (ex-info "fixture git command failed"
                      {:args args :exit (:exit result) :err (:err result)})))
    (str/trim (str (:out result)))))

(defn branch-present? [repo branch]
  (zero? (:exit (run-git "-C" repo "show-ref" "--verify" "--quiet"
                         (str "refs/heads/" branch)))))

(defn sha256-file [^java.io.File file]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (.update digest (java.nio.file.Files/readAllBytes (.toPath file)))
    (format "%064x" (java.math.BigInteger. 1 (.digest digest)))))

(defn tree-snapshot [path]
  (let [root-file (io/file path)
        root-path (.toPath root-file)]
    (into (sorted-map)
          (for [^java.io.File file (file-seq root-file)
                :when (.isFile file)]
            [(str (.relativize root-path (.toPath file))) (sha256-file file)]))))

(defn create-worktree! [repo parent handle]
  (let [branch (str "lane-" handle)
        path (.getCanonicalPath (io/file parent (str handle " tree")))]
    (git! "-C" repo "worktree" "add" "-q" "-b" branch path "HEAD")
    {:handle handle :branch branch :path path :subject (str "@agent:" handle)}))

(defn managed-clone-path [repo handle]
  ;; Match sdk/src/worktree.ts: worktreePath = /tmp/<repo-tag>-lane-<id>, where
  ;; the tag is the CONTAINER name for a `main/` checkout (AMB-8).
  (let [file (io/file repo)
        base (.getName file)
        tag (if (= "main" base)
              (or (some-> (.getParentFile file) .getName not-empty) base)
              base)]
    (.getCanonicalPath (io/file "/tmp" (str tag "-lane-" handle)))))

(defn create-clone! [repo parent handle]
  (let [branch (str "lane-" handle)
        path (managed-clone-path repo handle)]
    (git! "clone" "-q" "--no-hardlinks" repo path)
    ;; A clone inherits no committer identity; CI runners have none globally.
    (git! "-C" path "config" "user.email" "janitor@example.invalid")
    (git! "-C" path "config" "user.name" "Janitor Test")
    (git! "-C" path "checkout" "-qb" branch "HEAD")
    (git! "-C" path "remote" "set-url" "--push" "origin"
          "north-disabled://managed-clone-no-push")
    {:handle handle :branch branch :path path :subject (str "@agent:" handle)}))

(defn commit-file! [repo filename text message]
  (spit (io/file repo filename) text)
  (git! "-C" repo "add" filename)
  (git! "-C" repo "commit" "-qm" message))

(defn harvest-clone! [repo clone]
  (git! "-C" repo "fetch" "--no-tags" "--no-write-fetch-head" "--"
        (:path clone) (str "refs/heads/" (:branch clone) ":refs/heads/" (:branch clone))))

(defn register-lane! [port repo {:keys [subject branch path]} graph-branch]
  (doseq [[predicate value]
          [["kind" "lane"] ["repo" repo] ["worktree" path]
           ["branch" (or graph-branch branch)]]]
    (assert-fact! port subject predicate value)))

(defn commit-run! [port handle]
  (let [run (str "@run:" handle)]
    (assert-fact! port run "agent" handle)
    (assert-fact! port run "at" "2026-07-20T09:00:00Z")
    (assert-fact! port run "process_outcome" "ran")
    ;; Last-write commit marker: without this exact fact the run is invisible.
    (assert-fact! port run "kind" "run")))

(defn commit-modern-terminal! [port subject]
  (let [facts {"process_outcome" #{"ran"}
               "delivery_outcome" #{"unverified"}
               "delivery_reason" #{"fixture_terminal_without_delivery_proof"}}
        marker (north.terminal-projection/terminal-manifest-sha256 facts)]
    (doseq [[predicate values] facts
            value values]
      (assert-fact! port subject predicate value))
    ;; Digest is the lane terminal's last-write commit marker.
    (assert-fact! port subject "terminal_manifest_sha256" marker)))

(defn run-maintenance [port environment & flags]
  (apply proc/shell
         {:out :string :err :string :continue true
          :extra-env (merge environment
                            {"BEAGLE_STORE_PORT" (str port)
                             "BEAGLE_STORE_LOG" @test-log
                             "NORTH_TELEMETRY_PARTITION" "0"
                             "BEAGLE_STORE_TELEMETRY_LOG" ""})}
         "bb" maintenance-host "worktrees" flags))

;; ---- unregistered <container>/worktrees/ census fixture ---------------------
;; Container layout (<root>/<repo>/main + worktrees/<slug> lanes, and a pins/<full-object-id>
;; checkout that must never be enumerated) with dated commits and back-dated
;; activity traces, so staleness is real rather than simulated.

;; Four days before now: past the 48h horizon by a margin no clock skew closes.
(def aged-date
  (.format (.minusDays (java.time.ZonedDateTime/now) 4)
           java.time.format.DateTimeFormatter/ISO_OFFSET_DATE_TIME))

(defn git-dated! [when & args]
  (let [result (apply proc/shell
                      {:out :string :err :string :continue true
                       :extra-env {"GIT_AUTHOR_DATE" when "GIT_COMMITTER_DATE" when}}
                      "git" args)]
    (when-not (zero? (:exit result))
      (throw (ex-info "fixture dated git command failed"
                      {:args args :exit (:exit result) :err (:err result)})))
    (str/trim (str (:out result)))))

(defn worktree-git-dir [path]
  (-> (slurp (io/file path ".git"))
      (str/replace #"^gitdir:\s*" "")
      str/trim))

(defn age-worktree! [path timestamp]
  (doseq [target [(io/file (worktree-git-dir path) "logs" "HEAD") (io/file path)]]
    (when (.exists target)
      (proc/shell {:out :string :err :string}
                  "touch" "-d" timestamp (.getPath target)))))

(defn census-repo! [census-root name]
  (let [root (.getCanonicalPath (io/file census-root name "main"))]
    (.mkdirs (io/file root))
    (git! "init" "-q" "-b" "main" root)
    (git! "-C" root "config" "user.email" "census@example.invalid")
    (git! "-C" root "config" "user.name" "Census Test")
    (spit (io/file root "tracked.txt") "canonical bytes\n")
    (git! "-C" root "add" "tracked.txt")
    (git-dated! aged-date "-C" root "commit" "-qm" "census fixture")
    {:name name
     :container (.getCanonicalPath (io/file census-root name))
     :root root}))

(defn census-worktree!
  [{:keys [container root]} slug {:keys [dirty? unmerged? aged?]}]
  (let [worktrees (doto (io/file container "worktrees") .mkdirs)
        path (.getCanonicalPath (io/file worktrees slug))]
    (git! "-C" root "worktree" "add" "-q" "-b" slug path "HEAD")
    (when unmerged?
      (spit (io/file path "own work.txt") "unlanded bytes\n")
      (git! "-C" path "add" "own work.txt")
      (git-dated! aged-date "-C" path "commit" "-qm" "unlanded census commit"))
    (when dirty?
      (spit (io/file path "uncommitted sentinel.txt") "dirty bytes must survive\n"))
    (when aged? (age-worktree! path aged-date))
    {:slug slug :branch slug :path path}))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north worktree janitor "
            (make-array java.nio.file.attribute.FileAttribute 0)))
      repo-name (str "main repo " (.getName tmp))
      home (doto (io/file tmp "home") .mkdirs)
      repo (.getCanonicalPath (io/file tmp repo-name))
      clone-clean-path (managed-clone-path repo "clone-clean")
      clone-dirty-path (managed-clone-path repo "clone-dirty")
      worktrees (doto (io/file tmp "managed worktrees") .mkdirs)
      census-root (doto (io/file tmp "census root") .mkdirs)
      log (io/file tmp "coordination.framlog")
      heartbeat (io/file tmp "worktree-heartbeat")
      agent-logs (doto (io/file tmp "agent logs") .mkdirs)
      git-log (io/file tmp "git-calls.log")
      git-wrapper (io/file tmp "git-wrapper")
      post-remove-marker (io/file tmp "post-remove-failure-armed")
      daemon (do
               (proc/process
                {:dir fram :out :string :err :string
                 :extra-env {"BEAGLE_STORE_SERVER_RUNTIME" "jvm-dev"
                             "BEAGLE_STORE_SERVER_QUIET" "1"
                             "BEAGLE_STORE_SERVER_XMX" "1g"}}
                (str fram "/bin/beagle-store-server") "serve" (str port)
                (.getCanonicalPath log) "north-coordination"))]
  (reset! test-log (.getCanonicalPath log))
  (try
    (when-not (await-up port)
      (throw (ex-info "throwaway Beagle Store coordinator did not start"
                      {:stdout (deref (:out daemon))
                       :stderr (deref (:err daemon))})))

    (git! "init" "-q" "-b" "main" repo)
    (git! "-C" repo "config" "user.email" "janitor@example.invalid")
    (git! "-C" repo "config" "user.name" "Janitor Test")
    (spit (io/file repo "tracked.txt") "canonical bytes\n")
    (git! "-C" repo "add" "tracked.txt")
    (git! "-C" repo "commit" "-qm" "fixture")

    (let [clean (create-worktree! repo worktrees "resolved-clean")
          dirty (create-worktree! repo worktrees "resolved-dirty")
          live (create-worktree! repo worktrees "live-clean")
          torn (create-worktree! repo worktrees "torn-terminal")
          hostile (create-worktree! repo worktrees "hostile-branch")
          status-fail (create-worktree! repo worktrees "status-failure")
          provenance-fail (create-worktree! repo worktrees "provenance-failure")
          post-remove-fail (create-worktree! repo worktrees "post-remove-failure")
          branch-delete-fail (create-worktree! repo worktrees "branch-delete-failure")
          clone-clean (create-clone! repo worktrees "clone-clean")
          clone-dirty (create-clone! repo worktrees "clone-dirty")
          control-subjects
          [["@worktree-allocation:janitor-fixture" "worktree_allocation"]
           ["@worktree-reservation:janitor-fixture" "worktree_reservation"]
           ["@worktree-control:janitor-fixture" "control"]]
          lanes [clean dirty live torn hostile status-fail provenance-fail
                 post-remove-fail branch-delete-fail clone-clean clone-dirty]
          demo (census-repo! census-root "demo")
          held (census-repo! census-root "held")
          census-reapable (census-worktree! demo "census-reapable" {:aged? true})
          census-dirty (census-worktree! demo "census-dirty"
                                         {:aged? true :dirty? true})
          census-unmerged (census-worktree! demo "census-unmerged"
                                            {:aged? true :unmerged? true})
          census-fresh (census-worktree! demo "census-fresh" {})
          census-claimed (census-worktree! demo "census-claimed" {:aged? true})
          census-held (census-worktree! held "census-held" {:aged? true})]
      (doseq [lane lanes] (register-lane! port repo lane nil))
      (doseq [[subject kind] control-subjects]
        (assert-fact! port subject "kind" kind)
        (assert-fact! port subject "worktree"
                      (str "/tmp/non-lane-" (subs subject 1))))
      ;; A registration of ANY kind claims a tree; the unregistered sweep must
      ;; never treat a claimed tree as its own.
      (assert-fact! port "@worktree-allocation:census-claimed" "kind"
                    "worktree_allocation")
      (assert-fact! port "@worktree-allocation:census-claimed" "worktree"
                    (:path census-claimed))
      ;; Graph data may never choose the branch passed to Git. Make one exact
      ;; registration hostile while its real worktree remains perfectly valid.
      (assert-fact! port (:subject hostile) "branch" "main")

      (commit-modern-terminal! port (:subject clean))
      (doseq [lane [dirty hostile status-fail provenance-fail
                    post-remove-fail branch-delete-fail clone-clean clone-dirty]]
        (commit-run! port (:handle lane)))
      ;; Torn modern lane terminal + uncommitted run: neither is terminal proof.
      (assert-fact! port (:subject torn) "process_outcome" "ran")
      (let [run (str "@run:" (:handle torn))]
        (assert-fact! port run "agent" (:handle torn))
        (assert-fact! port run "process_outcome" "ran"))

      (let [dirty-file (io/file (:path dirty) "uncommitted sentinel.txt")]
        (spit dirty-file "dirty bytes must survive\n"))
      (commit-file! (:path clone-clean) "harvested.txt" "durable clone commit\n" "harvest clone")
      (harvest-clone! repo clone-clean)
      (commit-file! (:path clone-dirty) "unharvested.txt" "sole clone commit\n" "unharvested clone")
      (spit (io/file (:path clone-dirty) "uncommitted sentinel.txt") "must survive\n")

      ;; A live concern's lease is the only thing holding `held`; every other
      ;; discriminator there is identical to the reapable tree in `demo`.
      (assert-fact! port "@concern-1785000000000-census" "kind" "concern")
      (assert-fact! port "@concern-1785000000000-census" "agent" "@census-owner")
      (assert-fact! port "@concern-1785000000000-census" "repo"
                    (.getCanonicalPath (io/file census-root "held")))
      (assert-fact! port "@concern-1785000000000-census" "reached" "building")
      (north.coord/acquire-lease!
       port "session:census-owner" "census-owner" (* 60 60 1000))

      ;; Test-only Git transport: every non-fault command execs the system Git.
      ;; Exact paths inject pre-mutation uncertainty, a post-remove observation
      ;; failure, and a branch-delete refusal. Every argv is recorded for the
      ;; non-force audit.
      (spit git-wrapper
            (str "#!/usr/bin/env bash\n"
                 "set -euo pipefail\n"
                 "printf '%q ' \"$@\" >> \"${GIT_CALL_LOG:?}\"\n"
                 "printf '\\n' >> \"${GIT_CALL_LOG:?}\"\n"
                 "if [[ ${1:-} == -C && ${2:-} == \"${STATUS_FAIL_PATH:?}\" && ${3:-} == status ]]; then exit 91; fi\n"
                 "if [[ ${1:-} == -C && ${2:-} == \"${PROVENANCE_FAIL_PATH:?}\" && ${3:-} == rev-parse ]]; then exit 92; fi\n"
                 "if [[ ${1:-} == -C && ${3:-} == worktree && ${4:-} == remove && ${6:-} == \"${POST_REMOVE_FAIL_PATH:?}\" ]]; then\n"
                 "  set +e\n"
                 "  \"${REAL_GIT:?}\" \"$@\"\n"
                 "  rc=$?\n"
                 "  set -e\n"
                 "  if (( rc == 0 )); then : > \"${POST_REMOVE_FAIL_MARKER:?}\"; fi\n"
                 "  exit \"$rc\"\n"
                 "fi\n"
                 "if [[ ${1:-} == -C && ${3:-} == worktree && ${4:-} == list && -f \"${POST_REMOVE_FAIL_MARKER:?}\" ]]; then\n"
                 "  mv \"${POST_REMOVE_FAIL_MARKER:?}\" \"${POST_REMOVE_FAIL_MARKER:?}.used\"\n"
                 "  exit 93\n"
                 "fi\n"
                 "if [[ ${1:-} == -C && ${3:-} == branch && ${4:-} == -d && ${6:-} == \"${BRANCH_DELETE_FAIL_BRANCH:?}\" ]]; then exit 94; fi\n"
                 "exec \"${REAL_GIT:?}\" \"$@\"\n"))
      (.setExecutable git-wrapper true)
      (spit git-log "")

      (let [watched [live torn hostile status-fail provenance-fail]
            before (into {} (map (juxt :handle #(tree-snapshot (:path %))) watched))
            dirty-before (tree-snapshot (:path dirty))
            environment {"HOME" (.getCanonicalPath home)
                         "NORTH_WORKER_HEARTBEAT" (.getCanonicalPath heartbeat)
                         "NORTH_MAINTENANCE_TASK_LOCK_PATH"
                         (.getCanonicalPath (io/file tmp "worktree-task.lock"))
                         "NORTH_AGENT_LOGS_DIR" (.getCanonicalPath agent-logs)
                         "NORTH_WORKTREE_ROOTS" (.getCanonicalPath census-root)
                         "NORTH_GIT_BIN" (.getCanonicalPath git-wrapper)
                         "REAL_GIT" (str/trim (:out (proc/shell {:out :string} "which" "git")))
                         "GIT_CALL_LOG" (.getCanonicalPath git-log)
                         "STATUS_FAIL_PATH" (:path status-fail)
                         "PROVENANCE_FAIL_PATH" (:path provenance-fail)
                         "POST_REMOVE_FAIL_PATH" (:path post-remove-fail)
                         "POST_REMOVE_FAIL_MARKER" (.getCanonicalPath post-remove-marker)
                         "BRANCH_DELETE_FAIL_BRANCH" (:branch branch-delete-fail)}
            census-before (into {} (map (juxt :slug #(tree-snapshot (:path %))))
                                [census-reapable census-dirty census-unmerged
                                 census-fresh census-claimed census-held])
            before-dry-log (sha256-file log)
            dry-run (run-maintenance port environment "--dry-run")
            after-dry-log (sha256-file log)
            census-after-dry
            (into {} (map (juxt :slug #(tree-snapshot (:path %))))
                  [census-reapable census-dirty census-unmerged
                   census-fresh census-claimed census-held])
            first-run (run-maintenance port environment)
            after-first-log (sha256-file log)
            orphan-values (many port (:subject dirty) "worktree_orphaned")]
        (check "production worktree task exits zero"
               (zero? (:exit first-run)) (str (:out first-run) (:err first-run)))
        (check "dry-run detects the unregistered reapable tree without mutating it"
               (and (zero? (:exit dry-run))
                    (str/includes? (:out dry-run)
                                   (str "WOULD REMOVE unregistered "
                                        (:path census-reapable)))
                    (str/includes? (:out dry-run) ":unregistered")
                    (str/includes? (:out dry-run) ":would-remove 1")
                    (str/includes? (:out dry-run) ":review 2")
                    (str/includes? (:out dry-run) ":live-concern 1")
                    (str/includes? (:out dry-run) ":scanned 6")
                    (= census-before census-after-dry))
               (:out dry-run))
        (check "dry-run surfaces dirty and unmerged stale trees for review"
               (and (str/includes? (:out dry-run)
                                   (str "REVIEW unregistered " (:path census-dirty)))
                    (str/includes? (:out dry-run) "dirty (0 tracked, 1 untracked)")
                    (str/includes? (:out dry-run)
                                   (str "REVIEW unregistered " (:path census-unmerged)))
                    (str/includes? (:out dry-run) "unmerged (1 commits not in main)"))
               (:out dry-run))
        (check "dry-run never mentions the fresh or claimed tree"
               (and (not (str/includes? (:out dry-run) (:path census-fresh)))
                    (not (str/includes? (:out dry-run) (:path census-claimed))))
               (:out dry-run))
        (check "dry-run performs zero coordinator writes"
               (= before-dry-log after-dry-log))
        (check "maintenance summary exposes janitor clone result"
               (and (str/includes? (:out first-run) ":registered")
                    (str/includes? (:out first-run) ":dirty 2")
                    (str/includes? (:out first-run) ":removed 2")
                    (str/includes? (:out first-run) ":partial 2")
                    (str/includes? (:out first-run) ":orphan-facts-written 2"))
               (:out first-run))
        (check "non-lane worktree control subjects never enter classification"
               (and (not (str/includes? (:out first-run)
                                        "invalid managed-lane subject"))
                    (every? #(not (str/includes? (:out first-run) (first %)))
                            control-subjects))
               (:out first-run))
        (check "resolved-clean expected worktree disappears"
               (not (.exists (io/file (:path clean)))))
        (check "resolved-clean expected branch disappears"
               (not (branch-present? repo (:branch clean))))
        (check "harvested managed clone disappears without deleting its durable ref"
               (and (not (.exists (io/file (:path clone-clean))))
                    (branch-present? repo (:branch clone-clean))))
        (check "dirty unharvested clone is retained with a named reason"
               (and (.isDirectory (io/file (:path clone-dirty)))
                    (str/includes? (:out first-run)
                                   "dirty clone has unharvested commits; manual salvage required"))
               (:out first-run))
        (check "post-remove observation failure reports the removed tree as partial"
               (and (not (.exists (io/file (:path post-remove-fail))))
                    (branch-present? repo (:branch post-remove-fail))
                    (str/includes? (:out first-run)
                                   (str "PARTIAL cleanup " (:subject post-remove-fail)))
                    (not (str/includes? (:out first-run)
                                        (str "KEEP/REVIEW " (:subject post-remove-fail)))))
               (:out first-run))
        (check "branch-delete failure reports partial cleanup without claiming the tree was kept"
               (and (not (.exists (io/file (:path branch-delete-fail))))
                    (branch-present? repo (:branch branch-delete-fail))
                    (str/includes? (:out first-run)
                                   (str "PARTIAL cleanup " (:subject branch-delete-fail)))
                    (not (str/includes? (:out first-run)
                                        (str "KEEP/REVIEW " (:subject branch-delete-fail)))))
               (:out first-run))
        (check "every non-removable worktree remains present"
               (every? #(.isDirectory (io/file (:path %)))
                       [dirty live torn hostile status-fail provenance-fail clone-dirty]))
        (check "every non-removable branch remains at its authoritative location"
               (and (every? #(branch-present? repo (:branch %))
                            [dirty live torn hostile status-fail provenance-fail])
                    (branch-present? (:path clone-dirty) (:branch clone-dirty))
                    (not (branch-present? repo (:branch clone-dirty)))))
        (check "dirty worktree bytes survive exactly"
               (= dirty-before (tree-snapshot (:path dirty))))
        (doseq [lane watched]
          (check (str (:handle lane) " remains byte-identical")
                 (= (get before (:handle lane)) (tree-snapshot (:path lane)))))
        (check "dirty resolved lane gets exactly one deterministic orphan fact"
               (and (= 1 (count orphan-values))
                    (str/includes? (first orphan-values)
                                   "resolved lane retains uncommitted changes"))
               (pr-str orphan-values))
        (check "unregistered reapable tree and its branch are reclaimed"
               (and (not (.exists (io/file (:path census-reapable))))
                    (not (branch-present? (:root demo) (:branch census-reapable)))
                    (str/includes? (:out first-run)
                                   (str "removed unregistered " (:path census-reapable)))
                    (str/includes? (:out first-run) ":unregistered")
                    (str/includes? (:out first-run) ":removed 1")
                    (str/includes? (:out first-run) ":scanned 6"))
               (:out first-run))
        (check "no unregistered tree held by dirt, an unmerged commit, a claim, freshness, or a live concern is touched"
               (and (every? #(.isDirectory (io/file (:path %)))
                            [census-dirty census-unmerged census-fresh
                             census-claimed census-held])
                    (every? #(= (get census-before (:slug %))
                                (tree-snapshot (:path %)))
                            [census-dirty census-unmerged census-fresh
                             census-claimed census-held])
                    (branch-present? (:root demo) (:branch census-dirty))
                    (branch-present? (:root demo) (:branch census-unmerged))
                    (branch-present? (:root demo) (:branch census-claimed))
                    (branch-present? (:root held) (:branch census-held)))
               (:out first-run))
        (check "a live concern keeps an otherwise reapable tree, and says so"
               (str/includes? (:out first-run)
                              (str "KEEP unregistered " (:path census-held)
                                   " — idle 4d but its repository has a live concern"))
               (:out first-run))
        (let [calls (slurp git-log)]
          (check "janitor issued no force deletion"
                 (not (str/includes? calls "--force")) calls)
          (check "janitor used non-force worktree remove and branch -d"
                 (and (str/includes? calls "worktree remove --")
                      (str/includes? calls "branch -d --")) calls)
          (check "hostile graph branch never reaches a Git delete argv"
                 (not (str/includes? calls "branch -d -- main")) calls)
          (check "post-remove uncertainty does not attempt branch deletion"
                 (not (str/includes? calls
                                     (str "branch -d -- " (:branch post-remove-fail))))
                 calls)
          (check "branch-delete regression reaches only the non-force delete"
                 (str/includes? calls
                                (str "branch -d -- " (:branch branch-delete-fail)))
                 calls))

        ;; A second production pass is the idempotency bar: no tree/branch is
        ;; removed, the dirty fact is not rewritten, and the coordinator log is
        ;; byte-identical to the post-first-pass log.
        (let [second-run (run-maintenance port environment)
              after-second-log (sha256-file log)
              orphan-values-2 (many port (:subject dirty) "worktree_orphaned")]
          (check "repeat worktree task exits zero" (zero? (:exit second-run))
                 (str (:out second-run) (:err second-run)))
          (check "repeat removes zero worktrees and writes zero orphan facts"
                 (and (str/includes? (:out second-run) ":registered")
                      (str/includes? (:out second-run) ":removed 0")
                      (str/includes? (:out second-run) ":partial 2")
                      (str/includes? (:out second-run) ":already-removed 2")
                      (str/includes? (:out second-run) ":orphan-facts-written 0"))
                 (:out second-run))
          (check "repeat never relabels an absent worktree as kept"
                 (and (not (str/includes? (:out second-run)
                                          (str "KEEP " (:subject clean))))
                      (not (str/includes? (:out second-run)
                                          (str "KEEP " (:subject post-remove-fail))))
                      (not (str/includes? (:out second-run)
                                          (str "KEEP " (:subject branch-delete-fail))))
                      (str/includes? (:out second-run)
                                     (str "PARTIAL cleanup " (:subject post-remove-fail)))
                      (str/includes? (:out second-run)
                                     (str "PARTIAL cleanup " (:subject branch-delete-fail))))
                 (:out second-run))
          (check "repeat reclaims no further unregistered tree"
                 (and (str/includes? (:out second-run) ":unregistered")
                      (str/includes? (:out second-run) ":removed 0")
                      (str/includes? (:out second-run) ":review 2")
                      (str/includes? (:out second-run) ":live-concern 1")
                      (str/includes? (:out second-run) ":scanned 5"))
                 (:out second-run))
          (check "repeat performs zero coordinator writes"
                 (= after-first-log after-second-log))
          (check "repeat leaves exactly the same single orphan fact"
                 (= orphan-values orphan-values-2))
          (check "heartbeat carries the latest worktree-janitor result"
                 (and (.isFile heartbeat)
                      (str/includes? (slurp heartbeat) ":registered")
                      (str/includes? (slurp heartbeat) ":removed 0")
                      (str/includes? (slurp heartbeat) ":partial 2")
                      (str/includes? (slurp heartbeat) ":already-removed 2")
                      (str/includes? (slurp heartbeat) ":unregistered"))
                 (when (.isFile heartbeat) (slurp heartbeat))))))

      ;; Lander fixture: a ref at main is safe to delete; a descendant is not.
      (git! "-C" repo "branch" "lane-lane-landed" "main")
      (let [tree (git! "-C" repo "rev-parse" "HEAD^{tree}")
            unlanded (git! "-C" repo "commit-tree" tree "-p" "HEAD" "-m" "unlanded lane")]
        (git! "-C" repo "update-ref" "refs/heads/lane-lane-unlanded" unlanded)
        (let [land (proc/shell {:out :string :err :string :continue true}
                               "bb" lander repo)]
          (check "lander deletes only landed harvested refs and inventories unlanded refs"
                 (and (zero? (:exit land))
                      (not (branch-present? repo "lane-lane-landed"))
                      (branch-present? repo "lane-lane-unlanded")
                      (str/includes? (:out land) "LANDED DELETE refs/heads/lane-lane-landed")
                      (str/includes? (:out land) "UNLANDED KEEP refs/heads/lane-lane-unlanded")
                      (str/includes? (:out land) "age=")
                      (str/includes? (:out land) "subject="))
                 (str (:out land) (:err land)))))

    (finally
      (try (proc/destroy-tree daemon) (catch Throwable _ nil))
      (fs/delete-tree clone-clean-path)
      (fs/delete-tree clone-dirty-path)
      (doseq [file (reverse (file-seq tmp))]
        (try (io/delete-file file true) (catch Throwable _ nil)))))

  (let [results @checks pass (count (filter second results))]
    (doseq [[label ok detail] results]
      (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
      (when (and (not ok) detail) (println (str "        " detail))))
    (println (format "\nworktree janitor integration: %d / %d PASS"
                     pass (count results)))
    (System/exit (if (= pass (count results)) 0 1))))
