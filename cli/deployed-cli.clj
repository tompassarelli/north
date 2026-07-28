#!/usr/bin/env bb
;; north deployed — is the code I committed actually the code that is running?
;;
;; WHY THIS EXISTS. Every expensive failure on 2026-07-29 was one question with
;; no cheap answer:
;;   - the CLI was believed unusably slow for DAYS; it was one hook, already
;;     fixed in source, never deployed.
;;   - three rebuilds silently landed nothing (a derivation aborted), and the
;;     old binary kept running with no error.
;;   - `firn rebuild` reported FAILURE after the build had already succeeded,
;;     because the coordinator had not adopted the new engine.
;;   - a measured 364x fix sat built-and-unused because nothing restarted the
;;     daemon.
;; In each case work was debugged that had never shipped. The defect is not
;; performance; it is that "did my change reach the running system?" took hours
;; to answer. This answers it in one screen.
;;
;; THREE COLUMNS, because there are three places a change gets stuck:
;;   SOURCE   what is committed on the repo's main
;;   BUILT    what the last rebuild actually locked into the system closure
;;   RUNNING  what the live process is serving right now
;; SOURCE≠BUILT means "rebuild". BUILT≠RUNNING means "paired cutover". Merging
;; them hides which one you need, and they need opposite actions.
(require '[clojure.string :as str]
         '[clojure.java.io :as io]
         '[babashka.process :refer [shell]]
         '[cheshire.core :as json])

(def HOME (System/getenv "HOME"))
(def CODE (str HOME "/code"))
(def NIXOS (str CODE "/nixos-config"))
(def SYSTEM-PROFILE "/nix/var/nix/profiles/system")
(def COORDINATOR-CUTOVER-COMMAND "sudo north-coord-runtime restart")

(def use-color? (and (nil? (System/getenv "NO_COLOR"))
                     (nil? (System/getenv "NORTH_NO_COLOR"))))
(defn- c [code s] (if use-color? (str "\033[" code "m" s "\033[0m") (str s)))
(defn- dim [s] (c "2" s))
(defn- bold [s] (c "1" s))
(defn- grn [s] (c "32" s))
(defn- red [s] (c "31" s))
(defn- ylw [s] (c "33" s))

(defn- sh
  "Run a command, returning trimmed stdout or nil. Never throws: this is a
  diagnostic, and a diagnostic that dies on a missing repo is useless exactly
  when something is wrong."
  [& args]
  (try
    (let [{:keys [out exit]} (apply shell {:out :string :err :string
                                           :continue true} args)]
      (when (zero? exit) (str/trim out)))
    (catch Throwable _ nil)))

(defn- sh-any
  "Like `sh` but returns stdout+stderr regardless of exit status. Required for
  health probes: `north coord-doctor` exits 1 AND writes to stderr precisely
  when the runtime is unhealthy, which is the case this table exists to catch.
  Reading only successful stdout made an unhealthy coordinator indistinguishable
  from a missing one, and rendered it as a green row."
  [& args]
  (try
    (let [{:keys [out err]} (apply shell {:out :string :err :string
                                          :continue true} args)]
      (str/trim (str out "\n" err)))
    (catch Throwable _ nil)))

(defn- short-rev [rev] (when rev (subs rev 0 (min 8 (count rev)))))

(defn- source-rev [repo]
  (sh "git" "-C" (str CODE "/" repo) "rev-parse" "refs/heads/main"))

(defn- commits-between
  "How many commits `to` is ahead of `from`, or nil when unanswerable."
  [repo from to]
  (when (and from to (not= from to))
    (some-> (sh "git" "-C" (str CODE "/" repo) "rev-list" "--count"
                (str from ".." to))
            parse-long)))

(defn- locked-revs
  "The revision each local input was pinned to by the last rebuild. This is the
  BUILT column: flake.lock is what the pipeline resolved and handed to Nix."
  []
  (try
    (let [lock (json/parse-string (slurp (str NIXOS "/flake.lock")) true)]
      (into {}
            (keep (fn [[k v]]
                    (let [l (:locked v)]
                      (when (and (= "git" (:type l))
                                 (str/includes? (str (:url l)) "file://"))
                        [(name k) (:rev l)])))
                  (:nodes lock))))
    (catch Throwable _ {})))

;; RUNNING is per-component, because each one exposes its identity differently.
;; Absent a real answer the column stays "-" rather than guessing; a fabricated
;; match here would defeat the entire purpose of the table.
(defn- system-fram-rev
  "The Fram revision THIS GENERATION was built with, read out of the switched
  system rather than flake.lock. The lock records what the last rebuild
  RESOLVED; it is updated even when the build then fails, so it can sit ahead of
  what actually got built. /run/current-system is the closure that is really
  installed."
  []
  (some->> (sh "grep" "-rho" "NORTH_COORD_FRAM_PACKAGE_REV=[0-9a-f]*"
               "/run/current-system/sw/bin/north-coord-runtime")
           str/split-lines
           first
           (re-find #"[0-9a-f]{40}")))

(defn- running-fram []
  ;; Deployment identity is the cheap question answered by coord-ready. The
  ;; former coord-doctor call ran the entire health/hygiene sweep just to read
  ;; one hash and could exceed 40 seconds on an otherwise healthy system.
  (some->> (sh-any "north" "coord-ready")
           (re-find #"rev[:=]\s*([0-9a-f]{40})")
           second))

(defn- running-north []
  ;; The CLI on PATH resolves into the store; the generation's own north is what
  ;; the closure provides. Equal store paths => the shell is running the system's
  ;; north rather than a stale profile or a checkout wrapper.
  (let [on-path (sh "readlink" "-f" (or (sh "bash" "-c" "command -v north") ""))
        sys (sh "readlink" "-f" "/run/current-system/sw/bin/north")]
    (when (and on-path sys) [on-path sys])))

(defn- row [component source built running-rev note]
  {:component component :source source :built built
   :running running-rev :note note})

(defn generation-link
  "Persistent `system-N-link` selected by the system profile, or nil.

  `/run/current-system` lives on tmpfs and is recreated at boot, so its mtime
  can make an old generation look freshly deployed. The generation link under
  `/nix/var/nix/profiles` is durable and keeps the original switch provenance."
  []
  (when-let [target (sh "readlink" SYSTEM-PROFILE)]
    (let [target-path (.toPath (io/file target))]
      (str
       (if (.isAbsolute target-path)
         (.normalize target-path)
         (.normalize
          (.resolve (.toPath (.getParentFile (io/file SYSTEM-PROFILE)))
                    target)))))))

(defn generation-built-epoch
  "When the selected persistent system generation was created, unix seconds,
  or nil. This remains a labelled time heuristic because the closure does not
  yet embed the nixos-config Git revision."
  []
  (when-let [link (generation-link)]
    (some-> (sh "stat" "-c" "%Y" link) str/trim parse-long)))

(defn commits-since-epoch
  "Commits on `repo`'s main strictly newer than `epoch`, or nil.

  nixos-config carries no revision into the closure — /run/current-system knows
  its NIXPKGS rev, not which config commit produced it — so unlike the other
  components this is a TIME comparison rather than a revision match. A heuristic,
  labelled as one, but it catches the case that matters.

  Observed 2026-07-29: the coordinator's -Xmx16g fix was committed at 06:40, the
  running generation was built at 06:33, and the daemon kept exhausting a 6 GB
  heap every 8 minutes with the fix sitting undeployed. Nothing surfaced that."
  [repo epoch]
  (when epoch
    (some-> (sh "git" "-C" (str CODE "/" repo) "rev-list" "--count"
                ;; Git's time boundary is inclusive at one-second resolution.
                ;; Advancing it prevents the commit that produced the selected
                ;; generation from counting itself as pending.
                (str "--since=@" (inc epoch)) "refs/heads/main")
            str/trim
            parse-long)))

(defn nixos-config-assessment
  "First-class deployment row for nixos-config's labelled time heuristic.
  Missing evidence is unknown/failing; it can never silently become green."
  []
  (let [epoch (generation-built-epoch)
        pending (commits-since-epoch "nixos-config" epoch)
        verdict
        (cond
          (or (nil? epoch) (nil? pending))
          [:unknown (ylw "?")
           "cannot determine — persistent generation provenance unavailable"]

          (pos? pending)
          [:stale-build (red "✗")
           (str "rebuild — " pending
                " commit(s) newer than generation (time-based)")]

          :else
          [:live (grn "✓") "live (time-based generation-link evidence)"])]
    {:component "nixos-config"
     :source nil
     :built nil
     :running nil
     :pending pending
     :generation_epoch epoch
     :evidence "persistent-generation-link-time"
     :note "time-based heuristic; the closure does not embed nixos-config rev"
     :verdict verdict}))

(defn- verdict
  "SOURCE≠BUILT means rebuild; BUILT≠RUNNING means paired cutover; they need
  opposite actions, so they are never merged.

  `expect-running?` marks a component whose live revision is discoverable. For
  those, an ABSENT running value is `unknown`, never `live` — the first version
  of this reported a green row for fram while the daemon was three commits
  behind, because a failed probe read as no-disagreement. Absence of evidence
  is the one thing this table must never render as health."
  [{:keys [component source built running expect-running?]}]
  (cond
    (or (nil? source) (nil? built))
    [:unknown (ylw "?") "cannot determine — missing revision"]

    (not= source built)
    (let [n (commits-between component built source)]
      [:stale-build (red "✗") (str "rebuild — " (or n "?") " commit(s) not built")])

    (and expect-running? (nil? running))
    [:unknown (ylw "?") "cannot determine — live revision unreadable"]

    (and running (not= built running))
    (let [n (commits-between component running built)]
      [:stale-process (red "✗")
       (str "paired cutover — process " (or n "?") " commit(s) behind the closure")])

    :else
    [:live (grn "✓") "live"]))

(defn deployment-report []
  (let [locked (locked-revs)
        fram-running (running-fram)
        sys-fram (system-fram-rev)
        rows (for [repo ["north" "fram" "beagle"]]
               ;; fram is the one component carrying its revision INTO the
               ;; switched system and INTO the live process, so it gets the
               ;; authoritative build source; the others fall back to the lock.
               (let [src (source-rev repo)
                     blt (if (= repo "fram") (or sys-fram (get locked repo))
                             (get locked repo))
                     run (when (= repo "fram") fram-running)]
                 (assoc (row repo src blt run nil)
                        :expect-running? (= repo "fram"))))
        judged (conj (mapv (fn [r] (assoc r :verdict (verdict r))) rows)
                      (nixos-config-assessment))
        worst (if (some #(#{:stale-build :stale-process :unknown}
                          (first (:verdict %)))
                        judged)
                1 0)]
    {:judged judged :worst worst}))

(defn json-rows [judged]
  (mapv (fn [r] (-> r
                    (assoc :status (name (first (:verdict r))))
                    (dissoc :verdict)))
        judged))

(defn -main [& args]
  (let [json? (some #{"--json"} args)
        {:keys [judged worst]} (deployment-report)]
    (if json?
      (println (json/generate-string (json-rows judged)))
      (do
        (println (bold "north deployed") (dim "— committed vs built vs running"))
        (println)
        (println (format "  %-13s %-10s %-10s %-10s %s"
                         "COMPONENT" "SOURCE" "BUILT" "RUNNING" "VERDICT"))
        (doseq [{:keys [component source built running] :as r} judged]
          (let [[_ mark text] (:verdict r)]
            (println (format "  %-13s %-10s %-10s %-10s %s %s"
                             component
                             (or (short-rev source) "-")
                             (or (short-rev built) "-")
                             (or (short-rev running) "-")
                             mark text))))
        (println)
        ;; The two states need opposite actions, so name the action rather than
        ;; leaving the reader to infer it from three hashes.
        (when (some #(= :stale-build (first (:verdict %))) judged)
          (println (str "  " (ylw "→") " commits are not in the system closure. Rebuild:"))
          (println (dim "      firn-rebuild-coordinated --why \"<reason>\"")))
        (when (some #(= :stale-process (first (:verdict %))) judged)
          (println (str "  " (ylw "→")
                        " the closure is newer than the live process. Paired cutover:"))
          (println (dim (str "      " COORDINATOR-CUTOVER-COMMAND))))
        (when (zero? worst)
          (println (str "  " (grn "everything committed is built and running."))))
        (let [[on-path sys] (or (running-north) [nil nil])]
          (when (and on-path sys (not= on-path sys))
            (println)
            (println (str "  " (ylw "note") " the `north` on your PATH is not the system's:"))
            (println (dim (str "      PATH:   " on-path)))
            (println (dim (str "      system: " sys)))))))
    (System/exit worst)))

;; Guarded so tests can load this file and exercise `verdict` directly. Without
;; it, requiring the namespace runs the whole report and exits the test process.
(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
