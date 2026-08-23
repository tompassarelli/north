;; board_active_test.clj — the curated board's three-valued activity authority.
;; A complete Store projection proves absence only when no driver is assigned.
;; One current typed session lease proves liveness. Every assigned thread whose
;; lease is missing, expired, malformed, or conflicting remains Unresolved;
;; `updated_at` never promotes activity.
;;   BEAGLE_STORE_OUT=/path/to/store/out bb -cp out:"$BEAGLE_STORE_OUT" tests/board_active_test.clj
(require '[store.types :as t] '[north.projections :as proj]
         '[north.main :as m])

(def now-ms 1500000000000)
(def fresh-exp (+ now-ms 1800000))
(def expired-exp (- now-ms 1800000))
(defn lease-val [handle exp]
  (t/triple handle :kernel/expires-at exp))

(def facts
  [;; @live-lease — agent driver holding a FRESH lease -> live (lease path)
   (t/triple "@live-lease" "title" "live via lease")
   (t/triple "@live-lease" "driver" "@ag-live")
   (t/triple "session:ag-live" :kernel/lease (lease-val "ag-live" fresh-exp))
   ;; @lapsed-recent — an expired lease stays unresolved despite recency.
   (t/triple "@lapsed-recent" "title" "lapsed but recent") (t/triple "@lapsed-recent" "driver" "@ag-lapsed")
   (t/triple "session:ag-lapsed" :kernel/lease (lease-val "ag-lapsed" expired-exp))
   (t/triple "@lapsed-recent" "updated_at" "2026-07-08")
   ;; @human-recent — a human driver with no lease remains unresolved.
   (t/triple "@human-recent" "title" "human recent") (t/triple "@human-recent" "driver" "@tom")
   (t/triple "@human-recent" "updated_at" "2026-07-06")
   ;; @human-stale — chronology does not change the same unresolved verdict.
   (t/triple "@human-stale" "title" "human stale") (t/triple "@human-stale" "driver" "@tom")
   (t/triple "@human-stale" "updated_at" "2026-05-28")
   ;; @no-signal — driver, no lease, no updated_at -> unresolved.
   (t/triple "@no-signal" "title" "no signal") (t/triple "@no-signal" "driver" "@ghost")
   ;; @no-driver — the complete projection proves assignment absence.
   (t/triple "@no-driver" "title" "no driver")
   ;; @garbage-ts — timestamp shape cannot affect activity or crash it.
   (t/triple "@garbage-ts" "title" "garbage ts") (t/triple "@garbage-ts" "driver" "@tom")
   (t/triple "@garbage-ts" "updated_at" "not-a-date")])

(def idx (proj/index-triples facts))
(defn activity [te] (#'m/driver-activity idx te now-ms))

(def cases
  [["fresh typed lease => LiveProven" (activity "@live-lease") proj/live-proven]
   ["expired lease + recent => Unresolved" (activity "@lapsed-recent") proj/unresolved]
   ["human recent without lease => Unresolved" (activity "@human-recent") proj/unresolved]
   ["human stale without lease => Unresolved" (activity "@human-stale") proj/unresolved]
   ["no lease + no updated_at => Unresolved" (activity "@no-signal") proj/unresolved]
   ["no driver => AbsentProven" (activity "@no-driver") proj/absent-proven]
   ["garbage updated_at => Unresolved" (activity "@garbage-ts") proj/unresolved]])

(def fails (filter (fn [[_ got want]] (not= got want)) cases))
(doseq [[nm got want] cases]
  (println (if (= got want) "  ok  " " FAIL ") nm "=> got" got))
(if (seq fails)
  (do (println "\nboard-active:" (count fails) "FAILED") (System/exit 1))
  (println "\nboard-active: all" (count cases) "passed"))
