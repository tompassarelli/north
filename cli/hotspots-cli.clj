#!/usr/bin/env bb
;; north hotspots — where is the time going, and what is failing, ranked.
;;
;; WHY. On 2026-07-29 answering "what should I fix?" took a full night of manual
;; measurement. Every number that mattered was already being recorded — the
;; coordinator's slow-read attribution, 3,500 agent logs, provider error detail —
;; and none of it was aggregated, so each question meant a fresh bespoke hunt.
;;
;; This reads what ALREADY EXISTS. It adds no instrumentation and no daemon: it
;; is a reader over the journal and the agent logs.
;;
;; TWO RULES it will not break, both learned the hard way that night:
;;
;;   1. NEVER report a mean without a distribution. Coordinator reads had a 24ms
;;      median with 23% of samples at 4.7-6.6s. A mean describes neither
;;      population and would have sent effort at the wrong thing.
;;   2. NEVER report a count without saying what produced it. A metric is a claim
;;      made by code, not an observation of the world; several "findings" that
;;      night were artifacts of a command that had silently errored. Every
;;      section names its source so a zero can be distinguished from a failure.
(require '[clojure.string :as str]
         '[babashka.process :refer [shell]])

(def use-color? (and (nil? (System/getenv "NO_COLOR"))
                     (nil? (System/getenv "NORTH_NO_COLOR"))))
(defn- c [code s] (if use-color? (str "\033[" code "m" s "\033[0m") (str s)))
(defn- dim [s] (c "2" s))
(defn- bold [s] (c "1" s))
(defn- grn [s] (c "32" s))
(defn- red [s] (c "31" s))
(defn- ylw [s] (c "33" s))

(defn- sh [& args]
  (try
    (let [{:keys [out]} (apply shell {:out :string :err :string :continue true} args)]
      (or out ""))
    (catch Throwable _ "")))

(defn percentile
  "Value at `p` (0-1) of a SORTED vector, or nil when empty. Nearest-rank: with
  few samples an interpolated percentile invents precision the data lacks."
  [sorted p]
  (when (seq sorted)
    (nth sorted (min (dec (count sorted))
                     (int (* p (count sorted)))))))

(defn distribution
  "The shape of a latency population — never a bare mean.

  A 24ms median with a 6s p95 and a 3.7s mean are three different facts about
  the same numbers, and only the first two tell you what a user experiences."
  [xs]
  (when (seq xs)
    (let [s (vec (sort xs))
          n (count s)]
      {:n n
       :min (first s)
       :median (percentile s 0.5)
       :p95 (percentile s 0.95)
       :max (peek s)
       :mean (long (/ (reduce + 0 s) n))})))

;; ---- coordinator reads ------------------------------------------------------
;; Emitted by fram's slow-read attribution:
;;   [fram] slow read :fenced-query 13717ms = reload 0ms + lock-wait 0ms + execute 13717ms
;; Only reads OVER the threshold appear, so this is explicitly the TAIL, not the
;; whole population — labelled as such below rather than presented as typical.
(def slow-read-re
  #"slow read :(\S+) (\d+)ms = reload (\d+)ms \+ lock-wait (\d+)ms \+ execute (\d+)ms")

(defn coordinator-reads [since]
  (->> (sh "journalctl" "--user" "-u" "north-fram.service" "--since" since "--no-pager")
       str/split-lines
       (keep #(re-find slow-read-re %))
       (map (fn [[_ route total reload lock execute]]
              {:route route
               :total (parse-long total)
               :reload (parse-long reload)
               :lock (parse-long lock)
               :execute (parse-long execute)}))))

(defn render-coordinator [rows]
  (println (bold "COORDINATOR READS") (dim "— source: journalctl north-fram.service, slow-read attribution"))
  (if (empty? rows)
    (println (dim "  no slow reads in window (either healthy, or the journal is unavailable)"))
    (doseq [[route rs] (sort-by (comp - count val) (group-by :route rows))]
      (let [d (distribution (map :total rs))
            phase (fn [k] (long (/ (reduce + 0 (map k rs)) (count rs))))]
        (println (format "  %-16s n=%-5d median=%-8s p95=%-9s max=%s"
                         route (:n d) (str (:median d) "ms")
                         (str (:p95 d) "ms") (str (:max d) "ms")))
        (println (format "  %-16s mean phases: reload=%dms lock-wait=%dms execute=%dms"
                         "" (phase :reload) (phase :lock) (phase :execute)))
        ;; The phase split IS the diagnosis, so state it rather than leaving the
        ;; reader to infer it from three numbers.
        (let [l (phase :lock) e (phase :execute) r (phase :reload)]
          (println (str "  " (dim "→ ")
                        (cond
                          (> l (max e r)) (red "blocked on a lock — contention or maintenance holds it")
                          (> r e) (ylw "dominated by reload — the log is moving under readers")
                          :else (ylw "dominated by execute — evaluation cost or GC, not contention")))))))
    ))

;; ---- lane outcomes ----------------------------------------------------------
(defn lane-outcomes [since-iso]
  (let [out (sh "bash" "-c"
                (str "cd " (System/getenv "HOME") "/.local/state/north/agents 2>/dev/null && "
                     "find . -name '*.log' -newermt '" since-iso "' "
                     "-exec grep -h 'complete (process=' {} + 2>/dev/null"))]
    (->> (str/split-lines out)
         (keep #(second (re-find #"process=([a-z_]+)" %)))
         frequencies)))

(defn failure-causes [since-iso]
  (let [out (sh "bash" "-c"
                (str "cd " (System/getenv "HOME") "/.local/state/north/agents 2>/dev/null && "
                     "find . -name '*.log' -newermt '" since-iso "' "
                     "-exec grep -hoE 'failure=[^]]{5,70}|cause: [^<\"]{5,70}' {} + 2>/dev/null"))]
    (->> (str/split-lines out)
         (remove str/blank?)
         (map #(str/replace % #"[0-9a-f]{8}-[0-9a-f-]+" "<id>"))
         frequencies
         (sort-by (comp - val))
         (take 6))))

(defn render-lanes [outcomes causes]
  (println)
  (println (bold "LANE OUTCOMES") (dim "— source: ~/.local/state/north/agents/*.log"))
  (let [total (reduce + 0 (vals outcomes))]
    (if (zero? total)
      (println (dim "  no lanes completed in window"))
      (doseq [[o n] (sort-by (comp - val) outcomes)]
        (println (format "  %-26s %5d  %4.1f%%  %s"
                         o n (* 100.0 (/ n total))
                         (if (= o "ran") (grn "") (red "← failure")))))))
  (when (seq causes)
    (println)
    (println (bold "TOP FAILURE CAUSES") (dim "— the sentence the provider actually reported"))
    (doseq [[cause n] causes]
      (println (format "  %3d  %s" n (str/trim (str/replace cause #"^(failure=|cause: )" "")))))))

(defn -main [& args]
  (let [window (or (first (filter #(re-matches #"\d+[hd]" %) args)) "6h")
        since (str window " ago")
        since-iso (str/trim (sh "date" "-d" (str "-" window) "+%Y-%m-%dT%H:%M:%S"))]
    (println (bold "north hotspots") (dim (str "— window " window)))
    (println)
    (render-coordinator (coordinator-reads since))
    (render-lanes (lane-outcomes since-iso) (failure-causes since-iso))
    (println)
    (println (dim "  Reads are the slow-read TAIL only (over threshold), not the whole population."))
    (println (dim "  A zero above means 'nothing recorded', which is not the same as 'nothing happened'."))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
