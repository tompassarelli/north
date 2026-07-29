#!/usr/bin/env bb
;; An unavailable domain must say WHY, not just WHICH.
;;
;; live-triples-at already captured the exception into :error; live-facts-view
;; dropped it, so every caller could report was a domain NAME. On 2026-07-29
;; that discarded string was
;;   "coordinator response line exceeds 8388608 bytes"
;; — the whole corpus outgrowing the response cap. Without it, "unavailable
;; domain(s): coordination" reads as a dead daemon, and an hour went into
;; bisecting a write path that was never broken.
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

(def OVERSIZE "coordinator response line exceeds 8388608 bytes")

;; --- the failure that motivated this ---------------------------------------
(with-redefs [send-op-for-log (fn [& _] (throw (ex-info OVERSIZE {})))]
  (let [{:keys [unavailable unavailable-detail complete]} (live-facts-view 7977)]
    (check! "the domain is still named" (= ["coordination"] unavailable))
    (check! "the REASON is carried alongside it"
            (= [["coordination" OVERSIZE]] unavailable-detail))
    (check! "an incomplete view is not reported as complete" (false? complete))))

;; --- a healthy view stays quiet --------------------------------------------
(with-redefs [send-op-for-log
              (fn [& _] {:version 1 :facts [["@a" "p" "v"]]})]
  (let [{:keys [unavailable unavailable-detail complete facts]}
        (live-facts-view 7977)]
    (check! "no unavailable domains when healthy" (empty? unavailable))
    (check! "no detail when healthy" (empty? unavailable-detail))
    (check! "complete when healthy" (true? complete))
    (check! "facts still come through" (= [["@a" "p" "v"]] facts))))

;; --- a malformed response is distinguishable from a thrown one --------------
;; Both make the domain unavailable, and the two need different fixes, so the
;; reason must not collapse them into one another.
(with-redefs [send-op-for-log (fn [& _] {:version 1 :facts "not-a-vector"})]
  (let [{:keys [unavailable-detail]} (live-facts-view 7977)]
    (check! "a malformed response reports its own distinct reason"
            (= [["coordination" "malformed :facts response"]] unavailable-detail))))

;; --- never nil: a reason is always present when a domain is down ------------
(with-redefs [send-op-for-log (fn [& _] (throw (RuntimeException.)))]
  (let [{:keys [unavailable-detail]} (live-facts-view 7977)]
    (check! "a message-less exception still yields a reason string"
            (and (= 1 (count unavailable-detail))
                 (string? (second (first unavailable-detail)))
                 (seq (second (first unavailable-detail)))))))

;; --- the response cap must exceed what the warm path actually needs ----------
;; The cap bounds one response, but 8 MiB sat BELOW North's own whole-corpus
;; view (~345k triples). Both domains blew it on every call, live-triples-at
;; marked them unavailable, and north silently fell back to a COLD FOLD of the
;; 36 MB log — a correct answer, 7-9x slower, with nothing saying why.
;;
;; Measured 2026-07-29, same corpus and load:
;;   verb        8 MiB cap    64 MiB cap
;;   validate    44,326 ms     6,425 ms
;;   threads     45,978 ms     5,163 ms
;;   ready       48,906 ms     6,428 ms
;;   blocked     45,517 ms     6,783 ms
;;
;; Asserts the PROPERTY — comfortably above the corpus view — not a literal, so
;; tuning stays free while a regression to a cap the warm path cannot fit fails.
(check! "the default response cap clears the whole-corpus view"
        (>= (Long/parseLong (deref #'north.coord/default-response-byte-limit))
            33554432))

(check! "the default is not above the hard maximum the validator permits"
        (<= (Long/parseLong (deref #'north.coord/default-response-byte-limit))
            67108864))

(println (format "live-facts-view-detail: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
