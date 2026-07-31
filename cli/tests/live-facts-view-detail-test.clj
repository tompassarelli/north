#!/usr/bin/env bb
;; Every block stubs telemetry-partition-enabled? — an unstubbed case reaches
;; the LIVE coordinator (70-120s cold fold; breaks exact-equality asserts).
;; north.coord's 4-arg swap! (coord.clj:961) shadows clojure.core/swap!:
;; atom ops in this namespace must be fully qualified.
(load-file (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                "/../coord.clj"))
(in-ns 'north.coord)
(clojure.core/refer-clojure)

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (clojure.core/swap! checks inc)
  (if pass? (println "PASS" label)
      (do (clojure.core/swap! failures inc) (println "FAIL" label))))

(def ABORTED "coordinator paged facts query failed")
(def PAGE-CURSOR "fram-query-page-v1.YQ")

;; --- the failure that motivated this ---------------------------------------
(with-redefs [telemetry-partition-enabled? (constantly false)
              query-page-in-domain (fn [& _] (throw (ex-info ABORTED {})))]
  (let [{:keys [unavailable unavailable-detail complete]} (live-facts-view 7977)]
    (check! "the domain is still named" (= ["coordination"] unavailable))
    (check! "the REASON is carried alongside it"
            (= [["coordination" ABORTED]] unavailable-detail))
    (check! "an incomplete view is not reported as complete" (false? complete))))

;; --- a healthy view stays quiet --------------------------------------------
(with-redefs [telemetry-partition-enabled? (constantly false)
              query-page-in-domain
              (fn [& _]
                {:version 1 :ok [["@a" "p" "v"]]
                 :more false :next nil :engine "scan"})]
  (let [{:keys [unavailable unavailable-detail complete facts]}
        (live-facts-view 7977)]
    (check! "no unavailable domains when healthy" (empty? unavailable))
    (check! "no detail when healthy" (empty? unavailable-detail))
    (check! "complete when healthy" (true? complete))
    (check! "facts still come through" (= [["@a" "p" "v"]] facts))))

;; --- a malformed response is distinguishable from a thrown one --------------
;; Both make the domain unavailable, and the two need different fixes, so the
;; reason must not collapse them into one another.
(with-redefs [telemetry-partition-enabled? (constantly false)
              query-page-in-domain
              (fn [& _]
                {:version 1 :ok "not-a-vector"
                 :more false :next nil :engine "scan"})]
  (let [{:keys [unavailable-detail]} (live-facts-view 7977)]
    (check! "a malformed response reports its own distinct reason"
            (= [["coordination" "coordinator returned a malformed paged facts response"]]
               unavailable-detail))))

;; --- never nil: a reason is always present when a domain is down ------------
(with-redefs [telemetry-partition-enabled? (constantly false)
              query-page-in-domain (fn [& _] (throw (RuntimeException.)))]
  (let [{:keys [unavailable-detail]} (live-facts-view 7977)]
    (check! "a message-less exception still yields a reason string"
            (and (= 1 (count unavailable-detail))
                 (string? (second (first unavailable-detail)))
                 (seq (second (first unavailable-detail)))))))

;; --- an expired pin degrades to an incomplete domain, never a torn view -----
(with-redefs [telemetry-partition-enabled? (constantly false)
              query-page-in-domain
              (fn [& args]
                (if (nil? (nth args 5 nil))
                  {:version 7 :ok [["@a" "p" "v"]]
                   :more true :next PAGE-CURSOR :engine "scan"}
                  (throw (ex-info "coordinator no longer retains the pinned query snapshot"
                                  {:type :query-page-snapshot-expired
                                   :at-version 7 :version 9}))))]
  (let [{:keys [facts unavailable-detail complete]} (live-facts-view 7977)]
    (check! "an expired pin aborts the paged domain with its typed reason"
            (some (fn [[domain reason]]
                    (and (= "coordination" domain)
                         (clojure.string/includes? reason "no longer retains")))
                  unavailable-detail))
    (check! "an expired pin exposes no partial warm facts"
            (and (false? complete) (empty? facts)))))

;; --- one version witnesses the entire drain ---------------------------------
(let [calls (atom 0)]
  (with-redefs [telemetry-partition-enabled? (constantly false)
                query-page-in-domain
                (fn [& _]
                  (if (= 1 (clojure.core/swap! calls inc))
                    {:version 1 :ok [["@a" "p" "v"]]
                     :more true :next PAGE-CURSOR :engine "scan"}
                    {:version 2 :ok [["@b" "p" "v"]]
                     :more false :next nil :engine "scan"}))]
    (let [{:keys [facts unavailable-detail complete]} (live-facts-view 7977)]
      (check! "version drift aborts the paged domain"
              (some (fn [[domain reason]]
                      (and (= "coordination" domain)
                           (clojure.string/includes? reason "version changed from 1 to 2")))
                    unavailable-detail))
      (check! "version drift exposes no silently truncated warm facts"
              (and (false? complete) (empty? facts))))))

(println (format "live-facts-view-detail: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
