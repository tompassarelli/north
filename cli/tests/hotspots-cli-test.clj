#!/usr/bin/env bb
;; `north hotspots` exists to stop a night of manual measurement from being the
;; only way to answer "what should I fix?". Its two hard rules, both learned the
;; hard way on 2026-07-29:
;;   1. never a mean without a distribution
;;   2. never a number without naming what produced it
;; These tests defend rule 1 and the parse; rule 2 is defended by the source
;; labels asserted at the bottom.
(load-file (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                "/../hotspots-cli.clj"))
(require '[clojure.string :as str])

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

;; --- distribution: the anti-mean rule ---------------------------------------
;; This exact shape is why the rule exists: coordinator reads had a 24ms median
;; with a long tail. A mean of these describes NEITHER population.
(let [bimodal (concat (repeat 31 24) (repeat 9 5500))
      d (distribution bimodal)]
  (check! "reports n" (= 40 (:n d)))
  (check! "median tracks the common case, not the tail" (= 24 (:median d)))
  (check! "p95 exposes the tail the median hides" (>= (:p95 d) 5000))
  (check! "max is the worst observed" (= 5500 (:max d)))
  (check! "the mean is between them and describes neither"
          (and (> (:mean d) (:median d)) (< (:mean d) (:p95 d)))))

(check! "an empty population is nil, not a fabricated zero"
        (nil? (distribution [])))
(check! "a single sample still yields a distribution"
        (= 1 (:n (distribution [7]))))

;; --- percentile: nearest-rank, no invented precision ------------------------
(let [s (vec (range 1 101))]
  (check! "p50 of 1..100" (= 51 (percentile s 0.5)))
  (check! "p95 of 1..100" (= 96 (percentile s 0.95)))
  (check! "p100 does not run off the end" (= 100 (percentile s 1.0))))
(check! "percentile of empty is nil" (nil? (percentile [] 0.5)))

;; --- the slow-read parse ----------------------------------------------------
;; The phase split is the diagnosis, so every field must survive parsing.
(def REAL-LINE
  "Jul 29 05:31:16 whiterabbit fram-server-native[3221912]: [fram] slow read :fenced-query 13717ms = reload 0ms + lock-wait 0ms + execute 13717ms")

(let [[_ route total reload lock execute] (re-find slow-read-re REAL-LINE)]
  (check! "parses the route" (= "fenced-query" route))
  (check! "parses the total" (= "13717" total))
  (check! "parses reload" (= "0" reload))
  (check! "parses lock-wait" (= "0" lock))
  (check! "parses execute" (= "13717" execute)))

(check! "a non-matching line yields nil, not a partial row"
        (nil? (re-find slow-read-re "some unrelated journal line")))

;; --- rule 2: every section names its source ---------------------------------
;; A number with no provenance is unverifiable, and several "findings" that night
;; were artifacts of a command that had silently errored.
(def leverage-source (slurp (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                        "/../hotspots-cli.clj")))
(check! "coordinator section names its source"
        (str/includes? leverage-source "source: journalctl north-coord.service"))
(check! "lane section names its source"
        (str/includes? leverage-source "source: ~/.local/state/north/agents"))
(check! "the tail-vs-population caveat is stated to the reader"
        (str/includes? leverage-source "TAIL only"))
(check! "a zero is explicitly distinguished from a failure to measure"
        (str/includes? leverage-source "not the same as 'nothing happened'"))

(println (format "hotspots-cli: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
