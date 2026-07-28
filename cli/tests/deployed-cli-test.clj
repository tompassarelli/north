#!/usr/bin/env bb
;; Tests for `north deployed`.
;;
;; The property under test is narrow and load-bearing: this table's ONLY job is
;; to say whether committed code is running, so a wrong green row is worse than
;; no tool at all. The first implementation printed "✓ live" for fram while the
;; daemon was three commits behind, because the health probe failed and absent
;; disagreement read as agreement. Every case below exists to keep that shut.
(load-file (str (.getParent (io/file *file*)) "/../deployed-cli.clj"))
(def deployed-source
  (slurp (str (.getParent (io/file *file*)) "/../deployed-cli.clj")))

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

(check! "built ahead of running means paired cutover"
        (= :stale-process (status {:component "fram" :source A :built A :running B
                                   :expect-running? true})))
(let [t (text {:component "fram" :source A :built A :running B
               :expect-running? true})]
  (check! "the stale-process row says paired cutover, not rebuild"
          (and (clojure.string/includes? t "paired cutover")
               (not (clojure.string/includes? t "rebuild")))))

(check! "paired cutover uses the runtime protocol"
        (= "sudo north-coord-runtime restart" COORDINATOR-CUTOVER-COMMAND))
(check! "production source never recommends restarting one writer directly"
        (not (clojure.string/includes?
              deployed-source
              "systemctl restart north-coord.service")))
(check! "deployment identity uses coord-ready, not the heavyweight doctor"
        (and (clojure.string/includes? deployed-source "\"coord-ready\"")
             (not (clojure.string/includes? deployed-source "\"coord-doctor\""))))

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
;; The persistent system-N-link records generation provenance; the tmpfs
;; /run/current-system link does not survive reboot as evidence. On
;; 2026-07-29 the coordinator's -Xmx16g fix was committed at 06:40, the running
;; generation was built at 06:33, and the daemon kept exhausting a 6 GB heap
;; every 8 minutes with the fix undeployed. Nothing surfaced that gap.

(let [calls (atom [])]
  (with-redefs [sh (fn [& args]
                     (swap! calls conj args)
                     (cond
                       (= "readlink" (first args)) "system-1072-link"
                       (= "stat" (first args)) "1785279200"
                       :else nil))]
    (check! "persistent generation-link timestamp is read"
            (= 1785279200 (generation-built-epoch)))
    (check! "never uses the tmpfs current-system link as provenance"
            (not-any? #(some #{"/run/current-system"} %) @calls))
    (check! "stats the selected durable generation link"
            (some #(some #{"/nix/var/nix/profiles/system-1072-link"} %) @calls))))

(check! "no generation timestamp yields nil, not zero"
        (nil? (commits-since-epoch "nixos-config" nil)))

(let [called (atom nil)]
  (with-redefs [sh (fn [& args] (reset! called args) "3\n")]
    (check! "counts commits newer than the generation"
            (= 3 (commits-since-epoch "nixos-config" 1785000000)))
    (check! "asks git for commits strictly newer than the generation epoch"
            (some #(= "--since=@1785000001" %) @called))
    (check! "scopes the count to main, not the checked-out branch"
            (some #(= "refs/heads/main" %) @called))))

;; A non-numeric or absent git answer must not become a false "0 pending".
(with-redefs [sh (fn [& _] "")]
  (check! "an unparseable count is nil, never a confident zero"
          (nil? (commits-since-epoch "nixos-config" 1785000000))))

(with-redefs [generation-built-epoch (constantly nil)
              commits-since-epoch (fn [& _] nil)]
  (check! "missing generation evidence is a failing unknown row"
          (= :unknown (first (:verdict (nixos-config-assessment))))))

(with-redefs [generation-built-epoch (constantly 1785000000)
              commits-since-epoch (fn [& _] 3)]
  (let [r (nixos-config-assessment)]
    (check! "pending config commits are a stale-build row"
            (= :stale-build (first (:verdict r))))
    (check! "pending count is preserved for JSON"
            (= 3 (:pending r)))))

(with-redefs [generation-built-epoch (constantly 1785000000)
              commits-since-epoch (fn [& _] 0)]
  (check! "zero pending config commits is live"
          (= :live (first (:verdict (nixos-config-assessment))))))

(let [good-row {:component "fram" :verdict [:live "✓" "live"]}
      unknown-row {:component "nixos-config"
                   :verdict [:unknown "?" "cannot determine"]}]
  (check! "JSON always includes the nixos-config result"
          (= ["fram" "nixos-config"]
             (mapv :component (json-rows [good-row unknown-row]))))
  (check! "JSON carries the failing status"
          (= "unknown" (:status (second (json-rows [good-row unknown-row]))))))

(with-redefs [locked-revs (constantly {"north" A "fram" A "beagle" A})
              source-rev (constantly A)
              running-fram (constantly A)
              system-fram-rev (constantly A)
              nixos-config-assessment
              (constantly {:component "nixos-config"
                           :verdict [:unknown "?" "cannot determine"]})]
  (let [{:keys [judged worst]} (deployment-report)]
    (check! "unknown nixos-config evidence makes the command fail"
            (= 1 worst))
    (check! "nixos-config participates in the same judged output"
            (= "nixos-config" (:component (last judged))))))

(println (format "deployed-cli: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
