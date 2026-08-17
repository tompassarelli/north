;; capture_test.clj — the UUIDv7-handle model on the capture path:
;;   (1) capture mints a UUIDv7 id (an opaque, version-7 uuid — NOT the old
;;       @yyyy-MM-dd-HHmmss timestamp scheme), and
;;   (2) asserts an explicit full-ISO `created_at` fact (now-iso) at birth, while
;;   (3) `resolve-ref` maps a @handle (or @id) ref to the canonical @id, latest
;;       created_at winning ties — the boundary fram never sees a handle through.
;;   bb -cp out:/home/tom/code/beagle/main/branch-core/out tests/capture_test.clj
(require '[fram.types :as t] '[fram.rt :as rt] '[north.main :as m]
         '[north.projections :as proj]
         '[clojure.string :as str] '[cheshire.core :as json])

;; --- (1) uuidv7: a real version-7 uuid, not a date --------------------------
(def uuid-re #"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
(def date-id-re #"^\d{4}-\d{2}-\d{2}-\d{6}$")   ; the old fram.rt/now-id shape
(def u1 (m/uuidv7))
(def _gap (Thread/sleep 5))
(def u2 (m/uuidv7))

;; --- (2) created_at present + full-ISO via capture-facts (private) ----------
(def cap (#'m/capture-facts "@t1" "Test thread" "personal" "self" "" "" ""
                             "2026-06-28T07:00:00" "2026-06-28"))
(defn fact-val [facts pred]
  (t/triple-t3
   (first (filter #(= (t/triple-t2 %) pred) facts))))

;; --- (3) resolve-ref: handle -> canonical id (latest created_at wins) --------
(def rc
  [(t/triple "@id-old" "title" "Old perf")
   (t/triple "@id-old" "handle" "perf")
   (t/triple "@id-old" "created_at" "2026-06-01T10:00:00")
   (t/triple "@id-new" "title" "New perf")
   (t/triple "@id-new" "handle" "perf")
   (t/triple "@id-new" "created_at" "2026-06-28T10:00:00")
   (t/triple "@solo" "title" "Solo")])
(def ridx (proj/index-triples rc))

;; Structured capture owns its freshly minted UUID. Inject one acknowledged
;; assert followed by conflicts and prove the path retracts exactly that fact,
;; removes the view, and reports a clean failed receipt.
(def partial-live (atom []))
(def partial-asserts (atom 0))
(def partial-retracts (atom []))
(def partial-view-deleted (atom false))
(def partial-output
  (with-redefs-fn
    {#'m/uuidv7 (fn [] "019f0000-0000-7000-8000-000000000001")
     #'rt/getenv (fn [key]
                   (when (= key "NORTH_CAPTURE_STRUCTURED") "1"))
     #'rt/ensure-dir (fn [_] nil)
     #'rt/today-iso (fn [] "2026-07-19")
     #'rt/now-iso (fn [] "2026-07-19T00:00:00Z")
     #'m/coord-port (fn [] 7977)
     #'m/coord-version (fn [_] 1)
     #'m/tell-retry
     (fn [_ _ op subject predicate value _]
       (if (= op "assert")
         (if (= 1 (swap! partial-asserts inc))
           (do
             (swap! partial-live conj (t/triple subject predicate value))
             "ok:2")
           "conflict")
         (do
           (swap! partial-retracts conj [subject predicate value])
           (swap! partial-live
                  (fn [facts]
                    (vec (remove #(and (= subject (t/triple-t1 %))
                                       (= predicate (t/triple-t2 %))
                                       (= value (t/triple-t3 %)))
                                 facts))))
           "ok:3")))
     #'rt/delete-file (fn [_] (reset! partial-view-deleted true) nil)
     #'rt/file-exists (fn [_] (not @partial-view-deleted))
     #'m/coord-live-propositions (fn [_] @partial-live)
     #'rt/spit-file (fn [& _] (throw (ex-info "partial capture wrote a view" {})))}
    #(with-out-str
       (m/cmd-capture "/tmp/north-capture-test" "/tmp/facts.log"
                      "Injected partial" "personal"))))
(def partial-receipt (json/parse-string (str/trim partial-output) true))

;; Cleanup is best-effort across the complete acknowledged write set. A failed
;; first retraction must not strand a later fact merely because `and`
;; short-circuited the recursion.
(def cleanup-attempts (atom []))
(def cleanup-facts
  [(t/triple "@cleanup-probe" "title" "Cleanup probe")
   (t/triple "@cleanup-probe" "kind" "thread")])
(def cleanup-all-attempted?
  (with-redefs [m/tell-retry
                (fn [_ _ _ subject predicate value _]
                  (swap! cleanup-attempts conj [subject predicate value])
                  (if (= 1 (count @cleanup-attempts)) "conflict" "ok:4"))]
    (#'m/retract-committed-capture-facts
     7977 "/tmp/facts.log" cleanup-facts ["ok:2" "ok:3"] 0)))

;; Help must be a non-mutating parser branch, never a literal `--help` thread.
(def capture-help-writes (atom 0))
(def capture-help-output
  (with-redefs [m/cmd-capture (fn [& _] (swap! capture-help-writes inc))]
    [(with-out-str (m/run ["capture" "--help"] "/tmp/threads" "/tmp/facts.log"))
     (with-out-str (m/run ["capture" "-h"] "/tmp/threads" "/tmp/facts.log"))]))

;; --- admission latency: both hot verbs must ask for ONE SUBJECT --------------
;; A whole-corpus server read is forbidden on this exact-subject path.
(def admission-shows (atom []))
(defn- shows-only [rows]
  {#'m/coord-port (fn [] 7977)
   #'m/coord-subject-propositions
   (fn [_ te]
     (swap! admission-shows conj te)
     (mapv (fn [[predicate value]] (t/triple te predicate value)) rows))
   #'m/coord-live-propositions
   (fn [& _] (throw (ex-info "whole-corpus read on an exact-subject path" {})))})

(def render-rows
  [["title" "Exact subject render"] ["kind" "thread"] ["created_by" "@tom_passarelli"]])
(def rendered (atom nil))
(def capture-render-output
  (with-redefs-fn
    (merge (shows-only render-rows)
           {#'m/uuidv7 (fn [] "019f0000-0000-7000-8000-000000000002")
            #'rt/ensure-dir (fn [_] nil)
            #'rt/today-iso (fn [] "2026-07-19")
            #'rt/now-iso (fn [] "2026-07-19T00:00:00Z")
            #'m/coord-port (fn [] 7977)
            #'m/coord-version (fn [_] 1)
            #'m/tell-retry (fn [& _] "ok:2")
            #'rt/spit-file (fn [_ content] (reset! rendered content) nil)})
    #(with-out-str
       (m/cmd-capture "/tmp/north-capture-test" "/tmp/facts.log"
                      "Exact subject render" "personal"))))

(def json-show-output
  (with-redefs-fn (shows-only [["title" "Exact subject show"] ["kind" "thread"]])
    #(with-out-str (m/cmd-json "/tmp/facts.log" "show" "019f0000-0000-7000-8000-000000000003" false))))

(def checks
  [["capture renders from an exact-subject read, never a whole-log fold"
    (and (some? @rendered)
         (str/includes? @rendered "title  \"Exact subject render\"")
         (str/includes? @rendered "created_by  \"@tom_passarelli\"")
         (str/includes? capture-render-output "captured -> "))]
   ["capture's exact-subject read asks for exactly the thread it just wrote"
    (= "@019f0000-0000-7000-8000-000000000002" (first @admission-shows))]
   ["json show reads one subject, never the whole corpus"
    (= [{:predicate "title" :value "Exact subject show"}
        {:predicate "kind" :value "thread"}]
       (json/parse-string (str/trim json-show-output) true))]
   ["json show asks the coordinator for exactly the requested subject"
    (some #{"@019f0000-0000-7000-8000-000000000003"} @admission-shows)]
   ["uuidv7 is a version-7 uuid"                 (boolean (re-matches uuid-re u1))]
   ["uuidv7 is NOT the old date-id scheme"       (nil? (re-matches date-id-re u1))]
   ["uuidv7 parses as a UUID, version 7"         (= 7 (.version (java.util.UUID/fromString u1)))]
   ["uuidv7 ids are distinct"                    (not= u1 u2)]
   ["uuidv7 is k-sortable (earlier < later)"     (neg? (compare u1 u2))]
   ["capture stamps kind=thread (kind-at-capture)"     (= "thread" (fact-val cap "kind"))]
   ["capture asserts a created_at fact"          (some? (fact-val cap "created_at"))]
   ["created_at is the full-ISO now-iso value"   (= "2026-06-28T07:00:00" (fact-val cap "created_at"))]
   ["created_at (full-ISO) differs from committed (date)"
    (not= (fact-val cap "created_at") (fact-val cap "committed"))]
   ["resolve @handle -> latest-created canonical id"   (= "@id-new" (m/resolve-ref ridx "@perf"))]
   ["resolve bare handle (no @) also resolves"         (= "@id-new" (m/resolve-ref ridx "perf"))]
   ["resolve real @id is passthrough"                  (= "@id-old" (m/resolve-ref ridx "@id-old"))]
   ["resolve titled-but-handleless id is passthrough"  (= "@solo" (m/resolve-ref ridx "@solo"))]
   ["resolve unknown ref is unchanged"                 (= "@ghost" (m/resolve-ref ridx "@ghost"))]
   ["partial structured capture reports failed-but-clean"
    (and (false? (:complete partial-receipt))
         (= "partial-cleaned" (:reason partial-receipt))
         (= 1 (:committed partial-receipt)))]
   ["partial structured capture retracts exactly its acknowledged fact"
    (and (empty? @partial-live)
         (= 1 (count @partial-retracts))
         @partial-view-deleted)]
   ["failed cleanup retraction still attempts every acknowledged fact"
    (and (false? cleanup-all-attempted?)
         (= [["@cleanup-probe" "title" "Cleanup probe"]
             ["@cleanup-probe" "kind" "thread"]]
            @cleanup-attempts))]
   ["capture help is non-mutating for both spellings"
    (and (zero? @capture-help-writes)
         (every? #(str/includes? % "usage: capture <title> [owner]")
                 capture-help-output))]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\ncapture:" (count checks) "/" (count checks) "PASS")
    (do (println "\ncapture:" (count fails) "FAILED") (System/exit 1))))
