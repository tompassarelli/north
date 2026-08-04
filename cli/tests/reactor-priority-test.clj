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

(defn stage-stubs [calls concern-fn]
  {#'maybe-rebuild-window!
   (fn [_]
     (swap! calls conj :rebuild-window)
     {:action "fired" :count 2})
   #'sweep-concerns!
   (fn [_]
     (swap! calls conj :concerns)
     (concern-fn))
   #'reconcile-local-concerns-bounded!
   (fn [_]
     (swap! calls conj :local-concern-reconcile)
     {:status :completed})
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
   #'reconcile-attention-bounded!
   (fn [_] (swap! calls conj :attention-reconcile) {:status :completed})})

(let [started (atom nil)
      awaited (atom nil)]
  (with-out-str
    (with-redefs
     [start-sweep-child!
      (fn [label options & command]
        (reset! started {:label label :options options :command command})
        ::child)
      await-sweep-child!
      (fn [child timeout-ms]
        (reset! awaited {:child child :timeout-ms timeout-ms})
        {:status :completed :result {:exit 0}})]
      (reconcile-local-concerns-bounded! "fixture")))
  (check "local concern recovery gives its bounded pass enough process headroom"
         (and (= "20000"
                 (get-in @started
                         [:options :extra-env
                          "NORTH_CONCERN_RECONCILE_MAX_MILLIS"]))
              (= 30000 (:timeout-ms @awaited))
              (= ["reconcile-local" "--operations-only"]
                 (vec (take-last 2 (:command @started)))))
         {:started @started :awaited @awaited}))

(let [calls (atom [])
      summary
      (with-redefs-fn
        (stage-stubs calls (constantly 0))
        #(sweep! true))]
  (check "rebuild collection precedes every maintenance stage"
         (= :rebuild-window (first @calls))
         @calls)
  (check "maintenance preserves an already collected rebuild window"
         (and (= "fired" (get-in summary [:rebuild-window :action]))
              (= :completed (:terminal-status summary)))
         summary)
  (check "dry-run maintenance does not mutate the local concern spool"
         (not (some #{:local-concern-reconcile} @calls))
         @calls))

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
               #(throw (ex-info "fixture stale transition failed" {})))
              #'with-sweep-lock (fn [f] (f)))
             #(sweep-once-exit-code)))))]
  (check "a failed stale-concern stage is contained and starves no later stage"
         (and (= :rebuild-window (first @calls))
              (= :local-concern-reconcile (second @calls))
              (= :concerns (nth @calls 2))
              (some #{:attention-reconcile} @calls)
              (zero? @exit)
              (str/includes? output "terminal=completed")
              (str/includes? output "rebuild-window=fired")
              (not (str/includes? output "maintenance=degraded"))
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
              (stage-stubs calls (constantly 0))
              #'with-sweep-lock (fn [f] (f))
              #'sweep-lanes!
              (fn [_]
                (swap! calls conj :lanes)
                (throw (ex-info "fixture lane sweep failed" {}))))
             #(sweep-once-exit-code)))))]
  (check "post-launch maintenance failure preserves the owner success"
         (and (= [:rebuild-window :local-concern-reconcile :concerns :lanes]
                 @calls)
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
              (stage-stubs calls (constantly 0))
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

(let [source (slurp reactor)
      body (second
            (re-find
             #"(?s)\(defn sweep-loop \[\](.*?)\n\n;; bin/north"
             source))]
  (check "periodic reactor owner enters through the locked one-shot lifecycle"
         (and (string? body)
              (str/includes? body "(sweep-once-exit-code)")
              (not (str/includes? body "(sweep! false)")))
         body))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nreactor priority: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
