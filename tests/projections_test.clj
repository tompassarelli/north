;; projections_test.clj — deterministic contract for the one lifecycle
;; classifier and every pull-eligibility view derived from it.
;;
;;   bb -cp out:$STORE/out tests/projections_test.clj      (run from the repo root)
(require '[store.types :as t]
         '[store.rt]
         '[north.projections :as proj]
         '[north.main])

(defn asrt [_tx l p r _record] (t/triple l p r))

(def today "2026-06-16")
(defn before? [a b] (neg? (compare a b)))

;; Activity authority is an explicit three-valued input. @a and @b exercise
;; precedence over LiveProven, @c is active, and @h is quarantined Unresolved.
(def activity-states
  {"@a" proj/live-proven
   "@b" proj/live-proven
   "@c" proj/live-proven
   "@h" proj/unresolved})
(defn activity [_idx te] (get activity-states te proj/absent-proven))
(defn unknown-activity [_idx _te] "UnknownValue")

(def asserts
  [;; @a — terminal beats active.
   (asrt 1 "@a" "title" "A" "test")
   (asrt 2 "@a" "committed" "2026-01-01" "test")
   (asrt 3 "@a" "outcome" "done" "test")
   (asrt 4 "@a" "driver" "@p" "test")

   ;; @b — blocked beats active.
   (asrt 10 "@b" "title" "B" "test")
   (asrt 11 "@b" "committed" "2026-01-01" "test")
   (asrt 12 "@b" "depends_on" "@dep" "test")
   (asrt 13 "@b" "driver" "@p" "test")
   (asrt 14 "@dep" "title" "Dependency" "test")
   (asrt 15 "@dep" "committed" "2026-01-01" "test")
   (asrt 16 "@dep" "created_at" "2026-01-01" "test")

   ;; @c — a genuinely live assignment is active.
   (asrt 20 "@c" "title" "C" "test")
   (asrt 21 "@c" "committed" "2026-01-01" "test")
   (asrt 22 "@c" "driver" "@p" "test")

   ;; @d — committed, driverless work is ready.
   (asrt 30 "@d" "title" "D" "test")
   (asrt 31 "@d" "committed" "2026-01-01" "test")
   (asrt 32 "@d" "created_at" "2026-01-01" "test")

   ;; @e — future scheduling makes even a draft dormant.
   (asrt 40 "@e" "title" "E" "test")
   (asrt 41 "@e" "do_on" "2026-12-01" "test")

   ;; @f — a past schedule does not promote an uncommitted draft.
   (asrt 50 "@f" "title" "F" "test")
   (asrt 51 "@f" "do_on" "2020-01-01" "test")

   ;; @g — future scheduling gates committed work too: dormant beats ready.
   (asrt 60 "@g" "title" "G" "test")
   (asrt 61 "@g" "committed" "2026-01-01" "test")
   (asrt 62 "@g" "do_on" "2026-12-01" "test")

   ;; @h — the driver fact remains, but activity is unresolved.
   (asrt 70 "@h" "title" "H" "test")
   (asrt 71 "@h" "committed" "2026-01-01" "test")
   (asrt 72 "@h" "driver" "@uncertain" "test")

   ;; @i — every terminal predicate is a work axis, not a grouping anchor.
   (asrt 80 "@i" "title" "I" "test")
   (asrt 81 "@i" "committed" "2026-01-01" "test")
   (asrt 82 "@i" "superseded_by" "@replacement" "test")

   ;; @j — overdue scheduling and stored priority remain distinct ranking axes.
   (asrt 90 "@j" "title" "J" "test")
   (asrt 91 "@j" "committed" "2026-01-01" "test")
   (asrt 92 "@j" "created_at" "2026-01-01" "test")
   (asrt 93 "@j" "do_on" "2020-01-01" "test")
   (asrt 94 "@j" "priority" "P1" "test")])

(def idx (proj/index-triples asserts))
(defn cls [te] (proj/classify idx te today before? activity))
(def work-set (set (proj/work-thread-ids-i idx)))
(def ready-set (set (proj/ready idx today before? activity)))
(def condition-ready-set
  (set (filter (fn [te] (= "ready" (proj/condition-i idx te today before? activity)))
               work-set)))
(def eligible-set
  (set (filter (fn [te] (proj/eligible? idx te today before? activity))
               work-set)))
(def expected-ready #{"@dep" "@d" "@j"})

;; Archive is a separate presentation/grooming axis. This paired corpus proves
;; that adding archived_at does not mutate lifecycle state; it intentionally says
;; nothing about whether a default view should display archived work.
(def archive-asserts
  [(asrt 1 "@plain" "title" "Plain" "test")
   (asrt 2 "@plain" "committed" "2026-01-01" "test")
   (asrt 3 "@plain" "created_at" "2026-01-01" "test")
   (asrt 10 "@archived" "title" "Archived" "test")
   (asrt 11 "@archived" "committed" "2026-01-01" "test")
   (asrt 12 "@archived" "created_at" "2026-01-01" "test")
   (asrt 13 "@archived" "archived_at" "2026-06-01" "test")])
(def archive-idx (proj/index-triples archive-asserts))
(defn archive-cls [te] (proj/classify archive-idx te today before? activity))

;; Exercise the production three-valued lease judgment against a fixed clock.
;; functions are deliberately private implementation seams; resolving their vars
;; keeps the public lifecycle API small while making the temporal contract exact.
(def liveness-asserts
  [(asrt 1 "@lease-live" "title" "Lease live" "test")
   (asrt 2 "@lease-live" "driver" "@agent-live" "test")
   (asrt 3 "session:agent-live" :kernel/lease
         (t/triple "agent-live" :kernel/expires-at 2000000000000) "test")
   (asrt 10 "@recent" "title" "Recent human" "test")
   (asrt 11 "@recent" "driver" "@human" "test")
   (asrt 12 "@recent" "updated_at" "2026-06-15T12:00:00" "test")
   (asrt 20 "@expired-recent" "title" "Expired lease, recent work" "test")
   (asrt 21 "@expired-recent" "driver" "@agent-expired" "test")
   (asrt 22 "@expired-recent" "updated_at" "2026-06-15T12:00:00" "test")
   (asrt 23 "session:agent-expired" :kernel/lease
         (t/triple "agent-expired" :kernel/expires-at 1000000000000) "test")
   (asrt 30 "@stale" "title" "Stale assignment" "test")
   (asrt 31 "@stale" "driver" "@human" "test")
   (asrt 32 "@stale" "updated_at" "2026-05-01T12:00:00" "test")
   (asrt 40 "@driverless" "title" "Driverless" "test")
   (asrt 41 "@driverless" "updated_at" "2026-06-15T12:00:00" "test")
   (asrt 50 "@conflicting-driver" "title" "Conflicting driver" "test")
   (asrt 51 "@conflicting-driver" "driver" "@one" "test")
   (asrt 52 "@conflicting-driver" "driver" "@two" "test")
   (asrt 60 "@malformed-lease" "title" "Malformed lease" "test")
   (asrt 61 "@malformed-lease" "driver" "@broken" "test")
   (asrt 62 "session:broken" :kernel/lease "not-a-typed-lease" "test")
   (asrt 70 "@wrong-holder" "title" "Wrong lease holder" "test")
   (asrt 71 "@wrong-holder" "driver" "@expected" "test")
   (asrt 72 "session:expected" :kernel/lease
         (t/triple "somebody-else" :kernel/expires-at 2000000000000) "test")
   (asrt 80 "@conflicting-lease" "title" "Conflicting lease" "test")
   (asrt 81 "@conflicting-lease" "driver" "@duplicate" "test")
   (asrt 82 "session:duplicate" :kernel/lease
         (t/triple "duplicate" :kernel/expires-at 2000000000000) "test")
   (asrt 83 "session:duplicate" :kernel/lease
         (t/triple "duplicate" :kernel/expires-at 3000000000000) "test")])
(def liveness-idx (proj/index-triples liveness-asserts))
(def liveness-now-ms 1500000000000)
(def driver-activity-fn (ns-resolve 'north.main 'driver-activity))
(def next-item-fn (ns-resolve 'north.main 'next-item))
(def grooming-fn (ns-resolve 'north.main 'canonical-grooming-reviews))
(def dep-next (next-item-fn idx "@dep" today before? activity))
(def ready-next (next-item-fn idx "@d" today before? activity))
(def urgent-next (next-item-fn idx "@j" today before? activity))
(def blocked-next (next-item-fn idx "@b" today before? activity))

(def grooming-asserts
  [(asrt 1 "@work" "title" "Groom me" "test")
   (asrt 2 "@work" "kind" "thread" "test")
   (asrt 3 "@work" "committed" "2026-01-01" "test")
   (asrt 4 "@work" "driver" "@worker" "test")])
(def grooming-idx (proj/index-triples grooming-asserts))
(defn unresolved-activity [_idx _te] proj/unresolved)
(defn live-activity [_idx _te] proj/live-proven)
(def unresolved-reviews
  (grooming-fn grooming-idx grooming-idx today before? unresolved-activity))
(def active-reviews (grooming-fn grooming-idx grooming-idx today before? live-activity))

(def queue-asserts
  [(asrt 1 "@qa" "title" "Queue A" "test")
   (asrt 2 "@qa" "kind" "thread" "test")
   (asrt 3 "@qa" "committed" "2026-01-01" "test")
   (asrt 4 "@qb" "title" "Queue B" "test")
   (asrt 5 "@qb" "kind" "thread" "test")
   (asrt 6 "@qb" "committed" "2026-01-01" "test")
   (asrt 7 "@qb" "queue_rank" "v1|10|first|_" "test")
   (asrt 8 "@qc" "title" "Queue C" "test")
   (asrt 9 "@qc" "kind" "thread" "test")
   (asrt 10 "@qc" "committed" "2026-01-01" "test")
   (asrt 11 "@qc" "queue_rank" "v1|20|before|@qa" "test")
   (asrt 12 "@done-old" "title" "Done old" "test")
   (asrt 13 "@done-old" "kind" "thread" "test")
   (asrt 14 "@done-old" "outcome" "done" "test")
   (asrt 15 "@done-old" "updated_at" "2026-01-01" "test")
   (asrt 16 "@done-new" "title" "Done new" "test")
   (asrt 17 "@done-new" "kind" "thread" "test")
   (asrt 18 "@done-new" "outcome" "done" "test")
   (asrt 19 "@done-new" "updated_at" "2026-02-01" "test")])
(def queue-idx (proj/index-triples queue-asserts))
(def recent-terminal-fn (ns-resolve 'north.main 'recent-terminal-tes))
(def board-curated-fn (ns-resolve 'north.main 'board-curated-tes))
(def queue-order-result (north.main/queue-order queue-idx ["@qa" "@qb" "@qc"]))
(def curated-board (board-curated-fn idx today before? activity false))

(def checks
  [["terminal beats a live assignment" (= "terminal" (cls "@a"))]
   ["blocked beats a live assignment" (= "blocked" (cls "@b"))]
   ["live assignment is active" (= "active" (cls "@c"))]
   ["committed driverless work is ready" (= "ready" (cls "@d"))]
   ["future draft is dormant" (= "dormant" (cls "@e"))]
   ["past-scheduled uncommitted work stays draft" (= "draft" (cls "@f"))]
   ["future scheduling gates committed work" (= "dormant" (cls "@g"))]
   ["unresolved assignment is quarantined" (= "unresolved" (cls "@h"))]
   ["unknown activity values fail closed as unresolved"
    (= "unresolved" (proj/classify idx "@d" today before? unknown-activity))]
   ["unresolved assignment remains visible as assignment" (proj/assigned? idx "@h")]
   ["superseded thread remains in the work corpus" (contains? work-set "@i")]
   ["superseded thread is terminal" (= "terminal" (cls "@i"))]
   ["archive metadata does not alter lifecycle"
    (and (= "ready" (archive-cls "@plain"))
         (= (archive-cls "@plain") (archive-cls "@archived")))]
   ["dormant? sees a future do_on" (proj/dormant? idx "@g" today before?)]
   ["dormant? rejects a past do_on" (not (proj/dormant? idx "@f" today before?))]
   ["ready is the expected fixed-corpus set" (= expected-ready ready-set)]
   ["board READY bucket and ready projection agree" (= condition-ready-set ready-set)]
   ["next eligibility and ready projection agree" (= eligible-set ready-set)]
   ["ready excludes terminal" (not (contains? ready-set "@a"))]
   ["ready excludes blocked" (not (contains? ready-set "@b"))]
   ["ready excludes active" (not (contains? ready-set "@c"))]
   ["ready excludes dormant" (not (contains? ready-set "@g"))]
   ["ready excludes draft" (not (contains? ready-set "@f"))]
   ["explanation exposes ready eligibility"
    (let [e (proj/explain idx "@d" today before? activity)]
      (and (= "ready" (:state e)) (:eligible e) (not (empty? (:reason e)))))]
   ["explanation exposes ineligibility reason"
    (let [e (proj/explain idx "@g" today before? activity)]
      (and (= "dormant" (:state e)) (not (:eligible e)) (not (empty? (:reason e)))))]
   ["unexpired typed agent lease is LiveProven"
    (= proj/live-proven
       (driver-activity-fn liveness-idx "@lease-live" liveness-now-ms))]
   ["recent assignment without a lease is Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@recent" liveness-now-ms))]
   ["recent activity does not rescue an expired lease"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@expired-recent" liveness-now-ms))]
   ["stale assignment without a lease is Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@stale" liveness-now-ms))]
   ["complete driver absence is AbsentProven despite recency"
    (= proj/absent-proven
       (driver-activity-fn liveness-idx "@driverless" liveness-now-ms))]
   ["conflicting driver facts are Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@conflicting-driver" liveness-now-ms))]
   ["malformed lease evidence is Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@malformed-lease" liveness-now-ms))]
   ["mismatched lease holder is Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@wrong-holder" liveness-now-ms))]
   ["multiple lease propositions are Unresolved"
    (= proj/unresolved
       (driver-activity-fn liveness-idx "@conflicting-lease" liveness-now-ms))]
   ["curated JSON board retains unresolved assignments"
    (some #{"@h"} curated-board)]
   ["unresolved presentation has a distinct default"
    (= "🟠" (proj/condition-emoji idx "unresolved"))]
   ["graph leverage is an explicit weighted score component"
    (and (= 1 (:leverage dep-next)) (= 3 (:score dep-next)))]
   ["ready ranking uses leverage and urgency only"
    (and (= 0 (:urgency ready-next)) (= 0 (:score ready-next)))]
   ["urgency and stored priority remain distinct"
    (and (= 5 (:urgency urgent-next)) (= "P1" (:priority urgent-next)) (= 5 (:score urgent-next)))]
   ["dependency sequencing is an eligibility gate"
    (and (= 1 (:sequencing blocked-next))
         (= "blocked" (:state (proj/explain idx "@b" today before? activity))))]
   ["recommendation carries the canonical eligibility basis"
    (= (:basis ready-next) (:reason (proj/explain idx "@d" today before? activity)))]
   ["grooming turns uncertain activity into an unresolved-assignment review"
    (= ["driver"] (mapv :pred unresolved-reviews))]
   ["grooming keeps the done-bar review for a genuinely live pickup"
    (= ["done_when"] (mapv :pred active-reviews))]
   ["manual queue moves replay in coordinator-version order over fallback"
    (= ["@qb" "@qc" "@qa"] queue-order-result)]
   ["queue rank receipt grammar is stable"
    (= "v1|42|after|@qa" (north.main/queue-rank-value 42 "after" "@qa"))]
   ["recent terminal projection is newest first"
    (= ["@done-new" "@done-old"] (recent-terminal-fn queue-idx))]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\nprojections:" (count checks) "/" (count checks) "PASS")
    (do (println "\nprojections:" (count fails) "FAILED") (System/exit 1))))
