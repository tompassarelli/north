#!/usr/bin/env bb
(require '[clojure.java.io :as io] '[clojure.string :as str])
(load-file "cli/shipped-report.clj")

(def checks (atom []))
(defn check! [label ok?] (swap! checks conj [label (boolean ok?)]))
(def now (java.time.Instant/parse "2026-08-03T12:00:00Z"))
(def runs {"@run:one" [["kind" "run"] ["at" "2026-08-03T11:00:00Z"] ["process_outcome" "ran"] ["delivery_outcome" "verified"] ["thread" "@thread-one"] ["provider" "anthropic"] ["duration_ms" "120000"]]
           "@run:two" [["kind" "run"] ["at" "2026-08-02T12:00:00Z"] ["process_outcome" "ran"] ["thread" "@thread-two"] ["provider" "openai"] ["composition_id" "implementer"]]
           "@run:old" [["kind" "run"] ["at" "2026-07-20T12:00:00Z"] ["process_outcome" "ran"] ["thread" "@thread-one"] ["provider" "openai"]]})
(def threads {"@thread-one" [["title" "Ship fixture"] ["outcome" "landed in abcdef1"] ["progress" "commit 7654321"]]
              "@thread-two" [["title" "Unverified fixture"] ["outcome" "landed"]]})
(with-redefs [north.shipped-report/run-subjects (fn [_] (vec (keys runs)))
              north.shipped-report/exact-facts-many
              (fn [_ _ subjects]
                (into {}
                      (map (fn [subject]
                             [subject
                              (north.shipped-report/rows->facts
                               (or (get runs subject) (get threads subject)))])
                           subjects)))]
  (let [rows (north.shipped-report/report-rows 7977 (.minus now (java.time.Duration/ofHours 24)) now)
        rendered (north.shipped-report/render rows (.minus now (java.time.Duration/ofHours 24)) now)]
    (check! "windowed run projection excludes old facts" (= 2 (count rows)))
    (check! "render groups native and managed harnesses" (and (str/includes? rendered "native-claude") (str/includes? rendered "managed-codex")))
    (check! "render marks missing delivery proof unverified" (str/includes? rendered "unverified · Unverified fixture"))
    (check! "render includes title, outcome, and fact-referenced commits" (and (str/includes? rendered "Ship fixture") (str/includes? rendered "outcome: landed in abcdef1") (str/includes? rendered "commits: 7654321, abcdef1")))))
(doseq [[label ok?] @checks] (println (if ok? "PASS" "FAIL") label))
(System/exit (if (every? second @checks) 0 1))
