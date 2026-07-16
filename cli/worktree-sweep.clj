#!/usr/bin/env bb
;; ============================================================================
;; worktree-sweep.clj <port> [--dry-run] — STANDALONE orphan-worktree reaper.
;;
;; Per-lane git worktrees (sdk/src/worktree.ts) are removed INLINE on a clean `ran`
;; finalize. A crashed / capped / dirty lane KEEPS its worktree (salvage-first — the
;; tree may hold hours of WIP). This sweep is the JANITOR that reaps those, but only
;; once the lane is provably RESOLVED (terminal outcome) AND its tree is provably CLEAN:
;;   - resolved + clean  -> `git worktree remove` + `git branch -d`  (reclaim disk)
;;   - resolved + dirty  -> leave it, append a `worktree_orphaned` fact (+ ping coord)
;;   - not resolved      -> leave it untouched (a live lane owns its tree)
;; It NEVER declares death and NEVER destroys WIP — it prunes what the reaper already
;; marked terminal, mirroring sweep-agent-logs! in north-reactor.clj.
;;
;; Run standalone NOW:
;;   bb cli/worktree-sweep.clj 7977            # reap
;;   bb cli/worktree-sweep.clj 7977 --dry-run  # report what it WOULD reap, touch nothing
;;
;; ----------------------------------------------------------------------------
;; WORKTREE-SWEEP WIRING (owner, on next reactor restart):
;;   north-reactor.clj is a HOT running process — do NOT edit + expect effect until
;;   restart. When you next restart the reactor, wire this reap into its 5-min sweep by
;;   loading this file and adding ONE line to sweep! (north-reactor.clj:302):
;;     (load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/worktree-sweep.clj"))
;;     ;; then inside (defn sweep! [dry?] (let [ ... nw (north.wtsweep/sweep-worktrees! port dry?) ...]))
;;   i.e. add `nw (north.wtsweep/sweep-worktrees! port dry?)` to the sweep! let-bindings and
;;   fold nw into the summary println. The reap logic here is namespaced (north.wtsweep) and
;;   pure-of-argv so the reactor can call sweep-worktrees! directly with its own port + dry?.
;; ============================================================================
(require '[clojure.java.io :as io] '[clojure.string :as str] '[babashka.process :as proc])

;; shared coord substrate + PURE reap decisions — the SAME liveness model the reactor uses.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/reap.clj"))

(ns north.wtsweep
  (:require [clojure.string :as str] [babashka.process :as proc] [clojure.java.io :as io]))

(defn q-col [port body]
  (->> (:ok (north.coord/send-op port {:op :query
              :query {:find "e" :rules [{:head {:rel "e" :args [{:var "e"}]} :body body}]}}))
       (map first)))

(defn strip-sigil [s pfx] (if (str/starts-with? (str s) pfx) (subs (str s) (count pfx)) (str s)))

;; is-this-lane-resolved? — join through `agent <h>` (recordRun lands `outcome` on @run-<h>-<ts>,
;; not on @agent:<h>) + cross-ref @swarm agent_death. Verbatim model of north-reactor's lane-resolved?*.
(defn subjects-tagged-agent [port h]
  (distinct (q-col port [{:rel "triple" :args [{:var "e"} "agent" h]}])))
(defn lane-resolved?* [port h]
  (north.reap/lane-resolved?
    h
    (map #(north.coord/many port % "outcome") (subjects-tagged-agent port h))
    (north.coord/many port "@swarm" "agent_death")))

(defn git [& args]
  (apply proc/shell {:out :string :err :string :continue true} "git" args))

(defn worktree-present? [wt] (.isDirectory (io/file wt)))

;; `git -C <wt> status --porcelain` non-empty => uncommitted changes.
(defn worktree-dirty? [wt]
  (let [{:keys [exit out]} (git "-C" wt "status" "--porcelain")]
    (and (zero? exit) (seq (str/trim (str out))))))

;; main checkout root = parent of the common .git dir (worktrees are removed from there).
(defn main-root [wt]
  (let [{:keys [exit out]} (git "-C" wt "rev-parse" "--path-format=absolute" "--git-common-dir")]
    (when (zero? exit)
      (let [gd (str/trim (str out))]
        (.getCanonicalPath (.getParentFile (io/file gd)))))))

(defn ping-coord [port coord h wt]
  (try
    (proc/shell {:out :string :err :string :continue true}
                "bb" (str (.getParent (io/file (System/getProperty "babashka.file"))) "/msg-cli.clj")
                (str port) "send" "worktree-sweep" coord "URGENT"
                (str "lane " h " worktree kept for salvage (dirty tree): " wt))
    (catch Throwable _ nil)))

;; The reap, namespaced so the reactor can call it directly. Returns a count of trees reaped.
(defn sweep-worktrees! [port dry?]
  (let [lanes (distinct (q-col port [{:rel "triple" :args [{:var "e"} "worktree" {:var "_w"}]}]))
        reaped (atom 0)]
    (doseq [e lanes
            :let [h  (strip-sigil e "@agent:")
                  wt (north.coord/resolved port e "worktree")
                  br (or (north.coord/resolved port e "branch") (str "lane-" h))]]
      (when (and wt (lane-resolved?* port h))            ; the REAPER already judged it terminal
        (cond
          (not (worktree-present? wt))
          (println (str "[wt-sweep] lane " e " worktree already gone: " wt " (skip)"))

          (worktree-dirty? wt)                            ; salvage — never destroy WIP
          (do (when-not dry?
                (north.coord/append! port e "worktree_orphaned"
                  (str wt " | uncommitted changes — manual salvage"))
                (let [coord (or (north.coord/resolved port e "coordinator")
                                (north.coord/resolved port e "supervisor"))]
                  (when (and coord (seq coord)) (ping-coord port coord h wt))))
              (println (str "[wt-sweep] " (if dry? "WOULD KEEP" "KEPT") " dirty worktree " wt
                            " (lane " e ") -> worktree_orphaned")))

          :else                                           ; resolved + clean -> reclaim
          (let [root (main-root wt)]
            (when-not dry?
              (when root
                (git "-C" root "worktree" "remove" wt)    ; plain (no --force) — dirty-refusal is a backstop
                (git "-C" root "branch" "-d" br)))
            (swap! reaped inc)
            (println (str "[wt-sweep] " (if dry? "WOULD REMOVE" "removed") " clean worktree " wt
                          " (lane " e ", branch " br ")"))))))
    (println (str "[wt-sweep] " (count lanes) " worktree-carrying lane(s) scanned, "
                  @reaped (if dry? " reapable" " reaped")))
    @reaped))

;; ---- standalone entrypoint (skipped when loaded as a lib by the reactor) ---------------
;; Fire ONLY when this file IS the invoked script — under load-file, babashka.file is the
;; LOADER's path (the reactor), so `*file*` != babashka.file and the sweep stays dormant.
(when (= (some-> *file* io/file .getCanonicalPath)
         (some-> (System/getProperty "babashka.file") io/file .getCanonicalPath))
  (let [args (vec *command-line-args*)
        flags (set (filter #(str/starts-with? % "--") args))
        pos   (remove #(str/starts-with? % "--") args)
        port  (Integer/parseInt (or (first pos) (System/getenv "FRAM_PORT") "7977"))
        dry?  (contains? flags "--dry-run")]
    (sweep-worktrees! port dry?)))
