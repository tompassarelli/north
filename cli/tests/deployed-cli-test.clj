#!/usr/bin/env bb
;; Tests for `north deployed`.
;;
;; The property under test is narrow and load-bearing: this table's ONLY job is
;; to say whether committed code is running, so a wrong green row is worse than
;; no tool at all. The first implementation printed "✓ live" for fram while the
;; daemon was three commits behind, because the health probe failed and absent
;; disagreement read as agreement. Every case below exists to keep that shut.
(load-file (str (.getParent (io/file *file*)) "/../deployed-cli.clj"))

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

(defn status [m] (first (verdict m)))
(defn text [m] (nth (verdict m) 2))

(def A "aaaaaaaa1111111111111111111111111111aaaa")
(def B "bbbbbbbb2222222222222222222222222222bbbb")

;; --- the healthy row --------------------------------------------------------
(check! "all three equal is live"
        (= :live (status {:component "fram" :source A :built A :running A
                          :expect-running? true})))

(check! "a component with no discoverable runtime is live on source=built"
        (= :live (status {:component "beagle" :source A :built A :running nil
                          :expect-running? false})))

;; --- the two failures need OPPOSITE actions ---------------------------------
(check! "source ahead of built means rebuild"
        (= :stale-build (status {:component "fram" :source B :built A :running A
                                 :expect-running? true})))
(check! "the rebuild row says rebuild, not restart"
        (clojure.string/includes?
         (text {:component "fram" :source B :built A :running A
                :expect-running? true})
         "rebuild"))

(check! "built ahead of running means restart"
        (= :stale-process (status {:component "fram" :source A :built A :running B
                                   :expect-running? true})))
(check! "the restart row says restart, not rebuild"
        (clojure.string/includes?
         (text {:component "fram" :source A :built A :running B
                :expect-running? true})
         "restart"))

;; --- an unbuilt commit outranks a stale process -----------------------------
;; Restarting first would be wasted work: the closure does not yet contain the
;; change, so the process would come back equally stale.
(check! "source≠built wins even when running also differs"
        (= :stale-build (status {:component "fram" :source B :built A :running A
                                 :expect-running? true})))

;; --- absence must never render as health (the original bug) -----------------
(check! "unreadable running revision is unknown, NOT live"
        (= :unknown (status {:component "fram" :source A :built A :running nil
                             :expect-running? true})))
(check! "missing source is unknown"
        (= :unknown (status {:component "fram" :source nil :built A :running A
                             :expect-running? true})))
(check! "missing built is unknown"
        (= :unknown (status {:component "fram" :source A :built nil :running A
                             :expect-running? true})))

;; --- nixos-config: time-based, because it has no rev in the closure ---------
;; /run/current-system records its NIXPKGS rev, not which config commit built
;; it, so this component cannot be revision-matched like the others. On
;; 2026-07-29 the coordinator's -Xmx16g fix was committed at 06:40, the running
;; generation was built at 06:33, and the daemon kept exhausting a 6 GB heap
;; every 8 minutes with the fix undeployed. Nothing surfaced that gap.

(check! "no generation timestamp yields nil, not zero"
        (nil? (commits-since-epoch "nixos-config" nil)))

(let [called (atom nil)]
  (with-redefs [sh (fn [& args] (reset! called args) "3\n")]
    (check! "counts commits newer than the generation"
            (= 3 (commits-since-epoch "nixos-config" 1785000000)))
    (check! "asks git for commits since that exact epoch"
            (some #(= "--since=@1785000000" %) @called))
    (check! "scopes the count to main, not the checked-out branch"
            (some #(= "refs/heads/main" %) @called))))

;; A non-numeric or absent git answer must not become a false "0 pending".
(with-redefs [sh (fn [& _] "")]
  (check! "an unparseable count is nil, never a confident zero"
          (nil? (commits-since-epoch "nixos-config" 1785000000))))

(println (format "deployed-cli: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
