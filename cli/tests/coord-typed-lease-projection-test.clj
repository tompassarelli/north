#!/usr/bin/env bb
(load-file (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                "/../coord.clj"))
(in-ns 'north.coord)
(clojure.core/refer-clojure)

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (clojure.core/swap! checks inc)
  (if pass?
    (println "PASS" label)
    (do
      (clojure.core/swap! failures inc)
      (println "FAIL" label))))

(def valid-lease
  (t/triple "session:agent-live" :kernel/lease
            (t/triple "agent-live" :kernel/expires-at 2000000000000)))

(def transaction (t/transaction-coordinate "north-coordination" 4216))
(def occurrence (t/occurrence-coordinate transaction 0))
(def prior-occurrence
  (t/occurrence-coordinate
   (t/transaction-coordinate "north-coordination" 4200) 0))
(def asserted-by (t/triple transaction :kernel/asserted-by "@agent:writer"))
(def supersedes (t/triple occurrence :kernel/supersedes prior-occurrence))

(with-redefs [telemetry-partition-enabled? (constantly false)
              with-client! (fn [_ _ operation] (operation :fixture-client))
              north.store-rpc-client/scan-all!
              (fn [& _]
                {:served-version 91
                 :rows [(t/triple "@thread:one" "title" "Typed terms survive")
                        valid-lease
                        asserted-by
                        supersedes]})]
  (let [{:keys [facts domains complete]} (live-facts-view! 7977)
        telemetry (live-domain! 7977 :telemetry)]
    (check! "a valid typed lease does not invalidate the durable fact view"
            (true? complete))
    (check! "the coordination projection excludes recognized kernel terms"
            (= [["@thread:one" "title" "Typed terms survive"]] facts))
    (check! "the projected domain keeps the Store snapshot version"
            (= 91 (get-in domains [:coordination :version])))
    (check! "lease filtering is not applied to another domain"
            (false? (:available telemetry)))))

(def malformed-lease
  (t/triple "session:agent-broken" :kernel/lease
            (t/triple "agent-broken" :kernel/expires-at "not-an-integer")))

(def malformed-asserted-by
  (t/triple "not-a-transaction-coordinate" :kernel/asserted-by "@agent:writer"))
(def malformed-supersedes
  (t/triple transaction :kernel/supersedes prior-occurrence))

(defn rejected-view [version proposition]
  (with-redefs [telemetry-partition-enabled? (constantly false)
                with-client! (fn [_ _ operation] (operation :fixture-client))
                north.store-rpc-client/scan-all!
                (fn [& _] {:served-version version :rows [proposition]})]
    (live-facts-view! 7977)))

(doseq [[label version proposition]
        [["lease" 92 malformed-lease]
         ["asserted-by" 93 malformed-asserted-by]
         ["supersedes" 94 malformed-supersedes]]]
  (let [{:keys [facts unavailable unavailable-detail complete]}
        (rejected-view version proposition)]
    (check! (str "a malformed typed " label " fails the coordination domain closed")
            (and (false? complete)
                 (empty? facts)
                 (= ["coordination"] unavailable)))
    (check! (str "the malformed typed " label " retains a diagnostic reason")
            (and (= 1 (count unavailable-detail))
                 (clojure.string/includes?
                  (second (first unavailable-detail))
                  "non-string triple")))))

(with-redefs [with-client! (fn [_ _ operation] (operation :fixture-client))
              north.store-rpc-client/scan-all!
              (fn [& _] {:served-version 95 :rows [valid-lease]})]
  (let [{:keys [holder version]} (lease-status! 7977 "session:agent-live")]
    (check! "the dedicated lease projection still receives the typed lease"
            (and (= "agent-live" holder) (= 95 version)))))

(println (format "coord typed lease projection: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
