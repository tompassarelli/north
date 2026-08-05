#!/usr/bin/env bb
;; `north trace` must name the CAUSE of a dispatch failure, not just its category.
;;
;; Before this, a failed lane reported:
;;   ✗ 6 COMPLETION  process=provider_error · delivery=blocked (provider_terminal_error)
;; Three facts naming the same category, none naming a cause — while the actual
;; sentence ("Codex managed hook did not complete successfully") sat unread on
;; the run subject. Across 128,290 coordination facts the predicate `detail`
;; appears exactly once.
(require '[clojure.java.io :as io])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def trace-cli (str root "/cli/trace-cli.clj"))
(let [caller (System/getProperty "babashka.file")]
  (try
    (System/setProperty "north.trace.lib" "1")
    (System/setProperty "babashka.file" trace-cli)
    (load-file trace-cli)
    (finally
      (System/clearProperty "north.trace.lib")
      (System/setProperty "babashka.file" caller))))

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

(defn entry
  ([subject at] (entry subject at nil))
  ([subject at detail]
   (cond-> {:subject subject :facts {"at" #{at}}}
     detail (assoc-in [:facts "provider_error_detail"] #{detail}))))

;; --- the basic case ---------------------------------------------------------
(check! "returns the detail for a single failed run"
        (= "boom"
           (provider-error-detail
            [(entry "@run:a" "2026-07-29T01:00:00Z" "boom")])))

(check! "no detail anywhere yields nil"
        (nil? (provider-error-detail [(entry "@run:a" "2026-07-29T01:00:00Z")])))

(check! "no runs at all yields nil"
        (nil? (provider-error-detail [])))

;; --- ordering is by TIME, not by subject string -----------------------------
;; A lane can have several runs. Sorting by subject would report whichever id
;; happened to sort last, which is arbitrary; the operator wants the LATEST
;; failure. These ids are deliberately ordered opposite to their timestamps.
(check! "reports the most recent failure, not the last subject alphabetically"
        (= "newest"
           (provider-error-detail
            [(entry "@run:zzz" "2026-07-29T01:00:00Z" "oldest")
             (entry "@run:aaa" "2026-07-29T09:00:00Z" "newest")])))

(check! "ordering holds regardless of input order"
        (= "newest"
           (provider-error-detail
            [(entry "@run:aaa" "2026-07-29T09:00:00Z" "newest")
             (entry "@run:zzz" "2026-07-29T01:00:00Z" "oldest")])))

;; --- runs without a detail must not mask one that has it --------------------
(check! "a later run with no detail does not hide an earlier real cause"
        (= "the cause"
           (provider-error-detail
            [(entry "@run:a" "2026-07-29T01:00:00Z" "the cause")
             (entry "@run:b" "2026-07-29T09:00:00Z")])))

;; --- a missing timestamp must not throw -------------------------------------
(check! "a run with an unparseable `at` is tolerated"
        (some? (provider-error-detail
                [{:subject "@run:a"
                  :facts {"provider_error_detail" #{"boom"}}}])))

(println (format "trace-cause: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
