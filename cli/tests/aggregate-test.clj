;; aggregate-test.clj — the incremental-aggregate primitive (coord.clj) against a
;; LIVE :7977 daemon. Proves ONE fold + two reducers: count-distinct (quorum) and
;; Σ (numeric usage), each commutative + idempotent.
;;   bb cli/tests/aggregate-test.clj [port]
(require '[clojure.java.io :as io])
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/../coord.clj"))
(alias 'co (the-ns 'north.coord))

(let [port (Integer/parseInt (or (first *command-line-args*) "7977"))
      tag  (format "%08x" (rand-int 0x7fffffff))
      b    (str "@aggtest:" tag)
      checks (atom [])
      chk (fn [nm ok] (swap! checks conj [nm ok]))
      ;; quorum body: distinct workers that emitted DONE against this test batch
      done-body [{:rel "triple" :args [{:var "d"} "agg_done_batch" b]}
                 {:rel "triple" :args [{:var "d"} "agg_done_worker" {:var "w"}]}]
      ;; usage body: every run booked to this test batch
      usage-body [{:rel "triple" :args [{:var "c"} "agg_run_batch" b]}
                  {:rel "triple" :args [{:var "c"} "agg_run_tokens" {:var "n"}]}]]

  ;; --- COUNT-DISTINCT (quorum). 3 distinct workers; w2 reports TWICE. ---------
  (doseq [[d w] [["d1" "w1"] ["d2" "w2"] ["d2b" "w2"] ["d3" "w3"]]]
    (let [de (str b ":" d)]
      (co/put! port de "agg_done_batch" b)
      (co/put! port de "agg_done_worker" w)))
  (chk "count-distinct collapses a double-reporting worker (3 distinct, 4 facts)"
       (= 3 (co/count-distinct port ["w"] done-body)))
  (chk "distinct-of returns the SET (for missing-member diffs)"
       (= #{"w1" "w2" "w3"} (co/distinct-of port ["w"] done-body)))
  (chk "quorum-met? FIRES at K=3"      (true?  (co/quorum-met? port 3 ["w"] done-body)))
  (chk "quorum-met? WAITS at K=4"      (false? (co/quorum-met? port 4 ["w"] done-body)))
  (chk "idempotent: re-asserting w2 DONE does not move the count"
       (do (co/put! port (str b ":d2c") "agg_done_batch" b)
           (co/put! port (str b ":d2c") "agg_done_worker" "w2")
           (= 3 (co/count-distinct port ["w"] done-body))))

  ;; --- Σ (numeric usage). Append-only samples; the total is a fold, never a cell. r4 has
  ;; the SAME value as r1 (100) — the dedup trap: a value-only projection would
  ;; collapse them and under-count. [key val] keeps them distinct. -------------
  (doseq [[c n] [["r1" 100] ["r2" 250] ["r3" 50.5] ["r4" 100]]]
    (let [ce (str b ":run:" c)]
      (co/put! port ce "agg_run_batch" b)
      (co/put! port ce "agg_run_tokens" n)))
  (chk "sum-of Σ's [key val] rows (100+250+50.5+100), equal-valued addends NOT deduped"
       (== 500.5 (co/sum-of port ["c" "n"] usage-body)))
  ;; the ROW SEAM: scope rows with a client-side predicate the scan body can't
  ;; express (here: drop r3), then fold through the SAME shared reducer. This is
  ;; how callers can narrow a numeric fold without reimplementing the reducer.
  (chk "sum-rows folds pre-filtered rows (drop r3=50.5 -> 100+250+100)"
       (== 450.0 (co/sum-rows (->> (co/agg-rows port ["c" "n"] usage-body)
                                   (remove #(= "50.5" (str (second %))))))))

  (let [results @checks pass (count (filter second results))]
    (doseq [[nm ok] results] (println (format "  [%s]  %s" (if ok "PASS" "FAIL") nm)))
    (println (format "\nincremental-aggregate (coord.clj): %d / %d PASS" pass (count results)))
    (System/exit (if (= pass (count results)) 0 1))))
