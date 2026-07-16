#!/usr/bin/env bb
;; log-split.clj — Phase-2 replay-filter compaction for the log split.
;;
;; Reads a single coordination log and partitions every line into two per-log
;; files by log-for(subject), preserving each line's original :tx (lines are
;; copied VERBATIM — no re-serialization — so :tx and byte-shape are exact):
;;   coordination.log  the human-meaningful work graph + everything a live
;;                     consumer subscribes to (threads, concern, @agent, @lease,
;;                     @cmd, @swarm, un-kinded legacy)
;;   telemetry.log     high-volume machine output (run/session/mine/guard_denial)
;;
;; INVARIANT: line-count conservation — coord N + telem M == total T, dropped==0.
;; Every input line routes to EXACTLY one log (an unparseable line routes to
;; coordination, the engine's safe default), so nothing is ever lost.
;;
;; This is the SAME contract the coordinator routes writes by (fram
;; coord_daemon.clj log-for/load-log-routing!) — consumed verbatim, NOT
;; re-derived: telemetry allow-list of KIND names, PRIMARY = a subject's stored
;; `kind` fact, FALLBACK = the structural @<token> prefix for kind-less subjects
;; (promotes @session, the biggest telemetry mass, which carries no kind fact).
;;
;;   north-log-split [--dry-run|--execute] [--log PATH] [--coord PATH]
;;                   [--telemetry PATH] [--force]
;;
;;   --dry-run (default)  print `coord N / telem M / total T`, assert the
;;                        invariant, write NOTHING.
;;   --execute            write coord + telemetry files (atomic tmp+rename);
;;                        refuses to clobber existing outputs unless --force.
;; The input log is READ-ONLY (it is the rollback pre-image); this tool never
;; mutates it.

(require '[clojure.string :as str]
         '[clojure.edn :as edn])

;; ---- the shared log-for contract (mirror of coord_daemon.clj) ---------------

(def DEFAULT-TELEMETRY-KINDS #{"run" "session" "mine" "guard_denial"})

;; @<token>:… → token between @ and the first colon, ONLY when a colon is present
;; (bare @log-routing / @run without a colon → nil). Exact mirror of subject-token.
(defn subject-token [s]
  (when (and (string? s) (> (count s) 1) (= \@ (.charAt ^String s 0)))
    (let [c (.indexOf ^String s ":")]
      (when (pos? c) (subs s 1 c)))))

;; log-for: telemetry iff the subject's kind — or, kind-less, its @token — is in
;; the allow-list; everything else (incl. a nil subject) → coordination.
(defn log-for [kindmap allow subject]
  (let [k (or (get kindmap subject) (subject-token subject))]
    (if (contains? allow k) :telemetry :coordination)))

;; best-effort EDN parse of one flat line; nil on a torn/partial line.
(defn parse-line [ln]
  (try (edn/read-string ln) (catch Exception _ nil)))

;; subject `kind` map, folded in :tx order so latest-wins matches the warm store
;; (kind is single-valued: assert sets, retract clears).
(defn build-kindmap [parsed]
  (->> parsed
       (keep :m)
       (filter #(= "kind" (:p %)))
       (sort-by #(or (:tx %) 0))
       ;; fram is a triple store: a single-valued retract removes the (s,p,r)
       ;; triple ONLY when r matches the live value — retracting an already-
       ;; superseded value is a no-op. Value-matched dissoc mirrors that (a
       ;; blind dissoc would wrongly clear a kind the store still holds).
       (reduce (fn [m op]
                 (if (= "retract" (:op op))
                   (if (= (get m (:l op)) (:r op)) (dissoc m (:l op)) m)
                   (assoc m (:l op) (:r op))))
               {})))

;; telemetry allow-list: fold @log-routing telemetry_kind facts (multi-valued: assert
;; adds, retract removes) in :tx order; a non-empty result OVERRIDES the default
;; (matches load-log-routing!, which only replaces when `seq` is non-empty).
(defn build-allowlist [parsed]
  (let [ks (->> parsed
                (keep :m)
                (filter #(and (= "@log-routing" (:l %)) (= "telemetry_kind" (:p %))))
                (sort-by #(or (:tx %) 0))
                (reduce (fn [s op]
                          (if (= "retract" (:op op))
                            (disj s (str (:r op)))
                            (conj s (str (:r op)))))
                        #{}))]
    (if (seq ks) ks DEFAULT-TELEMETRY-KINDS)))

;; ---- io ---------------------------------------------------------------------

(defn read-lines [path]
  (when-not (.exists (java.io.File. ^String path))
    (binding [*out* *err*] (println (str "north-log-split: input log not found: " path)))
    (System/exit 2))
  (str/split-lines (slurp path)))

(defn write-atomic! [path lines]
  (let [tmp (str path ".tmp-log-split")]
    (spit tmp (if (seq lines) (str (str/join "\n" lines) "\n") ""))
    (.renameTo (java.io.File. ^String tmp) (java.io.File. ^String path))))

;; ---- arg parse --------------------------------------------------------------

(defn parse-args [args]
  (loop [a args, m {:mode :dry-run}]
    (if (empty? a)
      m
      (let [[k & more] a]
        (case k
          "--dry-run"   (recur more (assoc m :mode :dry-run))
          "--execute"   (recur more (assoc m :mode :execute))
          "--force"     (recur more (assoc m :force true))
          "--log"       (recur (rest more) (assoc m :log (first more)))
          "--coord"     (recur (rest more) (assoc m :coord (first more)))
          "--telemetry" (recur (rest more) (assoc m :telemetry (first more)))
          (do (binding [*out* *err*] (println (str "north-log-split: unknown arg: " k)))
              (System/exit 2)))))))

(defn dir-of [path]
  (let [f (.getParentFile (java.io.File. ^String path))]
    (if f (.getPath f) ".")))

;; ---- main -------------------------------------------------------------------

(let [opts   (parse-args *command-line-args*)
      log    (or (:log opts)
                 (System/getenv "FRAM_LOG")
                 (str (System/getenv "HOME") "/.local/state/north/facts.log"))
      d      (dir-of log)
      coord  (or (:coord opts) (str d "/coordination.log"))
      telem  (or (:telemetry opts) (str d "/telemetry.log"))
      lines  (read-lines log)
      total  (count lines)
      parsed (mapv (fn [ln] {:raw ln :m (parse-line ln)}) lines)
      kindmap (build-kindmap parsed)
      allow   (build-allowlist parsed)
      routed  (reduce (fn [acc {:keys [raw m]}]
                        (update acc (log-for kindmap allow (:l m)) conj raw))
                      {:coordination [] :telemetry []}
                      parsed)
      cl      (:coordination routed)
      tl      (:telemetry routed)
      n       (count cl)
      mm      (count tl)
      dropped (- total (+ n mm))]

  ;; INVARIANT — the whole point of the tool. A non-conserving split is a BUG,
  ;; never a silent partial: abort loudly.
  (when-not (and (= (+ n mm) total) (= dropped 0))
    (binding [*out* *err*]
      (println (str "north-log-split: INVARIANT VIOLATED — coord " n " + telem " mm
                    " = " (+ n mm) " != total " total " (dropped " dropped ")")))
    (System/exit 1))

  (println (str "coord " n " / telem " mm " / total " total))
  (println (str "  invariant OK: " n " + " mm " = " total ", dropped 0"))
  (println (str "  allow-list (telemetry kinds): " (str/join " " (sort allow))
                (if (= allow DEFAULT-TELEMETRY-KINDS) "  (default)" "  (@log-routing override)")))
  (println (str "  input (read-only): " log))

  (case (:mode opts)
    :dry-run
    (do (println (str "  outputs (would write): " coord " · " telem))
        (println "  DRY-RUN — nothing written. Re-run with --execute to write."))

    :execute
    (let [exists (filter #(.exists (java.io.File. ^String %)) [coord telem])]
      (when (and (seq exists) (not (:force opts)))
        (binding [*out* *err*]
          (println (str "north-log-split: refusing to clobber existing output(s): "
                        (str/join " " exists) " — pass --force to overwrite")))
        (System/exit 3))
      (write-atomic! coord cl)
      (write-atomic! telem tl)
      (println (str "  WROTE " coord " (" n " lines) · " telem " (" mm " lines)"))
      (println (str "  input " log " left intact (rollback pre-image)")))))
