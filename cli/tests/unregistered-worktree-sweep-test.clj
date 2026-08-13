#!/usr/bin/env bb
;; Unregistered-worktree janitor regression, driven with NO live daemon: the two
;; graph joins are deferred values, so real Git is the only authority under test.
;; The scheduled worktree task calls this exact function.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
;; coord.clj only to satisfy the janitor's coordinator-query symbols; this test
;; never reaches them, because both graph joins arrive already resolved.
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/worktree-census.clj"))
(load-file (str root "/cli/worktree-janitor.clj"))

;; Four days before now: past the 48h horizon by a margin no clock skew closes.
(def aged-date
  (.format (.minusDays (java.time.ZonedDateTime/now) 4)
           java.time.format.DateTimeFormatter/ISO_OFFSET_DATE_TIME))
(def checks (atom []))
(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(defn run-git [& args]
  (apply proc/shell {:out :string :err :string :continue true} "git" args))

(defn git! [& args]
  (let [result (apply run-git args)]
    (when-not (zero? (:exit result))
      (throw (ex-info "fixture git command failed"
                      {:args args :exit (:exit result) :err (:err result)})))
    (str/trim (str (:out result)))))

(defn git-dated! [& args]
  (let [result (apply proc/shell
                      {:out :string :err :string :continue true
                       :extra-env {"GIT_AUTHOR_DATE" aged-date
                                   "GIT_COMMITTER_DATE" aged-date}}
                      "git" args)]
    (when-not (zero? (:exit result))
      (throw (ex-info "fixture dated git command failed"
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

(defn worktree-git-dir [path]
  (-> (slurp (io/file path ".git")) (str/replace #"^gitdir:\s*" "") str/trim))

(defn age-worktree! [path]
  (doseq [target [(io/file (worktree-git-dir path) "logs" "HEAD") (io/file path)]]
    (when (.exists target)
      (proc/shell {:out :string :err :string}
                  "touch" "-d" aged-date (.getPath target)))))

(defn census-repo! [census-root name]
  (let [main (.getCanonicalPath (io/file census-root name "main"))]
    (.mkdirs (io/file main))
    (git! "init" "-q" "-b" "main" main)
    (git! "-C" main "config" "user.email" "census@example.invalid")
    (git! "-C" main "config" "user.name" "Census Test")
    (spit (io/file main "tracked.txt") "canonical bytes\n")
    (git! "-C" main "add" "tracked.txt")
    (git-dated! "-C" main "commit" "-qm" "census fixture")
    {:name name
     :container (.getCanonicalPath (io/file census-root name))
     :root main}))

(defn census-pin!
  "A pin in the exact shape both live pins have: clean, merged, DETACHED,
   and aged past the horizon. Nothing but its parent directory keeps it out
   of the reaper's hands."
  [{:keys [container root]} name]
  (let [pins (doto (io/file container "pins") .mkdirs)
        path (.getCanonicalPath (io/file pins name))
        head (git! "-C" root "rev-parse" "HEAD")]
    (git! "-C" root "worktree" "add" "-q" "--detach" path head)
    (spit (io/file pins (str name ".pin"))
          (str name " — externally consumed. Consumers: a build outside this repo.\n"))
    (age-worktree! path)
    {:name name :path path}))

(defn census-worktree!
  [{:keys [container root]} slug {:keys [dirty? unmerged? aged?]}]
  (let [worktrees (doto (io/file container "worktrees") .mkdirs)
        path (.getCanonicalPath (io/file worktrees slug))]
    (git! "-C" root "worktree" "add" "-q" "-b" slug path "HEAD")
    (when unmerged?
      (spit (io/file path "own work.txt") "unlanded bytes\n")
      (git! "-C" path "add" "own work.txt")
      (git-dated! "-C" path "commit" "-qm" "unlanded census commit"))
    (when dirty?
      (spit (io/file path "uncommitted sentinel.txt") "dirty bytes must survive\n"))
    (when aged? (age-worktree! path))
    {:slug slug :branch slug :path path}))

(defn sweep! [census-root dry? claimed live-repos]
  (let [out (java.io.StringWriter.)
        result (binding [*out* out]
                 (with-redefs [north.worktree-census/roots (fn [] [census-root])]
                   (north.worktree-janitor/sweep-unregistered-worktrees!
                    {:dry? dry?
                     :claimed-worktrees (delay claimed)
                     :live-concern-repos (delay live-repos)})))]
    {:result result :out (str out)}))

(let [tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north unregistered sweep "
            (make-array java.nio.file.attribute.FileAttribute 0)))
      census-root (.getCanonicalPath (doto (io/file tmp "census root") .mkdirs))]
  (try
    (let [demo (census-repo! census-root "demo")
          held (census-repo! census-root "held")
          reapable (census-worktree! demo "reapable" {:aged? true})
          dirty (census-worktree! demo "dirty" {:aged? true :dirty? true})
          unmerged (census-worktree! demo "unmerged" {:aged? true :unmerged? true})
          fresh (census-worktree! demo "fresh" {})
          claimed (census-worktree! demo "claimed" {:aged? true})
          held-tree (census-worktree! held "held" {:aged? true})
          pin (census-pin! demo "gjoa")
          foreign (doto (io/file (:container demo) "worktrees" "foreign") .mkdirs)
          _ (proc/shell {:out :string :err :string}
                        "touch" "-d" aged-date (.getPath foreign))
          claimed-set #{(:path claimed)}
          live-repos #{(:container held)}
          before (into {} (map (juxt :slug #(tree-snapshot (:path %))))
                       [reapable dirty unmerged fresh claimed held-tree])
          pin-before (tree-snapshot (:path pin))
          dry (sweep! census-root true claimed-set live-repos)]

      ;; T17 — the highest-value assertion in the migration. `:scanned 7` is the
      ;; whole proof: the pin is not kept, not reviewed, not skipped — it is
      ;; NEVER ENUMERATED. A sweeper that opts in on worktrees/ cannot reach it.
      (check "dry run detects exactly one reclaimable unregistered tree"
             (= {:scanned 7 :claimed 1 :fresh 1 :review 3 :live-concern 1
                 :uncertain 0 :partial 0 :removed 0 :would-remove 1 :errors 0}
                (:result dry))
             (pr-str (:result dry)))
      (check "dry run names the tree it would remove and why"
             (str/includes? (:out dry)
                            (str "WOULD REMOVE unregistered " (:path reapable)
                                 " — idle 4d, merged into main, clean"))
             (:out dry))
      (check "dry run surfaces dirty, unmerged, and foreign trees for review"
             (and (str/includes? (:out dry)
                                 (str "REVIEW unregistered " (:path dirty)
                                      " — idle 4d, dirty (0 tracked, 1 untracked)"))
                  (str/includes? (:out dry)
                                 (str "REVIEW unregistered " (:path unmerged)
                                      " — idle 4d, unmerged (1 commits not in main)"))
                  (str/includes? (:out dry)
                                 (str "REVIEW unregistered " (.getCanonicalPath foreign)
                                      " — idle 4d, not a registered git worktree")))
             (:out dry))
      (check "dry run reports the live concern that holds an otherwise reapable tree"
             (str/includes? (:out dry)
                            (str "KEEP unregistered " (:path held-tree)
                                 " — idle 4d but its repository has a live concern"))
             (:out dry))
      (check "dry run never mentions a fresh or claimed tree"
             (and (not (str/includes? (:out dry) (:path fresh)))
                  (not (str/includes? (:out dry) (:path claimed))))
             (:out dry))
      (check "a pin never appears in the sweep at all (T17)"
             (not (str/includes? (:out dry) (:path pin)))
             (:out dry))
      (check "the census does not enumerate a pins/ tree (T23)"
             (not (contains?
                   (into #{} (map :worktree)
                         (north.worktree-census/repo-rows
                          {:repo "demo" :container (:container demo) :root (:root demo)}))
                   (:path pin))))
      (check "dry run mutates nothing"
             (and (every? #(.isDirectory (io/file (:path %)))
                          [reapable dirty unmerged fresh claimed held-tree])
                  (every? #(= (get before (:slug %)) (tree-snapshot (:path %)))
                          [reapable dirty unmerged fresh claimed held-tree])))

      (let [live (sweep! census-root false claimed-set live-repos)]
        (check "production sweep reclaims exactly the proven tree"
               (= {:scanned 7 :claimed 1 :fresh 1 :review 3 :live-concern 1
                   :uncertain 0 :partial 0 :removed 1 :would-remove 0 :errors 0}
                  (:result live))
               (pr-str (:result live)))
        (check "the reclaimed worktree and its branch are both gone"
               (and (not (.exists (io/file (:path reapable))))
                    (not (branch-present? (:root demo) (:branch reapable)))
                    (str/includes? (:out live)
                                   (str "removed unregistered " (:path reapable)
                                        " and " (:branch reapable))))
               (:out live))
        (check "no held tree loses a byte or a branch"
               (and (every? #(= (get before (:slug %)) (tree-snapshot (:path %)))
                            [dirty unmerged fresh claimed held-tree])
                    (branch-present? (:root demo) (:branch dirty))
                    (branch-present? (:root demo) (:branch unmerged))
                    (branch-present? (:root demo) (:branch claimed))
                    (branch-present? (:root held) (:branch held-tree))
                    (.isDirectory foreign)))
        (check "the pin survives a production sweep byte for byte (T17)"
               (and (.isDirectory (io/file (:path pin)))
                    (= pin-before (tree-snapshot (:path pin)))
                    (not (str/includes? (:out live) (:path pin))))
               (:out live))
        (check "git's own registration list no longer carries the reclaimed tree"
               (not (str/includes?
                     (str/join " " (map :path (north.worktree-census/registered-worktrees
                                               (:root demo))))
                     (:path reapable)))))

      (let [again (sweep! census-root false claimed-set live-repos)]
        (check "a repeat sweep is idempotent"
               (= {:scanned 6 :claimed 1 :fresh 1 :review 3 :live-concern 1
                   :uncertain 0 :partial 0 :removed 0 :would-remove 0 :errors 0}
                  (:result again))
               (pr-str (:result again))))

      ;; A live concern is the ONLY difference between `held` and the reclaimed
      ;; tree: drop the claim and the same tree becomes reclaimable.
      (let [released (sweep! census-root false claimed-set #{})]
        (check "releasing the live concern makes the held tree reclaimable"
               (and (= 1 (:removed (:result released)))
                    (not (.exists (io/file (:path held-tree))))
                    (not (branch-present? (:root held) (:branch held-tree))))
               (pr-str (:result released)))))

    (finally
      (doseq [file (reverse (file-seq tmp))]
        (try (io/delete-file file true) (catch Throwable _ nil)))))

  (let [results @checks pass (count (filter second results))]
    (doseq [[label ok detail] results]
      (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
      (when (and (not ok) detail) (println (str "        " detail))))
    (println (format "\nunregistered worktree sweep: %d / %d PASS"
                     pass (count results)))
    (System/exit (if (= pass (count results)) 0 1))))
