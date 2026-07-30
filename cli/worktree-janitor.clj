;; Reactor-owned cleanup for terminal managed-lane worktrees.
;;
;; This file is a library, not a command. Loading it only defines functions: the
;; production surface is `north-reactor.clj sweep-once`, and the long-running
;; reactor calls the exact same `sweep-worktrees!` function on its normal sweep.
(ns north.worktree-janitor
  (:require [babashka.fs :as fs]
            [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^:private max-agent-id-chars 512)
(def ^:private max-path-chars 4096)
(def ^:private clone-push-sentinel "north-disabled://managed-clone-no-push")

(defn- query-rows! [port query]
  (let [response (north.coord/send-op port {:op :query :query query})]
    (if (and (map? response) (contains? response :ok))
      (:ok response)
      (throw (ex-info "worktree janitor coordinator query failed"
                      {:type :worktree-janitor-query-failed
                       :response response})))))

(defn- q-col [port body]
  (->> (query-rows!
        port
        {:find "e"
         :rules [{:head {:rel "e" :args [{:var "e"}]}
                  :body body}]})
       (map first)))

(defn- subject-facts [port subject]
  (let [rows (query-rows!
              port
              {:find "worktree_fact"
               :rules [{:head {:rel "worktree_fact"
                               :args [{:var "p"} {:var "r"}]}
                        :body [{:rel "triple"
                                :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [facts [predicate value]]
              (update facts predicate (fnil conj #{}) value))
            {}
            rows)))

(defn- singleton [facts predicate]
  (let [values (get facts predicate)]
    (when (= 1 (count values)) (first values))))

(defn- safe-string? [value max-chars]
  (and (string? value)
       (not (str/blank? value))
       (<= (count value) max-chars)
       (not (re-find #"[\u0000\r\n]" value))))

(defn- agent-handle [subject]
  (when (and (safe-string? subject (+ max-agent-id-chars 7))
             (str/starts-with? subject "@agent:"))
    (let [handle (subs subject 7)]
      (when (safe-string? handle max-agent-id-chars) handle))))

(defn- expand-home [path]
  (if (str/starts-with? path "~/")
    (when-let [home (System/getenv "HOME")]
      (str (str/replace home #"/+$" "") (subs path 1)))
    path))

(defn- registered-path [value]
  (when (safe-string? value max-path-chars)
    (when-let [expanded (expand-home value)]
      (let [file (io/file expanded)]
        (when (.isAbsolute file) (.getCanonicalPath file))))))

(defn- git-bin [] (or (System/getenv "NORTH_GIT_BIN") "git"))

(defn- git [& args]
  (apply proc/shell
         {:out :string :err :string :continue true}
         (git-bin) args))

(defn- canonical-git-path [result]
  (when (zero? (:exit result))
    (let [raw (str/trim (str (:out result)))]
      (when (safe-string? raw max-path-chars)
        (.getCanonicalPath (io/file raw))))))

(defn- branch-ref [branch] (str "refs/heads/" branch))

(defn- git-output [result]
  (when (zero? (:exit result)) (str/trim (str (:out result)))))

(defn- worktree-registered? [root worktree]
  (let [result (git "-C" root "worktree" "list" "--porcelain" "-z")]
    (when (zero? (:exit result))
      (boolean
       (some #(= (str "worktree " worktree) %)
             (str/split (str (:out result)) #"\u0000" -1))))))

(defn- branch-present? [root branch]
  (let [result (git "-C" root "show-ref" "--verify" "--quiet"
                    (branch-ref branch))]
    (cond
      (zero? (:exit result)) true
      (= 1 (:exit result)) false
      :else nil)))

(defn- known-state [value]
  (cond (= true value) "present"
        (= false value) "absent"
        :else "unknown"))

(defn- absent-cleanup-state
  "Recognize a prior cleanup without pretending an absent tree was retained.
   A fully absent registration+branch is idempotently settled; any other absent
   worktree state remains an explicit partial-cleanup incident."
  [handle facts]
  (let [expected-branch (str "lane-" handle)
        graph-kind (singleton facts "kind")
        graph-branch (singleton facts "branch")
        root (registered-path (singleton facts "repo"))
        worktree (registered-path (singleton facts "worktree"))]
    (when (and worktree (not (.exists (io/file worktree))))
      (if (and (= "lane" graph-kind)
               (= expected-branch graph-branch)
               root
               (.isDirectory (io/file root)))
        (let [registered? (worktree-registered? root worktree)
              branch-present (branch-present? root expected-branch)
              expected-clone (str "/tmp/" (.getName (io/file root)) "-" expected-branch)]
          (if (or (and (= false registered?) (= false branch-present))
                  ;; A provisioned clone is intentionally unregistered. Its exact
                  ;; path shape identifies a prior clean removal while its harvested
                  ;; canonical ref remains for the separate landing reconciler.
                  (and (= false registered?) (= expected-clone worktree)))
            {:kind :already-removed}
            {:kind :partial
             :reason (str "worktree is absent from disk"
                          " (registration=" (known-state registered?)
                          ", branch=" (known-state branch-present) ")")}))
        {:kind :partial
         :reason "registered worktree path is absent; exact cleanup provenance cannot be reconstructed"}))))

(defn- validate-provenance
  "Validate every destructive-action input against Git itself. Graph values are
   only registrations; they never grant deletion authority."
  [handle facts]
  (try
    (let [expected-branch (str "lane-" handle)
          graph-kind (singleton facts "kind")
          graph-branch (singleton facts "branch")
          root (registered-path (singleton facts "repo"))
          worktree (registered-path (singleton facts "worktree"))]
      (cond
        (not= "lane" graph-kind)
        {:ok? false :reason "subject is not one exact kind=lane entity"}

        (not= expected-branch graph-branch)
        {:ok? false :reason "registered branch is absent, conflicted, or not the derived lane branch"}

        (or (nil? root) (nil? worktree) (= root worktree))
        {:ok? false :reason "registered main root/worktree paths are absent, conflicted, relative, or identical"}

        (not (.isDirectory (io/file root)))
        {:ok? false :reason "registered main root is not a directory"}

        (not (.isDirectory (io/file worktree)))
        {:ok? false :reason "registered worktree is not a directory"}

        :else
        (let [ref-check (git "check-ref-format" "--branch" expected-branch)
              root-top (canonical-git-path
                        (git "-C" root "rev-parse" "--show-toplevel"))
              wt-top (canonical-git-path
                      (git "-C" worktree "rev-parse" "--show-toplevel"))
              root-common (canonical-git-path
                           (git "-C" root "rev-parse"
                                "--path-format=absolute" "--git-common-dir"))
              wt-common (canonical-git-path
                         (git "-C" worktree "rev-parse"
                              "--path-format=absolute" "--git-common-dir"))
              root-dot-git (.getCanonicalPath (io/file root ".git"))
              wt-git-dir (canonical-git-path
                          (git "-C" worktree "rev-parse" "--absolute-git-dir"))
              actual-branch-result
              (git "-C" worktree "symbolic-ref" "--quiet" "--short" "HEAD")
              actual-branch (when (zero? (:exit actual-branch-result))
                              (str/trim (str (:out actual-branch-result))))
              root-head-result (git "-C" root "rev-parse" "HEAD")
              branch-head-result (git "-C" root "rev-parse" (branch-ref expected-branch))
              wt-head-result (git "-C" worktree "rev-parse" "HEAD")
              root-head (when (zero? (:exit root-head-result))
                          (str/trim (str (:out root-head-result))))
              branch-head (when (zero? (:exit branch-head-result))
                            (str/trim (str (:out branch-head-result))))
              wt-head (when (zero? (:exit wt-head-result))
                        (str/trim (str (:out wt-head-result))))
              registered? (worktree-registered? root worktree)
              branch-present (branch-present? root expected-branch)
              linked-git-prefix (when root-common
                                  (str root-common java.io.File/separator "worktrees"
                                       java.io.File/separator))
              clone-dot-git (.getCanonicalPath (io/file worktree ".git"))
              clone-origin (git-output (git "-C" worktree "config" "--get" "remote.origin.url"))
              clone-push (git-output (git "-C" worktree "config" "--get" "remote.origin.pushurl"))
              clone-head-merged (git "-C" root "merge-base" "--is-ancestor" wt-head "HEAD")
              clone-ref (git-output (git "-C" root "rev-parse" "--verify"
                                         (branch-ref expected-branch)))
              clone? (and (zero? (:exit ref-check))
                          (= root root-top)
                          (= worktree wt-top)
                          (= root-common root-dot-git)
                          (= wt-common clone-dot-git)
                          (= wt-git-dir clone-dot-git)
                          (.isDirectory (io/file clone-dot-git))
                          (= false registered?)
                          (= expected-branch actual-branch)
                          (= root clone-origin)
                          (= clone-push-sentinel clone-push)
                          (safe-string? wt-head 128))]
          (cond
            (and (zero? (:exit ref-check))
                   (= root root-top)
                   (= worktree wt-top)
                   (= root-common wt-common root-dot-git)
                   (.isDirectory (io/file root-dot-git))
                   (string? wt-git-dir)
                   (str/starts-with? wt-git-dir linked-git-prefix)
                   (= expected-branch actual-branch)
                   (= true branch-present)
                   (= true registered?)
                   (safe-string? root-head 128)
                   (= branch-head wt-head))
            {:ok? true :mode :linked :root root :worktree worktree :branch expected-branch}

            clone?
            {:ok? true :mode :clone :root root :worktree worktree :branch expected-branch
             :harvested? (or (= wt-head clone-ref) (zero? (:exit clone-head-merged)))}

            :else
            {:ok? false :reason "real Git provenance does not exactly match a registered worktree or managed clone"}))))
    (catch Throwable error
      {:ok? false
       :reason (str "Git provenance probe failed: "
                    (or (.getMessage error) (.getName (class error))))})))

(defn- worktree-status [worktree]
  (let [result (git "-C" worktree "status" "--porcelain=v1" "-z"
                    "--untracked-files=all")]
    (cond
      (not (zero? (:exit result))) {:kind :uncertain :reason "git status failed"}
      (empty? (str (:out result))) {:kind :clean}
      :else {:kind :dirty})))

(defn- orphan-fact [worktree branch]
  (str worktree " | branch=" branch
       " | resolved lane retains uncommitted changes; manual salvage required"))

(defn- ensure-orphan-fact! [port subject value]
  (if (contains? (set (north.coord/many port subject "worktree_orphaned")) value)
    false
    (do
      (north.coord/append! port subject "worktree_orphaned" value)
      (when-not (contains? (set (north.coord/many port subject "worktree_orphaned")) value)
        (throw (ex-info "worktree orphan fact was not visible after append"
                        {:subject subject})))
      true)))

(defn- remove-clean-worktree! [{:keys [root worktree branch]}]
  ;; Preflight the exact condition `git branch -d` enforces. This makes a partial
  ;; tree-only removal vanishingly narrow while preserving Git's own non-force
  ;; delete as the final authority.
  (let [merged (git "-C" root "merge-base" "--is-ancestor"
                    (branch-ref branch) "HEAD")]
    (if-not (zero? (:exit merged))
      {:kind :uncertain :reason "lane branch is not proven merged into the registered main checkout HEAD"}
      (let [removed (git "-C" root "worktree" "remove" "--" worktree)]
        ;; A Git command may fail after changing state. Inspect the postcondition
        ;; even on non-zero exit so a removed tree is never reported as kept.
        (let [path-present? (.exists (io/file worktree))
              registered? (worktree-registered? root worktree)]
          (cond
            ;; Exact pre-state still holds: this is the only post-attempt outcome
            ;; that can truthfully be reported as KEEP.
            (and path-present? (= true registered?))
            {:kind :uncertain
             :reason (if (zero? (:exit removed))
                       "git reported success but the registered worktree remains"
                       "non-force git worktree remove refused and the registered worktree remains")}

            ;; Exact worktree removal is proved; only now may branch deletion run.
            (and (not path-present?) (= false registered?))
            (let [deleted (git "-C" root "branch" "-d" "--" branch)
                  branch-present (branch-present? root branch)]
              (cond
                (= false branch-present)
                {:kind :removed}

                (= true branch-present)
                {:kind :partial
                 :reason (if (zero? (:exit deleted))
                           "worktree removed; Git reported branch-delete success but the branch remains"
                           "worktree removed; non-force branch delete refused and the branch remains")}

                :else
                {:kind :partial
                 :reason (str "worktree removed; branch deletion postcondition is unknown"
                              " (git-exit=" (:exit deleted) ")")}))

            ;; The command ran and the exact before/after state is no longer
            ;; provable. This is a cleanup incident, not a retained tree.
            :else
            {:kind :partial
             :reason
             (str "worktree removal outcome is partial or unknown"
                  " (path=" (if path-present? "present" "absent")
                  ", registration="
                  (cond (= true registered?) "present"
                        (= false registered?) "absent"
                        :else "unknown")
                  ", git-exit=" (:exit removed) ")")}))))))

(defn- remove-clean-clone! [{:keys [worktree]}]
  ;; A managed clone owns its entire git-dir. Provenance above establishes its
  ;; marker tuple before this recursive removal; no canonical ref is touched.
  (try
    (fs/delete-tree worktree)
    (if (.exists (io/file worktree))
      {:kind :partial :reason "managed clone removal returned but its path remains"}
      {:kind :removed})
    (catch Throwable error
      {:kind :partial :reason (str "managed clone removal failed: " (.getMessage error))})))

(defn- zero-result []
  {:scanned 0
   :unresolved 0
   :dirty 0
   :uncertain 0
   :partial 0
   :already-removed 0
   :removed 0
   :would-remove 0
   :orphan-facts-written 0
   :errors 0})

(defn- bump [result key] (update result key (fnil inc 0)))

(defn- lane-resolution [lane-resolved? handle]
  (try
    {:known? true :resolved? (boolean (lane-resolved? handle))}
    (catch Throwable error
      {:known? false
       :reason (str "canonical lane-resolution probe failed: "
                    (or (.getMessage error) (.getName (class error))))})))

(defn- sweep-subject!
  [port dry? lane-resolved? repo-filter subject]
  (let [handle (agent-handle subject)
        facts (subject-facts port subject)
        graph-repo (singleton facts "repo")
        resolution (when handle (lane-resolution lane-resolved? handle))]
    (cond
      (and repo-filter (not= repo-filter graph-repo))
      {:kind :skipped}

      (nil? handle)
      (do
        (println (str "[worktrees] KEEP " subject
                      " — invalid managed-lane subject"))
        {:kind :uncertain})

      (not (:known? resolution))
      (do
        (println (str "[worktrees] KEEP " subject " — " (:reason resolution)))
        {:kind :uncertain})

      (not (:resolved? resolution))
      {:kind :unresolved}

      :else
      (let [prior-cleanup (absent-cleanup-state handle facts)]
        (cond
          (= :already-removed (:kind prior-cleanup))
          {:kind :already-removed}

          (= :partial (:kind prior-cleanup))
          (do
            (println (str "[worktrees] PARTIAL cleanup " subject
                          " — " (:reason prior-cleanup)))
            {:kind :partial})

          :else
          (let [provenance (validate-provenance handle facts)]
            (if-not (:ok? provenance)
              (do
                (println (str "[worktrees] KEEP " subject " — " (:reason provenance)))
                {:kind :uncertain})
              (let [status (worktree-status (:worktree provenance))]
                (case (:kind status)
                  :uncertain
                  (do
                    (println (str "[worktrees] KEEP " subject " — " (:reason status)))
                    {:kind :uncertain})

                  :dirty
                  (let [value (orphan-fact (:worktree provenance) (:branch provenance))
                        wrote? (and (not dry?)
                                    (ensure-orphan-fact! port subject value))]
                    (println (str "[worktrees] " (if dry? "WOULD KEEP" "KEPT")
                                  " dirty " (:worktree provenance)
                                  (when (and (= :clone (:mode provenance))
                                             (not (:harvested? provenance)))
                                    " — dirty clone has unharvested commits; manual salvage required")))
                    {:kind :dirty :orphan-written? wrote?})

                  :clean
                  (cond
                    (and (= :clone (:mode provenance)) (not (:harvested? provenance)))
                    (do
                      (println (str "[worktrees] KEEP " subject
                                    " — clean clone has unharvested commits; canonical lane ref does not match and head is not in main"))
                      {:kind :uncertain})

                    dry?
                    (do
                      (println (str "[worktrees] WOULD REMOVE clean "
                                    (:worktree provenance)))
                      {:kind :would-remove})

                    :else
                    (let [removed (if (= :clone (:mode provenance))
                                    (remove-clean-clone! provenance)
                                    (remove-clean-worktree! provenance))]
                      (case (:kind removed)
                        :removed
                        (do
                          (println (str "[worktrees] removed clean "
                                        (:worktree provenance)
                                        (when (= :linked (:mode provenance))
                                          (str " and " (:branch provenance)))))
                          {:kind :removed})

                        :partial
                        (do
                          (println (str "[worktrees] PARTIAL cleanup " subject
                                        " — " (:reason removed)))
                          {:kind :partial})

                        (do
                          (println (str "[worktrees] KEEP " subject
                                        " — " (:reason removed)))
                          {:kind :uncertain})))))))))))))

;; ---- UNREGISTERED wt-* siblings ---------------------------------------------
;; Same non-force discipline as the lane sweep above, for trees no fact claims.
;; Dirty, unmerged, claimed, or live-concern-owned trees are never removed.

(defn registered-worktree-paths
  "Every worktree path the graph claims, whatever the subject kind. A claim of any
   kind means another owner, so this sweep leaves it alone."
  [log-path]
  (set (keys (north.worktree-census/claimed-worktrees log-path))))

(defn- validate-unregistered-provenance
  "Prove against Git alone that this path is a linked `wt-` worktree of exactly
   `root`, on its own non-baseline branch. The census supplies candidates; only
   this function grants deletion authority."
  [{:keys [root container]} base row]
  (try
    (let [worktree (registered-path (:worktree row))
          branch (:branch row)]
      (cond
        (or (nil? worktree) (= worktree root))
        {:ok? false :reason "worktree path is absent, relative, or the main checkout"}

        (not (str/starts-with? (.getName (io/file worktree))
                               north.worktree-census/worktree-leaf-prefix))
        {:ok? false :reason "worktree leaf is not a wt- sibling"}

        (not= container (.getCanonicalPath (.getParentFile (io/file worktree))))
        {:ok? false :reason "worktree is not a sibling of the repository's main checkout"}

        (or (nil? branch) (:detached row) (= branch base))
        {:ok? false :reason "worktree has no branch of its own"}

        (not (zero? (:exit (git "check-ref-format" "--branch" branch))))
        {:ok? false :reason "branch name is not a valid git ref"}

        (not (.isDirectory (io/file worktree)))
        {:ok? false :reason "worktree path is not a directory"}

        :else
        (let [root-common (canonical-git-path
                           (git "-C" root "rev-parse"
                                "--path-format=absolute" "--git-common-dir"))
              wt-common (canonical-git-path
                         (git "-C" worktree "rev-parse"
                              "--path-format=absolute" "--git-common-dir"))
              root-dot-git (.getCanonicalPath (io/file root ".git"))
              wt-top (canonical-git-path
                      (git "-C" worktree "rev-parse" "--show-toplevel"))
              actual-branch (git-output
                             (git "-C" worktree "symbolic-ref" "--quiet"
                                  "--short" "HEAD"))
              branch-head (git-output
                           (git "-C" root "rev-parse" (branch-ref branch)))
              wt-head (git-output (git "-C" worktree "rev-parse" "HEAD"))]
          (if (and (= root-common wt-common root-dot-git)
                   (= worktree wt-top)
                   (= branch actual-branch)
                   (= true (worktree-registered? root worktree))
                   (= true (branch-present? root branch))
                   (safe-string? wt-head 128)
                   (= branch-head wt-head))
            {:ok? true :root root :worktree worktree :branch branch}
            {:ok? false
             :reason "real Git provenance does not match a linked worktree of this repository"}))))
    (catch Throwable error
      {:ok? false
       :reason (str "Git provenance probe failed: "
                    (or (.getMessage error) (.getName (class error))))})))

(defn- unregistered-review-reason [row]
  (cond
    (:foreign row) "not a registered git worktree (separate clone or plain directory)"
    (not (:dirty_known row)) "git status could not be read"
    (not (:clean row)) (str "dirty (" (:dirty_tracked row) " tracked, "
                            (:dirty_untracked row) " untracked)")
    (:detached row) "detached HEAD"
    (not (true? (:merged row))) (str "unmerged (" (:ahead row)
                                     " commits not in " (:base row) ")")
    :else "unclassified"))

(defn- sweep-unregistered-row!
  [dry? claimed live-concern-repos repo-entry base row]
  (let [path (:worktree row)
        age (north.worktree-census/human-age (:age_ms row))]
    (cond
      (contains? (force claimed) (registered-path path))
      {:kind :claimed}

      (not (north.worktree-census/stale? row))
      {:kind :fresh}

      (not (north.worktree-census/reapable? row))
      (do
        (println (str "[worktrees] REVIEW unregistered " path
                      " — idle " age ", " (unregistered-review-reason row)
                      "; never auto-removed"))
        {:kind :review})

      (contains? (force live-concern-repos) (:container repo-entry))
      (do
        (println (str "[worktrees] KEEP unregistered " path
                      " — idle " age " but its repository has a live concern"))
        {:kind :live-concern})

      :else
      (let [provenance (validate-unregistered-provenance repo-entry base row)]
        (cond
          (not (:ok? provenance))
          (do
            (println (str "[worktrees] KEEP unregistered " path
                          " — " (:reason provenance)))
            {:kind :uncertain})

          ;; The census read status moments ago; the mutation gate re-reads it.
          (not= :clean (:kind (worktree-status (:worktree provenance))))
          (do
            (println (str "[worktrees] KEEP unregistered " path
                          " — status changed or became unreadable before removal"))
            {:kind :uncertain})

          dry?
          (do
            (println (str "[worktrees] WOULD REMOVE unregistered " path
                          " — idle " age ", merged into " base ", clean"))
            {:kind :would-remove})

          :else
          (let [removed (remove-clean-worktree! provenance)]
            (case (:kind removed)
              :removed
              (do
                (println (str "[worktrees] removed unregistered " path
                              " and " (:branch provenance)))
                {:kind :removed})

              :partial
              (do
                (println (str "[worktrees] PARTIAL cleanup " path
                              " — " (:reason removed)))
                {:kind :partial})

              (do
                (println (str "[worktrees] KEEP unregistered " path
                              " — " (:reason removed)))
                {:kind :uncertain}))))))))

(defn- unregistered-zero-result []
  {:scanned 0 :claimed 0 :fresh 0 :review 0 :live-concern 0
   :uncertain 0 :partial 0 :removed 0 :would-remove 0 :errors 0})

(defn- container-selected? [repo-filter {:keys [repo container root]}]
  (or (nil? repo-filter)
      (= repo-filter repo)
      (contains? #{container root} (registered-path repo-filter))))

(defn sweep-unregistered-worktrees!
  "Reclaim `wt-` siblings no fact claims. Both graph joins arrive as deferred
   values the reactor supplies, so the sweep pays for them only when a tree
   actually reaches that gate — and can be driven with no live daemon."
  [{:keys [dry? repo-filter claimed-worktrees live-concern-repos]}]
  (when-not (and (delay? claimed-worktrees) (delay? live-concern-repos))
    (throw (ex-info "unregistered sweep requires the reactor's deferred graph joins" {})))
  (reduce
   (fn [result repo-entry]
     (let [base (north.worktree-census/main-branch (:root repo-entry))
           rows (north.worktree-census/repo-rows repo-entry)]
       (reduce
        (fn [result row]
          (let [action (sweep-unregistered-row!
                        dry? claimed-worktrees live-concern-repos
                        repo-entry base row)]
            (-> result (bump :scanned) (bump (:kind action)))))
        result
        rows)))
   (unregistered-zero-result)
   (filter #(container-selected? repo-filter %)
           (north.worktree-census/containers))))

(defn sweep-worktrees!
  "Inspect registered lane worktrees and reclaim only a canonically terminal,
   provenance-valid, status-clean tree on its derived branch. `lane-resolved?`
   is the reactor's canonical full lane-terminal/committed-run join."
  [{:keys [port dry? lane-resolved? repo-filter]}]
  (when-not (fn? lane-resolved?)
    (throw (ex-info "worktree janitor requires the reactor's canonical lane resolver" {})))
  (let [subjects (sort
                  (distinct
                   (q-col port [{:rel "triple"
                                 :args [{:var "e"} "kind" "lane"]}
                                {:rel "triple"
                                 :args [{:var "e"} "worktree" {:var "_w"}]}])))]
    (reduce
     (fn [result subject]
       (let [action (sweep-subject!
                     port dry? lane-resolved? repo-filter subject)
             result (bump result :scanned)
             result (if (= :skipped (:kind action))
                      result
                      (bump result (:kind action)))]
         (cond-> result
           (:orphan-written? action) (bump :orphan-facts-written))))
     (zero-result)
     subjects)))
