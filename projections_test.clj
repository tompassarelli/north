;; projections_test.clj — deterministic contract for the one lifecycle
;; classifier and every pull-eligibility view derived from it.
;;
;;   bb -cp out:$FRAM/out projections_test.clj      (run from the repo root)
(require '[fram.kernel :as k]
         '[fram.fold :as fold]
         '[fram.rt]
         '[north.projections :as proj]
         '[north.main])

(defn asrt [tx l p r frame] (fold/->FactOp tx "assert" l p r frame))

(def today "2026-06-16")
(defn before? [a b] (neg? (compare a b)))

;; Liveness is an explicit input, not inferred from the durable driver fact.
;; @a and @b exercise precedence over a live assignment; @c is simply active.
(def live-subjects #{"@a" "@b" "@c"})
(defn live? [_idx te] (contains? live-subjects te))

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

   ;; @h — the driver fact remains, but its stale assignment is not activity.
   (asrt 70 "@h" "title" "H" "test")
   (asrt 71 "@h" "committed" "2026-01-01" "test")
   (asrt 72 "@h" "driver" "@parked" "test")

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

(def idx (k/build-index (:facts (fold/fold asserts))))
(defn cls [te] (proj/classify idx te today before? live?))
(def work-set (set (proj/work-thread-ids-i idx)))
(def ready-set (set (proj/ready idx today before? live?)))
(def condition-ready-set
  (set (filter (fn [te] (= "ready" (proj/condition-i idx te today before? live?)))
               work-set)))
(def eligible-set
  (set (filter (fn [te] (proj/eligible? idx te today before? live?))
               work-set)))
(def expected-ready #{"@dep" "@d" "@h" "@j"})

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
(def archive-idx (k/build-index (:facts (fold/fold archive-asserts))))
(defn archive-cls [te] (proj/classify archive-idx te today before? live?))

;; Exercise the production lease/recency predicate against a fixed clock. These
;; functions are deliberately private implementation seams; resolving their vars
;; keeps the public lifecycle API small while making the temporal contract exact.
(def liveness-asserts
  [(asrt 1 "@lease-live" "title" "Lease live" "test")
   (asrt 2 "@lease-live" "driver" "@agent-live" "test")
   (asrt 3 "@lease:session:agent-live" "lease" "holder|2000000000000|1" "test")
   (asrt 10 "@recent" "title" "Recent human" "test")
   (asrt 11 "@recent" "driver" "@human" "test")
   (asrt 12 "@recent" "updated_at" "2026-06-15T12:00:00" "test")
   (asrt 20 "@expired-recent" "title" "Expired lease, recent work" "test")
   (asrt 21 "@expired-recent" "driver" "@agent-expired" "test")
   (asrt 22 "@expired-recent" "updated_at" "2026-06-15T12:00:00" "test")
   (asrt 23 "@lease:session:agent-expired" "lease" "holder|1000000000000|1" "test")
   (asrt 30 "@stale" "title" "Stale assignment" "test")
   (asrt 31 "@stale" "driver" "@human" "test")
   (asrt 32 "@stale" "updated_at" "2026-05-01T12:00:00" "test")
   (asrt 40 "@driverless" "title" "Driverless" "test")
   (asrt 41 "@driverless" "updated_at" "2026-06-15T12:00:00" "test")])
(def liveness-idx (k/build-index (:facts (fold/fold liveness-asserts))))
(def liveness-now (fram.rt/iso-to-seconds "2026-06-16T12:00:00"))
(def liveness-window (* 14 86400))
(def driver-live-fn (ns-resolve 'north.main 'driver-live?))
(def next-item-fn (ns-resolve 'north.main 'next-item))
(def grooming-fn (ns-resolve 'north.main 'canonical-grooming-reviews))
(def dep-next (next-item-fn idx "@dep" today before? live?))
(def parked-next (next-item-fn idx "@h" today before? live?))
(def urgent-next (next-item-fn idx "@j" today before? live?))
(def blocked-next (next-item-fn idx "@b" today before? live?))

(def grooming-asserts
  [(asrt 1 "@work" "title" "Groom me" "test")
   (asrt 2 "@work" "kind" "thread" "test")
   (asrt 3 "@work" "committed" "2026-01-01" "test")
   (asrt 4 "@work" "driver" "@worker" "test")])
(def grooming-idx (k/build-index (:facts (fold/fold grooming-asserts))))
(defn never-live? [_idx _te] false)
(defn always-live? [_idx _te] true)
(def parked-reviews (grooming-fn grooming-idx [] grooming-idx today before? never-live?))
(def active-reviews (grooming-fn grooming-idx [] grooming-idx today before? always-live?))

(def checks
  [["terminal beats a live assignment" (= "terminal" (cls "@a"))]
   ["blocked beats a live assignment" (= "blocked" (cls "@b"))]
   ["live assignment is active" (= "active" (cls "@c"))]
   ["committed driverless work is ready" (= "ready" (cls "@d"))]
   ["future draft is dormant" (= "dormant" (cls "@e"))]
   ["past-scheduled uncommitted work stays draft" (= "draft" (cls "@f"))]
   ["future scheduling gates committed work" (= "dormant" (cls "@g"))]
   ["parked driver falls through to ready" (= "ready" (cls "@h"))]
   ["parked assignment remains visible as assignment" (proj/assigned? idx "@h")]
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
    (let [e (proj/explain idx "@h" today before? live?)]
      (and (= "ready" (:state e)) (:eligible e) (not (empty? (:reason e)))))]
   ["explanation exposes ineligibility reason"
    (let [e (proj/explain idx "@g" today before? live?)]
      (and (= "dormant" (:state e)) (not (:eligible e)) (not (empty? (:reason e)))))]
   ["unexpired agent lease is live"
    (driver-live-fn liveness-idx "@lease-live" liveness-now liveness-window)]
   ["recent human activity is live without a lease"
    (driver-live-fn liveness-idx "@recent" liveness-now liveness-window)]
   ["recent activity rescues an expired lease"
    (driver-live-fn liveness-idx "@expired-recent" liveness-now liveness-window)]
   ["stale assignment is not live"
    (not (driver-live-fn liveness-idx "@stale" liveness-now liveness-window))]
   ["recency cannot make a driverless thread live"
    (not (driver-live-fn liveness-idx "@driverless" liveness-now liveness-window))]
   ["graph leverage is an explicit weighted score component"
    (and (= 1 (:leverage dep-next)) (= 3 (:score dep-next)))]
   ["parked-assignment momentum is explicit"
    (and (= 2 (:momentum parked-next)) (= 2 (:score parked-next)))]
   ["urgency and stored priority remain distinct"
    (and (= 5 (:urgency urgent-next)) (= "P1" (:priority urgent-next)) (= 5 (:score urgent-next)))]
   ["dependency sequencing is an eligibility gate"
    (and (= 1 (:sequencing blocked-next))
         (= "blocked" (:state (proj/explain idx "@b" today before? live?))))]
   ["recommendation carries the canonical eligibility basis"
    (= (:basis parked-next) (:reason (proj/explain idx "@h" today before? live?)))]
   ["grooming turns a stale pickup into a parked-assignment review"
    (= ["driver"] (mapv :pred parked-reviews))]
   ["grooming keeps the done-bar review for a genuinely live pickup"
    (= ["done_when"] (mapv :pred active-reviews))]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\nprojections:" (count checks) "/" (count checks) "PASS")
    (do (println "\nprojections:" (count fails) "FAILED") (System/exit 1))))
