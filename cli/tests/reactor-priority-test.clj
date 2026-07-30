#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def reactor
  (or (System/getenv "NORTH_TEST_REACTOR")
      (str root "/cli/north-reactor.clj")))
(def checks (atom []))

(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(System/setProperty "babashka.file" reactor)
(load-string
 (-> (slurp reactor)
     (str/replace-first #"^#![^\n]*\n" "")
     (str/replace
      #"\n\(if sweep-verb\?\n  \(System/exit \(sweep-once-exit-code\)\)\n  \(-main\)\)\s*$"
      "\n")))

(defn stage-stubs [calls concern-fn audit]
  {#'maybe-rebuild-window!
   (fn [_]
     (swap! calls conj :rebuild-window)
     {:action "fired" :count 2})
   #'sweep-concerns!
   (fn [_]
     (swap! calls conj :concerns)
     (concern-fn))
   #'sweep-lanes!
   (fn [_] (swap! calls conj :lanes) 0)
   #'sweep-unpublished-driver-claims!
   (fn [_] (swap! calls conj :unpublished-drivers) 0)
   #'north.worktree-janitor/sweep-worktrees!
   (fn [_]
     (swap! calls conj :worktree-janitor)
     {:removed 0 :dirty 0 :uncertain 0 :partial 0 :already-removed 0
      :orphan-facts-written 0 :errors 0})
   #'sweep-agent-logs!
   (fn [_] (swap! calls conj :agent-logs) {:deleted 0 :capped 0})
   #'north.spend-breaker/sweep-burn!
   (fn [& _] (swap! calls conj :spend-burn) {:tripped false})
   #'north.spend-breaker/sweep-kill!
   (fn [& _] (swap! calls conj :spend-kill) 0)
   #'maybe-clock-audit!
   (fn [_] (swap! calls conj :clock-audit) audit)})

(let [calls (atom [])
      summary
      (with-redefs-fn
        (stage-stubs calls (constantly 0) {:status :deferred :reason :timeout})
        #(sweep! true))]
  (check "rebuild collection precedes every maintenance stage"
         (= :rebuild-window (first @calls))
         @calls)
  (check "later audit deferral cannot erase an already collected rebuild window"
         (and (= "fired" (get-in summary [:rebuild-window :action]))
              (= :deferred (:terminal-status summary)))
         summary))

(let [calls (atom [])
      exit (atom nil)
      output
      (with-out-str
        (binding [*err* *out*]
          (reset!
           exit
           (with-redefs-fn
             (assoc
              (stage-stubs
               calls
               #(throw (ex-info "fixture stale transition failed" {}))
               {:status :skipped})
              #'with-sweep-lock (fn [f] (f)))
             #(sweep-once-exit-code)))))]
  (check "post-launch maintenance failure preserves the owner success"
         (and (= [:rebuild-window :concerns] @calls)
              (zero? @exit)
              (str/includes? output "terminal=completed")
              (str/includes? output "rebuild-window=fired")
              (str/includes? output "maintenance=degraded")
              (not (str/includes? output "terminal=failed")))
         {:calls @calls :exit @exit :output output}))

(let [calls (atom [])
      exit (atom nil)
      output
      (with-out-str
        (binding [*err* *out*]
          (reset!
           exit
           (with-redefs-fn
             (assoc
              (stage-stubs calls (constantly 0) {:status :skipped})
              #'with-sweep-lock (fn [f] (f))
              #'maybe-rebuild-window!
              (fn [_]
                (swap! calls conj :rebuild-window)
                {:action "error"
                 :count 0
                 :error (ex-info "fixture queue collection failed" {})}))
             #(sweep-once-exit-code)))))]
  (check "queue-owner failure remains nonzero and actionable"
         (and (= [:rebuild-window] @calls)
              (= 1 @exit)
              (str/includes? output "terminal=failed")
              (str/includes? output "fixture queue collection failed")
              (not (str/includes? output "maintenance=degraded")))
         {:calls @calls :exit @exit :output output}))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nreactor priority: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
