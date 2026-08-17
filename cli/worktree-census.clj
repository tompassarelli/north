;; Shared read model behind `north worktrees` and the scheduled unregistered
;; sweep. Library, not a command; filesystem state derives from Git at read time,
;; while graph ownership comes from Beagle Store's indexed coordination API.
(ns north.worktree-census
  (:require [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

;; Directory = lifecycle policy. `<container>/worktrees/` holds the ephemeral
;; lanes this census reports and the janitor may reclaim; `<container>/pins/`
;; holds externally-consumed checkouts that automation never touches. The policy
;; is carried by the PARENT DIRECTORY, not by any leaf name — which is why the
;; pins exclusion here is structural: nothing under `pins/` is ever listed, so
;; no downstream filter has to remember to exclude it.
(def worktrees-dir-name "worktrees")
(def pins-dir-name "pins")
;; Confidential and read-only trees stay out of the census by rule, not by the
;; accident of carrying no <name>/main checkout.
(def excluded-container-names #{"client" "resources"})
(def stale-age-ms (* 48 60 60 1000))
(def ^:private max-path-chars 4096)
(def ^:private max-worktree-claims 4096)

(defn- git-bin [] (or (System/getenv "NORTH_GIT_BIN") "git"))

(defn git [& args]
  (apply proc/shell
         {:out :string :err :string :continue true}
         (git-bin) args))

(defn- git-out [result]
  (when (zero? (:exit result)) (str/trim (str (:out result)))))

(defn canonical
  "Absolute canonical path, or nil for anything that cannot name one."
  [path]
  (when (and (string? path)
             (not (str/blank? path))
             (<= (count path) max-path-chars)
             (not (re-find #"[\x00\r\n]" path)))
    (let [expanded (if (str/starts-with? path "~/")
                     (when-let [home (System/getenv "HOME")]
                       (str (str/replace home #"/+$" "") (subs path 1)))
                     path)]
      (when expanded
        (try
          (let [file (io/file expanded)]
            (when (.isAbsolute file) (.getCanonicalPath file)))
          (catch Throwable _ nil))))))

(defn roots
  "Census roots: NORTH_WORKTREE_ROOTS (colon-separated) or ~/code."
  []
  (let [raw (some-> (System/getenv "NORTH_WORKTREE_ROOTS") str/trim not-empty)]
    (->> (if raw
           (str/split raw #":")
           [(str (System/getenv "HOME") "/code")])
         (keep canonical)
         distinct
         vec)))

(defn containers
  "Container-layout repositories under `roots`: <root>/<repo>/main is a checkout,
   and every lane of that repo lives under <root>/<repo>/worktrees/."
  ([] (containers (roots)))
  ([roots]
   (vec
    (for [root roots
          ^java.io.File dir (sort-by #(.getName ^java.io.File %)
                                     (or (seq (.listFiles (io/file root))) []))
          :when (and (.isDirectory dir)
                     (not (contains? excluded-container-names (.getName dir)))
                     (.exists (io/file dir "main" ".git")))]
      {:repo (.getName dir)
       :container (.getCanonicalPath dir)
       :root (.getCanonicalPath (io/file dir "main"))}))))

(defn claimed-worktrees
  "Canonical worktree path -> the subject whose live coordination fact claims it."
  [port]
  (into {}
        (keep (fn [[subject value]]
                (when-let [path (canonical value)]
                  [path subject])))
        (:rows
         (north.coord/bounded-query-in-domain
          port
          :coordination
          {:find "worktree_claim"
           :rules
           [{:head {:rel "worktree_claim"
                    :args [{:var "subject"} {:var "path"}]}
             :body [{:rel "triple"
                     :args [{:var "subject"} "worktree" {:var "path"}]}]}]}
          max-worktree-claims))))

(defn container-index [containers]
  {:by-name (into {} (map (juxt :repo :container)) containers)
   :by-path (into {}
                  (mapcat (fn [{:keys [container root]}]
                            [[container container] [root container]]))
                  containers)})

(defn resolve-container
  "Container a `repo` string names — bare name, container, main checkout, or a
   worktree inside it — or nil. Unrecognized repos join nothing rather than
   guessing a container."
  [index repo]
  (when (string? repo)
    (or (get (:by-name index) repo)
        (when-let [path (canonical repo)]
          (or (get (:by-path index) path)
              (get (:by-path index) (.getParent (io/file path))))))))

(defn main-branch
  "The branch the repo's main checkout is on — the census baseline. A detached
   main checkout has no baseline branch and yields nil."
  [root]
  (git-out (git "-C" root "symbolic-ref" "--quiet" "--short" "HEAD")))

(defn- parse-porcelain [text]
  (->> (str/split (or text "") #"\n\n+")
       (keep
        (fn [block]
          (let [lines (remove str/blank? (str/split-lines (str/trim block)))
                kv (into {}
                         (map (fn [line]
                                (let [i (.indexOf ^String line " ")]
                                  (if (neg? i)
                                    [line true]
                                    [(subs line 0 i) (subs line (inc i))]))))
                         lines)]
            (when-let [path (get kv "worktree")]
              {:path path
               :head (let [head (get kv "HEAD")] (when (string? head) head))
               :branch (let [branch (get kv "branch")]
                         (when (string? branch)
                           (str/replace branch #"^refs/heads/" "")))
               :detached? (contains? kv "detached")
               :locked? (contains? kv "locked")
               :prunable? (contains? kv "prunable")}))))
       vec))

(defn registered-worktrees
  "Git's own registration list for a repo, main checkout included."
  [root]
  (if-let [text (git-out (git "-C" root "worktree" "list" "--porcelain"))]
    (parse-porcelain text)
    []))

(defn branch-commit-times
  "branch -> last commit epoch-ms, in one process for the whole repo."
  [root]
  (into {}
        (keep (fn [line]
                (let [[branch stamp] (str/split (str/trim line) #" " 2)]
                  (when-let [seconds (some-> stamp str/trim parse-long)]
                    [branch (* 1000 seconds)]))))
        (str/split-lines
         (or (git-out (git "-C" root "for-each-ref"
                           "--format=%(refname:short) %(committerdate:unix)"
                           "refs/heads"))
             ""))))

(defn status-counts
  "Working-tree dirt, split the way salvage cares about it. `:known? false` when
   Git could not answer — an unreadable status is never reported as clean."
  [path]
  (let [result (git "-C" path "status" "--porcelain=v1" "--untracked-files=all")]
    (if-not (zero? (:exit result))
      {:known? false :tracked nil :untracked nil}
      (let [lines (remove str/blank? (str/split-lines (str (:out result))))]
        {:known? true
         :tracked (count (remove #(str/starts-with? % "??") lines))
         :untracked (count (filter #(str/starts-with? % "??") lines))}))))

(defn divergence
  "Commits on each side of the repo's baseline branch. `:ahead 0` is exactly the
   merged condition Git's own non-force `branch -d` enforces, so the census and
   the janitor read one number rather than two independent notions of merged."
  [root base head]
  (when (and base head)
    (when-let [text (git-out (git "-C" root "rev-list" "--left-right" "--count"
                                  (str "refs/heads/" base "..." head)))]
      (let [[behind ahead] (map parse-long (str/split (str/trim text) #"\s+"))]
        (when (and behind ahead)
          {:behind behind :ahead ahead :merged? (zero? ahead)})))))

(defn- linked-git-dir
  "A linked worktree's private git dir, read from its `.git` pointer file with no
   subprocess."
  [path]
  (let [pointer (io/file path ".git")]
    (when (.isFile pointer)
      (some-> (second (re-find #"(?m)^gitdir:\s*(.+)$" (slurp pointer)))
              str/trim
              canonical))))

(defn last-write-ms
  "Newest activity trace Git and the filesystem leave for this worktree: a MAX over
   independent traces, so any recent signal keeps a tree young — the error direction
   that never ages a live tree into reap range."
  [path commit-ms]
  (let [git-dir (linked-git-dir path)
        ;; The index and its directory are excluded on purpose: `git status`
        ;; rewrites them, so censusing a tree would reset its own age.
        stamps [(.lastModified (io/file path))
                (when git-dir (.lastModified (io/file git-dir "logs" "HEAD")))
                commit-ms]]
    (apply max 0 (remove nil? stamps))))

(defn census-row
  "One worktree's full derived state. `commit-times` is the repo-wide branch ->
   commit-ms map so this costs two processes, not four."
  [{:keys [repo container root]} base commit-times entry]
  (let [path (:path entry)
        leaf (.getName (io/file path))
        status (status-counts path)
        divergence (divergence root base (:head entry))
        commit-ms (get commit-times (:branch entry))
        written (last-write-ms path commit-ms)]
    {:repo repo
     :container container
     :root root
     :worktree path
     :name leaf
     :branch (:branch entry)
     :detached (boolean (:detached? entry))
     :locked (boolean (:locked? entry))
     :prunable (boolean (:prunable? entry))
     :head (:head entry)
     :base base
     :ahead (:ahead divergence)
     :behind (:behind divergence)
     :merged (:merged? divergence)
     :dirty_known (:known? status)
     :dirty_tracked (:tracked status)
     :dirty_untracked (:untracked status)
     :clean (and (:known? status)
                 (zero? (+ (:tracked status) (:untracked status))))
     :last_write_ms written
     :age_ms (max 0 (- (System/currentTimeMillis) written))}))

(defn foreign-row
  "A tree under <container>/worktrees/ Git does not register: a separate clone
   (north-exec provisions one deliberately) or a plain directory. It is REPORTED
   and never probed further — nothing here may be reclaimed, and the census must
   not silently omit a tree just because Git disowns it."
  [{:keys [repo container root]} base path]
  {:repo repo
   :container container
   :root root
   :worktree (.getCanonicalPath (io/file path))
   :name (.getName (io/file path))
   :foreign true
   :branch nil
   :detached false
   :locked false
   :prunable false
   :head nil
   :base base
   :ahead nil
   :behind nil
   :merged nil
   :dirty_known false
   :dirty_tracked nil
   :dirty_untracked nil
   :clean nil
   :last_write_ms (.lastModified (io/file path))
   :age_ms (max 0 (- (System/currentTimeMillis) (.lastModified (io/file path))))})

(defn worktrees-dir
  "The one directory a container's lanes live in."
  [container]
  (io/file container worktrees-dir-name))

(defn in-worktrees?
  "Is PATH a direct child of <container>/worktrees? A POSITIONAL test — the
   parent directory, never a leaf name — so `pins/` and every other container
   sibling are excluded by construction rather than by a filter that could be
   dropped in a refactor."
  [container path]
  (try
    (= (.getCanonicalPath (worktrees-dir container))
       (some-> (.getParentFile (io/file path)) .getCanonicalPath))
    (catch Throwable _ false)))

(defn repo-rows
  "Every tree under one repo's <container>/worktrees/, censused. Nothing under
   <container>/pins/ is enumerated: both scans below read `worktrees/` and only
   `worktrees/`, so a pin cannot reach a candidate list at all. If either
   listing is ever widened back to the container, <container>/pins must be
   excluded explicitly."
  [{:keys [container root] :as repo-entry}]
  (let [base (main-branch root)
        commit-times (branch-commit-times root)
        entries (->> (registered-worktrees root)
                     (remove #(= (canonical (:path %)) root))
                     (filter #(in-worktrees? container (:path %))))
        known (into #{} (keep #(canonical (:path %))) entries)
        ;; The foreign scan is the ONLY thing that sees north-exec's clones —
        ;; a clone never appears in `git worktree list`.
        foreign (->> (or (seq (.listFiles (worktrees-dir container))) [])
                     (filter #(and (.isDirectory ^java.io.File %)
                                   (not (contains? known (.getCanonicalPath ^java.io.File %)))))
                     (map #(foreign-row repo-entry base %)))]
    (vec (concat (pmap #(census-row repo-entry base commit-times %) entries)
                 foreign))))

(defn census
  "Every censused worktree on the machine, repo-major. `repo-filter` is an exact
   container name."
  [{:keys [repo-filter]}]
  (let [selected (cond->> (containers)
                   repo-filter (filter #(= repo-filter (:repo %))))]
    (vec (mapcat repo-rows selected))))

(defn stale?
  "Idle past the census staleness horizon."
  [row] (>= (or (:age_ms row) 0) stale-age-ms))

(defn reapable?
  "The exact shape the janitor may reclaim WITHOUT force: a branch-attached,
   proven-merged, provably clean tree UNDER <container>/worktrees/ that has been
   idle past the horizon. Nothing under `pins/` reaches this predicate — the
   census never enumerates one — and that structural exclusion, not the
   `(not (:detached row))` clause below, is what makes a pin safe. Liveness
   joins (graph lane registration, live concerns) are decided by the caller —
   this predicate is only the filesystem-and-Git half."
  [row]
  (boolean
   (and (:branch row)
        (not (:foreign row))
        (not (:detached row))
        (not (:locked row))
        (true? (:merged row))
        (true? (:dirty_known row))
        (true? (:clean row))
        (stale? row))))

(defn human-age [ms]
  (let [seconds (long (/ (or ms 0) 1000))
        minutes (long (/ seconds 60))
        hours (long (/ minutes 60))
        days (long (/ hours 24))]
    (cond
      (>= days 1) (str days "d")
      (>= hours 1) (str hours "h")
      (>= minutes 1) (str minutes "m")
      :else (str seconds "s"))))
