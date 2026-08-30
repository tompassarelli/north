(ns north.main
  (:gen-class)
  (:require [store.kernel-classify :as kc]
            [store.types :as t]
            [store.export :as exp]
            [north.projections :as proj]
            [north.validate :as val]
            [north.staleness :as stale]
            [north.audit :as audit]
            [clojure.string :as str]
            [cheshire.core :as core]
            [store.rt :as rt])
  (:import [java.util Random]
           [java.util UUID]))

(defn- ^String getenv-or [^String name ^String default]
  (let [value (store.rt/getenv name)]
  (if (nil? value) default value)))

(defn- ^String to-json [value]
  (cheshire.core/generate-string value))

(defn- ^String threads-dir []
  (getenv-or "BEAGLE_STORE_THREADS" (str (System/getProperty "user.dir") "/threads")))

(defn- ^String log-path []
  (getenv-or "BEAGLE_STORE_LOG" (str (System/getProperty "user.dir") "/coordination.log")))

(defn- ^String string-term [value]
  (if (string? value) value (throw (ex-info "North coordination data requires String Triple terms" {:type :north/non-string-triple}))))

(defn- ^String triple-subject [value]
  (string-term (t/triple-t1 value)))

(defn- ^String triple-predicate [value]
  (string-term (t/triple-t2 value)))

(defn- ^String triple-value [value]
  (string-term (t/triple-t3 value)))

(defn- coord-invoke [^String operation args]
  (let [callable (clojure.core/ns-resolve (symbol "north.coord") (symbol operation))]
  (if (some? callable) (apply callable args) (throw (ex-info "north.coord must be loaded before a data operation" {:type :north/coord-not-loaded :operation operation})))))

(defn- coord-port []
  (let [value (coord-invoke "port!" [])]
  (if (int? value) value (throw (ex-info "north.coord returned an invalid port" {:type :north/invalid-coord-port})))))

(defn- coord-propositions [^String operation args]
  (let [value (coord-invoke operation args)]
  (if (and (vector? value) (every? t/triple? value)) value (throw (ex-info "north.coord returned a malformed Triple projection" {:type :north/invalid-coord-projection :operation operation})))))

(defn- coord-live-propositions [port]
  (coord-propositions "live-propositions!" [port]))

(defn- coord-subject-propositions [port ^String subject]
  (coord-propositions "subject-propositions!" [port subject]))

(defn ^String uuidv7 []
  (let [ts (System/currentTimeMillis)
   r (Random.)
   msb (bit-or (bit-shift-left ts 16) 0x7000 (bit-and (.nextInt r) 0xFFF))
   lsb (bit-or (bit-shift-left 2 62) (bit-and (.nextLong r) 0x3FFFFFFFFFFFFFFF))]
  (str (UUID. msb lsb))))

(defn- ^String title-of [idx ^String te]
  (let [t (proj/string-value-at idx te "title")]
  (if (some? t) t "")))

(defn- ^String short-id [^String te]
  (if (str/starts-with? te "@") (subs te 1) te))

(defn ^String resolve-ref [idx ^String ref]
  (if (some? (proj/string-value-at idx ref "title")) ref (let [^String bare (short-id ref)
   matches (filterv (fn [^String te] (let [h (proj/string-value-at idx te "handle")]
  (and (some? h) (= h bare)))) (proj/thread-subjects idx))]
  (if (empty? matches) (let [pms (if (str/blank? bare) [] (filterv (fn [^String te] (str/starts-with? (short-id te) bare)) (proj/thread-subjects idx)))]
  (if (= (count pms) 1) (first pms) ref)) (reduce (fn [^String best ^String te] (if (str/blank? best) te (let [^String bc (let [c (proj/string-value-at idx best "created_at")]
  (if (some? c) c ""))
   ^String tc (let [c (proj/string-value-at idx te "created_at")]
  (if (some? c) c ""))]
  (if (store.rt/str-lt? bc tc) te best)))) "" matches)))))

(defn- ^String trunc [^String s n]
  (if (> (count s) n) (str (subs s 0 (- n 1)) "…") s))

(defn- ^String legacy-entity-kind [^String ek]
  (cond
  (or (= ek "lane") (or (= ek "managed") (= ek "session"))) "agent"
  (or (= ek "msg") (= ek "command")) "message"
  (= ek "mine") "north/mine"
  :else ek))

(defn- ^String namespace-kind [^String bare]
  (cond
  (str/starts-with? bare "concern-") "concern"
  (str/starts-with? bare "agent:") "agent"
  (or (str/starts-with? bare "msg:") (str/starts-with? bare "cmd:")) "message"
  (str/starts-with? bare "topic-") "topic"
  (str/starts-with? bare "mine:") "north/mine"
  (or (str/starts-with? bare "run-") (str/starts-with? bare "run:")) "run"
  (or (str/starts-with? bare "session:") (or (str/starts-with? bare "sess-") (str/starts-with? bare "cc-"))) "agent"
  (str/starts-with? bare "denial:") "guard_denial"
  (str/starts-with? bare "arena-") "north/arena_run"
  :else ""))

(defn- ^String kind-of [idx te]
  (if (nil? te) "other" (let [explicit (proj/string-value-at idx te "entity_kind")]
  (if (some? explicit) explicit (let [legacy (proj/string-value-at idx te "kind")]
  (if (some? legacy) (legacy-entity-kind legacy) (let [^String np (namespace-kind (short-id te))]
  (if (not (str/blank? np)) np (if (some? (proj/string-value-at idx te "title")) "thread" (if (some? (proj/string-value-at idx te "display_name")) "person" (if (or (some? (proj/string-value-at idx te "cardinality")) (or (some? (proj/string-value-at idx te "value_kind")) (some? (proj/string-value-at idx te "acyclic")))) "predicate" "other")))))))))))

(defn- ^String driver-label [idx ^String te]
  (let [d (proj/string-value-at idx te "driver")]
  (if (nil? d) "" (let [dn (proj/string-value-at idx d "display_name")]
  (if (some? dn) dn (short-id d))))))

(defrecord LevItem [te score])

(defn levitem-te [r] (:te r))

(defn levitem-score [r] (:score r))

(defrecord NextItem [te score leverage urgency priority sequencing basis])

(defn nextitem-te [r] (:te r))

(defn nextitem-score [r] (:score r))

(defn nextitem-leverage [r] (:leverage r))

(defn nextitem-urgency [r] (:urgency r))

(defn nextitem-priority [r] (:priority r))

(defn nextitem-sequencing [r] (:sequencing r))

(defn nextitem-basis [r] (:basis r))

(defrecord QueueDirective [te version position anchor])

(defn queuedirective-te [r] (:te r))

(defn queuedirective-version [r] (:version r))

(defn queuedirective-position [r] (:position r))

(defn queuedirective-anchor [r] (:anchor r))

(defrecord AgendaItem [te do_on])

(defn agendaitem-te [r] (:te r))

(defn agendaitem-do_on [r] (:do_on r))

(defn ^String queue-rank-value [version ^String position ^String anchor]
  (str "v1|" version "|" position "|" (if (str/blank? anchor) "_" anchor)))

(defn- queue-directive [idx ^String te]
  (let [raw (proj/string-value-at idx te "queue_rank")
   parts (if (some? raw) (vec (str/split raw #"\|")) [])
   version (if (= (count parts) 4) (store.rt/parse-int (nth parts 1)) -1)
   ^String position (if (= (count parts) 4) (nth parts 2) "")
   ^String anchor-token (if (= (count parts) 4) (nth parts 3) "")
   ^String anchor (if (= anchor-token "_") "" anchor-token)
   relative? (or (= position "before") (= position "after"))
   edge? (or (= position "first") (= position "last"))]
  (if (and (= (if (some? raw) (nth parts 0) "") "v1") (and (>= version 0) (or (and edge? (str/blank? anchor)) (and relative? (str/starts-with? anchor "@"))))) (->QueueDirective te version position anchor) nil)))

(defn- queue-index [items ^String target]
  (loop [i 0]
  (cond
  (>= i (count items)) -1
  (= (nth items i) target) i
  :else (recur (+ i 1)))))

(defn- queue-insert [items ^String target at]
  (loop [i 0
   result []]
  (if (>= i (count items)) (if (= at i) (conj result target) result) (let [with-target (if (= at i) (conj result target) result)]
  (recur (+ i 1) (conj with-target (nth items i)))))))

(defn- apply-queue-directive [items ^QueueDirective directive]
  (let [^String te (:te directive)
   old-index (queue-index items te)
   remaining (filterv (fn [^String item] (not (= item te))) items)
   anchor-index (queue-index remaining (:anchor directive))
   fallback-index (if (< old-index 0) (count remaining) (if (> old-index (count remaining)) (count remaining) old-index))
   insert-index (cond
  (= (:position directive) "first") 0
  (= (:position directive) "last") (count remaining)
  (and (= (:position directive) "before") (>= anchor-index 0)) anchor-index
  (and (= (:position directive) "after") (>= anchor-index 0)) (+ anchor-index 1)
  :else fallback-index)]
  (queue-insert remaining te insert-index)))

(defn- queue-order-from-base [idx base]
  (let [directives (reduce (fn [found ^String te] (let [directive (queue-directive idx te)]
  (if (some? directive) (conj found directive) found))) [] base)]
  (reduce (fn [ordered ^QueueDirective directive] (apply-queue-directive ordered directive)) base (vec (sort-by (fn [^QueueDirective directive] (:version directive)) directives)))))

(defn queue-order [idx tes]
  (queue-order-from-base idx (vec (sort-by (fn [^String te] (- 0 (proj/leverage-score idx te))) tes))))

(defn- coord-version [port]
  (try
  (let [value (coord-invoke "version!" [port])]
  (if (int? value) value -1))
  (catch Exception _
    -1)))

(defn- live-facts [^String log]
  (let [port (coord-port)
   version (coord-version port)]
  (if (< version 0) (throw (ex-info (str "North Store RPC read failed on 127.0.0.1:" port " (code " version ")") {:type :north/server-unavailable :port port :log log})) (coord-live-propositions port))))

(defn- live-idx [^String log]
  (proj/index-triples (live-facts log)))

(defn- live-subject-facts [^String log ^String te]
  (try
  (coord-subject-propositions (coord-port) te)
  (catch Exception _
    [])))

(defn- subject-facts [^String log ^String te]
  (let [warm (live-subject-facts log te)]
  (if (empty? warm) (filterv (fn [triple] (= te (triple-subject triple))) (live-facts log)) warm)))

(defn ^String store-rpc-failure-message [code port ^String log ^String consequence]
  (let [^String summary (cond
  (= code -1) (str "Store RPC SERVER UNREACHABLE on 127.0.0.1:" port)
  (= code -2) (str "Store RPC SPACE MISMATCH on 127.0.0.1:" port " (this command selected Store log database " log ")")
  (= code -3) (str "Store RPC PROTOCOL INCOMPATIBLE on 127.0.0.1:" port)
  :else (str "Store RPC preflight failed on 127.0.0.1:" port " (code " code ")"))
   ^String remedy (cond
  (= code -1) "Start the configured Beagle Store service"
  (= code -2) "Select the intended Store log database and SpaceId before retrying"
  (= code -3) "Use one matched North + Beagle Store release"
  :else "Inspect `north doctor` before retrying")]
  (str summary (if (str/blank? consequence) "" (str " — " consequence)) ". " remedy ".")))

(defn- ^String tell-once [port ^String log ^String op ^String te ^String pred ^String rv]
  (let [v (coord-version port)]
  (if (< v 0) "server-unavailable" (try
  (let [response (if (= op "assert") (coord-invoke "assert-at-version!" [port te pred rv v]) (coord-invoke "retract-at-version!" [port te pred rv v]))
   committed (:ok response)
   rejected (:reject response)]
  (if (some? committed) (str "ok:" committed) (if (= rejected :conflict) "conflict" (str rejected))))
  (catch Exception _
    "server-unavailable")))))

(defn- ^String tell-retry [port ^String log ^String op ^String te ^String pred ^String rv tries]
  (let [^String resp (tell-once port log op te pred rv)]
  (if (and (= resp "conflict") (> tries 0)) (tell-retry port log op te pred rv (- tries 1)) resp)))

(defn- ^Boolean ctrl? [^String s]
  (or (str/includes? s "\n") (str/includes? s "\r")))

(defn- add-fact [acc ^String te ^String p ^String v]
  (if (str/blank? v) acc (conj acc (t/triple te p v))))

(defn- ^String ref-or-blank [^String v]
  (if (str/blank? v) "" (str "@" v)))

(defn- capture-facts [^String te ^String title ^String owner ^String source ^String author ^String lead ^String proposed ^String created-at ^String today]
  (let [c (add-fact [] te "title" title)
   c (add-fact c te "kind" "thread")
   c (if (= owner "personal") c (add-fact c te "owner" owner))
   c (add-fact c te "source" source)
   c (add-fact c te "created_by" (ref-or-blank author))
   c (add-fact c te "lead" (ref-or-blank lead))
   c (add-fact c te "proposed_by" (ref-or-blank proposed))
   c (add-fact c te "created_at" created-at)
   c (add-fact c te "updated_at" today)
   c (add-fact c te "committed" today)]
  c))

(defrecord CaptureReceipt [id thread title path expected committed complete reason])

(defn capturereceipt-id [r] (:id r))

(defn capturereceipt-thread [r] (:thread r))

(defn capturereceipt-title [r] (:title r))

(defn capturereceipt-path [r] (:path r))

(defn capturereceipt-expected [r] (:expected r))

(defn capturereceipt-committed [r] (:committed r))

(defn capturereceipt-complete [r] (:complete r))

(defn capturereceipt-reason [r] (:reason r))

(defn- ^Boolean structured-capture? []
  (= "1" (getenv-or "NORTH_CAPTURE_STRUCTURED" "")))

(defn- print-capture-receipt [^String id ^String te ^String title ^String path expected committed ^Boolean complete ^String reason]
  (println (to-json (->CaptureReceipt id te title path expected committed complete reason))))

(defn- ^Boolean retract-committed-capture-facts [port ^String log facts results i]
  (if (>= i (count facts)) true (let [fact (nth facts i)
   ^String result (nth results i)
   current-ok (if (str/starts-with? result "ok:") (str/starts-with? (tell-retry port log "retract" (triple-subject fact) (triple-predicate fact) (triple-value fact) 5) "ok:") true)
   remaining-ok (retract-committed-capture-facts port log facts results (+ i 1))]
  (and current-ok remaining-ok))))

(defn- ^Boolean cleanup-partial-capture [port ^String log ^String te ^String path facts results]
  (let [retracted (retract-committed-capture-facts port log facts results 0)
   _ (store.rt/delete-file path)
   remaining (filterv (fn [fact] (= te (triple-subject fact))) (coord-live-propositions port))]
  (and retracted (empty? remaining) (not (store.rt/file-exists path)))))

(defn cmd-capture [^String threads-dir ^String log ^String title ^String owner]
  (let [^String source (getenv-or "NORTH_SOURCE" "self")
   ^String author (getenv-or "NORTH_AUTHOR" "you")
   ^String lead (getenv-or "NORTH_LEAD" "")
   ^String proposed (getenv-or "NORTH_PROPOSED_BY" "")]
  (cond
  (or (str/blank? title) (ctrl? title)) (println "usage: capture <title> [owner]   (title must be a non-empty single line)")
  (ctrl? owner) (println "capture: owner must be a single line")
  (or (ctrl? source) (ctrl? author) (ctrl? lead) (ctrl? proposed)) (println "capture: NORTH_SOURCE/AUTHOR/LEAD/PROPOSED_BY must each be a single line")
  :else (do
  (store.rt/ensure-dir threads-dir)
  (let [^String id (uuidv7)
   ^String slug (store.rt/slugify title)
   ^String today (store.rt/today-iso)
   ^String created-at (store.rt/now-iso)
   ^String te (str "@" id)
   ^String path (str threads-dir "/" id "-" slug ".md")
   port (coord-port)
   server-v (coord-version port)]
  (if (< server-v 0) (if (structured-capture?) (print-capture-receipt id te title path 0 0 false "store-rpc-unavailable") (println (store-rpc-failure-message server-v port log "capture was not recorded"))) (let [facts (capture-facts te title owner source author lead proposed created-at today)
   results (mapv (fn [c] (tell-retry port log "assert" (triple-subject c) (triple-predicate c) (triple-value c) 5)) facts)
   oks (count (filterv (fn [^String r] (str/starts-with? r "ok:")) results))]
  (if (= oks (count facts)) (do
  (store.rt/spit-file path (exp/thread-md (let [warm (live-subject-facts log te)]
  (if (empty? warm) facts warm)) te))
  (if (structured-capture?) (print-capture-receipt id te title path (count facts) oks true "captured") (println (str "captured -> " te "  " title "  [owner: " owner "]\n" "  file:      " path "\n" "  committed: " oks " facts via Store RPC. Next: north tell " id " <pred> <value>")))) (if (structured-capture?) (let [cleaned (cleanup-partial-capture port log te path facts results)]
  (print-capture-receipt id te title path (count facts) oks false (if cleaned "partial-cleaned" "partial-cleanup-failed"))) (println (str "capture PARTIAL: only " oks "/" (count facts) " fact(s) committed (Store RPC publication failure). Re-run — nothing is stranded in files.")))))))))))

(defn- ^Boolean id-like? [^String bare]
  (and (not (str/blank? bare)) (str/blank? (str/replace bare #"[0-9a-f-]" "")) (or (str/includes? bare "-") (>= (count bare) 8))))

(defn cmd-resolve [^String log ^String ref]
  (let [idx (live-idx log)
   ^String r (resolve-ref idx ref)]
  (if (and (= r ref) (id-like? (short-id ref)) (nil? (proj/string-value-at idx (str "@" (short-id ref)) "title"))) (println (str "ERROR unresolved id-like ref " ref " — not a thread id, unique prefix, or handle" " (ambiguous/truncated? `north show " (short-id ref) "` lists candidates)")) (println r))))

(defn cmd-done-bars [^String log ^String ref]
  (let [idx (live-idx log)
   ^String te (resolve-ref idx (if (str/starts-with? ref "@") ref (str "@" ref)))
   bars (proj/string-values-at idx te "done_when")
   evs (proj/string-values-at idx te "bar_evidence")]
  (if (empty? bars) nil (do
  (println (str "DONE BARS on " te " — this outcome claims they are met; cite probe + observed result:"))
  (doseq [b bars]
  (println (str "  " (stale/bar-mark evs b) " " b)))
  (println (str "  evidence: north tell " (short-id te) " bar_evidence \"<bar> → <observed result>\""))))))

(defn cmd-audit [^String log]
  (let [idx (live-idx log)
   rd (audit/repo-drift idx)]
  (println (str "REPO DRIFT — " (count rd) " group(s):"))
  (doseq [g rd]
  (println (str "  " (:norm g) ": " (str/join ", " (:forms g)))))))

(defn cmd-validate [^String log]
  (let [idx (live-idx log)
   ids (proj/thread-subjects idx)
   problems (reduce (fn [acc ^String te] (reduce (fn [a ^String v] (conj a (str (short-id te) ": " v))) acc (val/violations-i idx te))) [] ids)]
  (if (empty? problems) (do
  (println (str "OK — " (count ids) " threads, no violations."))
  0) (do
  (doseq [p problems]
  (println (str "  " p)))
  (println (str (count problems) " violation(s)."))
  1))))

(defn- ^Boolean valid-session-lease? [^String driverref lease now-ms]
  (let [^String handle (short-id driverref)]
  (and (t/triple? lease) (and (= handle (t/triple-t1 lease)) (and (= :kernel/expires-at (t/triple-t2 lease)) (and (integer? (t/triple-t3 lease)) (> (t/triple-t3 lease) now-ms)))))))

(defn- ^String driver-activity [idx ^String te now-ms]
  (let [drivers (proj/values-at idx te "driver")]
  (cond
  (empty? drivers) proj/absent-proven
  (not (= 1 (count drivers))) proj/unresolved
  :else (let [driver (first drivers)]
  (if (not (string? driver)) proj/unresolved (let [leases (proj/values-at idx (str "session:" (short-id driver)) :kernel/lease)]
  (if (and (= 1 (count leases)) (valid-session-lease? driver (first leases) now-ms)) proj/live-proven proj/unresolved)))))))

(defn- driver-activity-pred [now-ms]
  (fn [idx ^String te] (driver-activity idx te now-ms)))

(defn- default-activity []
  (driver-activity-pred (System/currentTimeMillis)))

(defn- ^Boolean unresolved-assignment? [idx ^String te activity]
  (= (activity idx te) proj/unresolved))

(defn cmd-ready [^String log ^Boolean all]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   activity (default-activity)
   raw (proj/ready idx today store.rt/str-lt? activity)
   rs (if all raw (filterv (fn [^String te] (= (kind-of idx te) "thread")) raw))
   ranked (queue-order idx rs)
   shown (if all ranked (vec (take 15 ranked)))
   rs-count (count rs)
   shown-count (count shown)]
  (if all (println (str "READY NOW — " (count rs))) (println (str "READY NOW — top " (count shown) " of " (count rs) " by queue order (leverage fallback)")))
  (println "  ready = committed + unblocked + assignment absence proved + not future-scheduled (vs open = merely nonterminal)")
  (doseq [te shown]
  (println (str "  " (short-id te) "  " (trunc (title-of idx te) 56))))
  (if (and (not all) (> rs-count shown-count)) (do
  (println (str "  … +" (- rs-count shown-count) " more · north ready --all"))))))

(defn cmd-blocked [^String log]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)
   bs (filterv (fn [^String te] (= (proj/condition-i idx te today before? activity) "blocked")) (proj/work-thread-ids-i idx))]
  (println (str "BLOCKED — " (count bs)))
  (doseq [te bs]
  (println (str "  " (short-id te) "  " (trunc (title-of idx te) 48) "  (waiting on " (count (proj/incomplete-deps idx te)) ")")))))

(defn cmd-leverage [^String log]
  (let [idx (live-idx log)
   cands (filterv (fn [^String te] (not (proj/terminal-i? idx te))) (proj/work-thread-ids-i idx))
   items (filterv (fn [^LevItem it] (> (:score it) 0)) (mapv (fn [^String te] (->LevItem te (proj/leverage-score idx te))) cands))
   ranked (vec (take 15 (sort-by (fn [^LevItem it] (- 0 (:score it))) items)))]
  (println "TOP UNBLOCKERS — finishing this transitively frees the most stuck threads")
  (doseq [it ranked]
  (println (str "  unblocks " (:score it) "  " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 46))))))

(defn- ^NextItem next-item [idx ^String te ^String today before? activity]
  (let [lev (proj/leverage-score idx te)
   doo (proj/string-value-at idx te "do_on")
   urg (if (some? doo) (cond
  (store.rt/str-lt? doo today) 5
  (= doo today) 3
  :else 0) 0)
   ^String pri (let [p (proj/string-value-at idx te "priority")]
  (if (some? p) p ""))
   sequencing (count (proj/incomplete-deps idx te))
   eligibility (proj/explain idx te today before? activity)]
  (->NextItem te (+ (* 3 lev) urg) lev urg pri sequencing (:reason eligibility))))

(defn cmd-next [^String log]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)
   items (mapv (fn [^String te] (next-item idx te today before? activity)) (proj/ready idx today before? activity))
   score-order (mapv (fn [^NextItem it] (:te it)) (vec (sort-by (fn [^NextItem it] (- 0 (:score it))) items)))
   ranked (mapv (fn [^String te] (next-item idx te today before? activity)) (vec (take 12 (queue-order-from-base idx score-order))))]
  (println (str "WHAT TO WORK ON — top picks (" today ")"))
  (println "  eligible = ready (committed + unblocked + assignment absence proved + not scheduled-later)")
  (println "  manual queue order is primary · fallback score = 3·graph-leverage + do_on urgency")
  (println "  stored priority is orthogonal human intent (shown, never silently scored)")
  (doseq [it ranked]
  (println (str "  [" (:score it) "] " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 46)))
  (println (str "      eligible: " (:basis it)))
  (println (str "      score: 3×" (:leverage it) " leverage + " (:urgency it) " urgency = " (:score it) " · sequencing: " (:sequencing it) " incomplete deps" " · priority: " (if (str/blank? (:priority it)) "none" (:priority it)) " (not scored)")))))

(defn cmd-agenda [^String log]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   cands (filterv (fn [^String te] (and (not (proj/terminal-i? idx te)) (some? (proj/string-value-at idx te "do_on")))) (proj/work-thread-ids-i idx))
   items (mapv (fn [^String te] (->AgendaItem te (let [d (proj/string-value-at idx te "do_on")]
  (if (some? d) d "")))) cands)
   overdue (vec (sort-by (fn [^AgendaItem it] (:do_on it)) (filterv (fn [^AgendaItem it] (store.rt/str-lt? (:do_on it) today)) items)))
   todayb (filterv (fn [^AgendaItem it] (= (:do_on it) today)) items)
   upcoming (vec (sort-by (fn [^AgendaItem it] (:do_on it)) (filterv (fn [^AgendaItem it] (store.rt/str-lt? today (:do_on it))) items)))]
  (println (str "AGENDA — " today))
  (println (str "OVERDUE (" (count overdue) ")"))
  (doseq [it overdue]
  (println (str "  " (:do_on it) "  " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 44))))
  (println (str "TODAY (" (count todayb) ")"))
  (doseq [it todayb]
  (println (str "  " (:do_on it) "  " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 44))))
  (println (str "UPCOMING (" (count upcoming) ")"))
  (doseq [it upcoming]
  (println (str "  " (:do_on it) "  " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 44))))))

(defn- board-group [idx ^String label grp]
  (if (not (empty? grp)) (do
  (println (str "\n" (proj/condition-emoji idx label) " " label " (" (count grp) ")"))
  (doseq [te grp]
  (println (str "  " (short-id te) "  " (trunc (title-of idx te) 52)))))))

(defn- in-condition [idx nonterm ^String today before? activity ^String c]
  (filterv (fn [^String te] (= (proj/condition-i idx te today before? activity) c)) nonterm))

(defn- board-full [idx ^String today before? activity nonterm]
  (do
  (println (str "THREADS — " (count nonterm) " open"))
  (board-group idx "active" (in-condition idx nonterm today before? activity "active"))
  (board-group idx "unresolved" (in-condition idx nonterm today before? activity "unresolved"))
  (board-group idx "ready" (queue-order idx (in-condition idx nonterm today before? activity "ready")))
  (board-group idx "blocked" (in-condition idx nonterm today before? activity "blocked"))
  (board-group idx "dormant" (in-condition idx nonterm today before? activity "dormant"))
  (board-group idx "draft" (in-condition idx nonterm today before? activity "draft"))))

(defn- board-curated [idx ^String today before? activity nonterm]
  (let [threads (filterv (fn [^String te] (= (kind-of idx te) "thread")) nonterm)
   active (in-condition idx threads today before? activity "active")
   unresolved (in-condition idx threads today before? activity "unresolved")
   nunresolved (count unresolved)
   readyl (in-condition idx threads today before? activity "ready")
   blockedl (in-condition idx threads today before? activity "blocked")
   nconcern (count (filterv (fn [^String s] (= (kind-of idx s) "concern")) (proj/all-subjects idx)))
   ashow (vec (take 20 active))
   active-count (count active)
   ashow-count (count ashow)
   rranked (mapv (fn [^String te] (->LevItem te (proj/leverage-score idx te))) (vec (take 15 (queue-order idx readyl))))
   readyl-count (count readyl)
   rranked-count (count rranked)]
  (println (str "THREADS — " (count threads) " open threads · " (count active) " active · " nunresolved " unresolved · " (count readyl) " ready · " (count blockedl) " blocked · " nconcern " concerns   (north threads --all for the full kanban)"))
  (println "  open = not terminal · active = lease-proven driver · unresolved = assigned without liveness proof · ready = assignment absence proved")
  (if (not (empty? active)) (do
  (println (str "\n" (proj/condition-emoji idx "active") " ACTIVE — who's on what (" (count active) ")"))
  (doseq [te ashow]
  (println (str "  " (let [^String dl (driver-label idx te)]
  (if (str/blank? dl) "?" dl)) "  " (short-id te) "  " (trunc (title-of idx te) 44))))
  (if (> active-count ashow-count) (do
  (println (str "  … +" (- active-count ashow-count) " more · north threads --all"))))))
  (if (> nunresolved 0) (do
  (let [ushow (vec (take 10 unresolved))
   ushow-count (count ushow)]
  (println (str "\n" (proj/condition-emoji idx "unresolved") " UNRESOLVED — assignment retained without positive liveness or absence proof (" nunresolved ")"))
  (doseq [te ushow]
  (println (str "  " (driver-label idx te) "  " (short-id te) "  " (trunc (title-of idx te) 36))))
  (if (> nunresolved ushow-count) (do
  (println (str "  … +" (- nunresolved ushow-count) " more · north needs-review")))))))
  (println (str "\n" (proj/condition-emoji idx "ready") " READY — top " (count rranked) " of " (count readyl) " by queue order (leverage fallback)"))
  (doseq [it rranked]
  (println (str "  unblocks " (:score it) "  " (short-id (:te it)) "  " (trunc (title-of idx (:te it)) 44))))
  (if (> readyl-count rranked-count) (do
  (println (str "  … +" (- readyl-count rranked-count) " more · north threads --all"))))
  (println "  machinery/agents/daemons → north dashboard")))

(defn cmd-board [^String log ^Boolean all]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)
   nonterm (filterv (fn [^String te] (not (proj/terminal-i? idx te))) (proj/work-thread-ids-i idx))]
  (if all (board-full idx today before? activity nonterm) (board-curated idx today before? activity nonterm))))

(defn- ^String cockpit-thread-line [idx ^String te ^String today before? activity]
  (let [^String condition (proj/condition-i idx te today before? activity)
   owner (proj/string-value-at idx te "owner")
   estimate (proj/string-value-at idx te "estimate_hours")
   deps (proj/incomplete-deps idx te)]
  (str "  " (short-id te) "  " condition " · " (trunc (title-of idx te) 38) " · owner " (if (some? owner) owner "personal") " · blocked by " (count deps) (if (some? estimate) (str " · est " estimate "h") ""))))

(defn- cockpit-exact-value [idx ^String subject ^String predicate]
  (let [values (proj/string-values-at idx subject predicate)]
  (if (= (count values) 1) (first values) nil)))

(defn- ^Boolean cockpit-account? [idx ^String account]
  (and (str/starts-with? account "@account:") (= (cockpit-exact-value idx account "kind") "provider_account") (some? (cockpit-exact-value idx account "account_id")) (some? (cockpit-exact-value idx account "provider")) (some? (cockpit-exact-value idx account "provider_profile")) (or (= (cockpit-exact-value idx account "account_role") "execution") (= (cockpit-exact-value idx account "account_role") "oversight")) (or (= (cockpit-exact-value idx account "execution_eligible") "true") (= (cockpit-exact-value idx account "execution_eligible") "false"))))

(defn- ^Boolean cockpit-attempt? [idx ^String attempt]
  (and (str/starts-with? attempt "@attempt:") (= (cockpit-exact-value idx attempt "kind") "execution_attempt") (some? (cockpit-exact-value idx attempt "execution_attempt_manifest_sha256")) (some? (cockpit-exact-value idx attempt "execution_attempt_run")) (some? (cockpit-exact-value idx attempt "execution_attempt_thread")) (some? (cockpit-exact-value idx attempt "execution_attempt_account"))))

(defn- ^String cockpit-attempt-state [idx ^String attempt]
  (cond
  (some? (cockpit-exact-value idx attempt "execution_attempt_terminal_manifest_sha256")) "terminal"
  (some? (cockpit-exact-value idx attempt "execution_attempt_unsent_manifest_sha256")) "proved-unsent"
  (some? (cockpit-exact-value idx attempt "execution_attempt_provider_start_manifest_sha256")) "provider-started"
  (some? (cockpit-exact-value idx attempt "execution_attempt_launch_intent_sha256")) "launch-intent"
  :else "reserved"))

(defn- ^Boolean cockpit-command? [idx ^String command ^String manifest]
  (and (str/starts-with? command "@bridge-command:") (= (cockpit-exact-value idx command "bridge.command/attempt-id") manifest) (some? (cockpit-exact-value idx command "bridge.command/kind"))))

(defn- ^Boolean cockpit-cancel-command? [idx ^String command]
  (let [kind (cockpit-exact-value idx command "bridge.command/kind")]
  (or (= kind "interrupt-turn") (= kind "redirect-now") (= kind "terminate-session"))))

(defn- cockpit-command-ordinal [idx ^String command]
  (let [ordinal (cockpit-exact-value idx command "bridge.command/ordinal")]
  (store.rt/parse-int (if (some? ordinal) ordinal "0"))))

(defn- cockpit-command-next [idx commands ^Boolean cancel]
  (let [pending (filterv (fn [^String command] (and (if cancel (cockpit-cancel-command? idx command) (not (cockpit-cancel-command? idx command))) (nil? (cockpit-exact-value idx command "bridge.command/delivery-receipt")))) commands)]
  (if (empty? pending) nil (let [^String command (first (sort-by (fn [^String candidate] (cockpit-command-ordinal idx candidate)) pending))
   ^String kind (let [value (cockpit-exact-value idx command "bridge.command/kind")]
  (if (some? value) value "unknown"))]
  (if (some? (cockpit-exact-value idx command "bridge.command/delivery-intent")) (str "reconcile-command/" kind) (if cancel (str "cancel/" kind) (str "send/" kind)))))))

(defn- ^String cockpit-safe-next [idx ^String account attempt]
  (let [role (cockpit-exact-value idx account "account_role")
   eligible (cockpit-exact-value idx account "execution_eligible")]
  (cond
  (= role "oversight") "no-op/oversight-account"
  (not (= eligible "true")) "no-op/account-ineligible"
  (nil? attempt) "reserve/no-attempt"
  :else (let [terminal? (some? (cockpit-exact-value idx attempt "execution_attempt_terminal_manifest_sha256"))
   unsent? (some? (cockpit-exact-value idx attempt "execution_attempt_unsent_manifest_sha256"))
   launch? (some? (cockpit-exact-value idx attempt "execution_attempt_launch_intent_sha256"))
   started? (some? (cockpit-exact-value idx attempt "execution_attempt_provider_start_manifest_sha256"))
   ^String manifest (let [value (cockpit-exact-value idx attempt "execution_attempt_manifest_sha256")]
  (if (some? value) value ""))
   commands (filterv (fn [^String subject] (cockpit-command? idx subject manifest)) (proj/all-subjects idx))
   cancel-next (cockpit-command-next idx commands true)
   command-next (cockpit-command-next idx commands false)]
  (cond
  terminal? "no-op/settled"
  unsent? "no-op/proved-unsent"
  (not launch?) "launch/reserved"
  (not started?) "reconcile-launch/awaiting-provider-start"
  (some? cancel-next) cancel-next
  (some? command-next) command-next
  :else "no-op/nothing-pending")))))

(defn- ^Boolean cockpit-replay-event-at? [idx ^String run sequence]
  (not (empty? (filterv (fn [^String event] (and (= (cockpit-exact-value idx event "wire_run_id") run) (= (cockpit-exact-value idx event "wire_event_sequence") (str sequence)) (some? (cockpit-exact-value idx event "wire_event_json")) (some? (cockpit-exact-value idx event "wire_event_sha256")))) (proj/all-subjects idx)))))

(defn- cockpit-greatest-replay-position [idx ^String run]
  (loop [sequence 0
   remaining (count (proj/all-subjects idx))]
  (if (or (= remaining 0) (not (cockpit-replay-event-at? idx run sequence))) (- sequence 1) (recur (+ sequence 1) (- remaining 1)))))

(defn- ^String cockpit-attempt-line [idx ^String attempt]
  (let [run (cockpit-exact-value idx attempt "execution_attempt_run")
   thread (cockpit-exact-value idx attempt "execution_attempt_thread")
   account (cockpit-exact-value idx attempt "execution_attempt_account")
   thread-lease (cockpit-exact-value idx attempt "execution_attempt_thread_lease")
   account-lease (cockpit-exact-value idx attempt "execution_attempt_account_lease")
   replay (if (some? run) (cockpit-greatest-replay-position idx run) -1)
   ^String safe-next (if (and (some? account) (cockpit-account? idx account)) (cockpit-safe-next idx account attempt) "invalid/missing-account-authority")]
  (str "  " (short-id attempt) " · " (cockpit-attempt-state idx attempt) " · thread " (if (some? thread) (short-id thread) "—") " · account " (if (some? account) (short-id account) "—") " · thread lease " (if (some? thread-lease) thread-lease "—") " · account lease " (if (some? account-lease) account-lease "—") " · replay " (if (>= replay 0) (str replay) "none") " · safe-next " safe-next)))

(defn cmd-cockpit [^String log]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)
   threads (filterv (fn [^String te] (= (kind-of idx te) "thread")) (proj/work-thread-ids-i idx))
   open (filterv (fn [^String te] (not (proj/terminal-i? idx te))) threads)
   sessions (filterv (fn [^String te] (or (= (kind-of idx te) "session") (some? (proj/string-value-at idx te "current_thread")) (some? (proj/string-value-at idx te "session_identity")))) (proj/all-subjects idx))
   accounts (filterv (fn [^String te] (cockpit-account? idx te)) (proj/all-subjects idx))
   attempts (filterv (fn [^String te] (cockpit-attempt? idx te)) (proj/all-subjects idx))
   landings (vec (take 8 (reverse (sort-by (fn [^String te] (let [at (proj/string-value-at idx te "updated_at")]
  (if (some? at) at ""))) (filterv (fn [^String te] (and (some? (proj/string-value-at idx te "candidate_rev")) (some (fn [^String reached] (= reached "landed")) (proj/string-values-at idx te "reached")))) (proj/all-subjects idx))))))]
  (println (str "NORTH LIVE — " (count open) " open threads · Store facts at read time"))
  (println "THREADS")
  (doseq [te (take 12 (sort-by (fn [^String te] (title-of idx te)) open))]
  (println (cockpit-thread-line idx te today before? activity)))
  (println "SESSIONS")
  (if (empty? sessions) (println "  no live-session fact projection") (doseq [te (take 12 sessions)]
  (let [thread (proj/string-value-at idx te "current_thread")
   parent (proj/string-value-at idx te "parent_thread")]
  (println (str "  " (short-id te) " · thread " (if (some? thread) (short-id thread) "—") " · parent " (if (some? parent) (short-id parent) "—"))))))
  (println "ACCOUNTS")
  (if (empty? accounts) (println "  no complete provider-account authority facts") (doseq [te (take 12 accounts)]
  (let [role (cockpit-exact-value idx te "account_role")
   eligible (cockpit-exact-value idx te "execution_eligible")
   headroom (proj/string-value-at idx te "headroom")
   usage (proj/string-value-at idx te "used_percent")
   account-attempts (filterv (fn [^String attempt] (= (cockpit-exact-value idx attempt "execution_attempt_account") te)) attempts)]
  (println (str "  " (short-id te) " · role " (if (some? role) role "—") " · execution-eligible " (if (some? eligible) eligible "—") " · headroom " (if (some? headroom) headroom "—") " · used " (if (some? usage) usage "—") " · safe-next " (cockpit-safe-next idx te (if (empty? account-attempts) nil (first account-attempts))))))))
  (println "ATTEMPTS")
  (if (empty? attempts) (println "  no complete execution-attempt facts") (doseq [attempt (take 12 (sort-by (fn [^String subject] (let [at (cockpit-exact-value idx subject "execution_attempt_reserved_at")]
  (if (some? at) at ""))) attempts))]
  (println (cockpit-attempt-line idx attempt))))
  (println "LATEST PUBLIC LANDINGS")
  (if (empty? landings) (println "  no landed candidate revision facts") (doseq [te landings]
  (let [rev (proj/string-value-at idx te "candidate_rev")]
  (println (str "  " (if (some? rev) (trunc rev 12) "—") " · " (trunc (title-of idx te) 54))))))
  (println "CONTROL")
  (println "  north bridge  — select a live row, type a message, or use /interrupt")
  (println "  north bridge attach <execution-id> [--cursor N]")
  (println "  north bridge msg <execution-id> <message> · north bridge interrupt <execution-id>")))

(defrecord JThread [id title condition emoji])

(defn jthread-id [r] (:id r))

(defn jthread-title [r] (:title r))

(defn jthread-condition [r] (:condition r))

(defn jthread-emoji [r] (:emoji r))

(defrecord JPresentation [active unresolved ready blocked draft])

(defn jpresentation-active [r] (:active r))

(defn jpresentation-unresolved [r] (:unresolved r))

(defn jpresentation-ready [r] (:ready r))

(defn jpresentation-blocked [r] (:blocked r))

(defn jpresentation-draft [r] (:draft r))

(defrecord JReview [id title pred detail])

(defn jreview-id [r] (:id r))

(defn jreview-title [r] (:title r))

(defn jreview-pred [r] (:pred r))

(defn jreview-detail [r] (:detail r))

(defrecord JFact [predicate value])

(defn jfact-predicate [r] (:predicate r))

(defn jfact-value [r] (:value r))

(defrecord JSubjectFact [subject predicate value])

(defn jsubjectfact-subject [r] (:subject r))

(defn jsubjectfact-predicate [r] (:predicate r))

(defn jsubjectfact-value [r] (:value r))

(defrecord JChildSettlementProjection [protocol version coordinator children runs])

(defn jchildsettlementprojection-protocol [r] (:protocol r))

(defn jchildsettlementprojection-version [r] (:version r))

(defn jchildsettlementprojection-coordinator [r] (:coordinator r))

(defn jchildsettlementprojection-children [r] (:children r))

(defn jchildsettlementprojection-runs [r] (:runs r))

(defrecord JAgentFact [id predicate value])

(defn jagentfact-id [r] (:id r))

(defn jagentfact-predicate [r] (:predicate r))

(defn jagentfact-value [r] (:value r))

(defn- ^JThread jthread [idx ^String te ^String today before? activity]
  (let [^String c (proj/condition-i idx te today before? activity)]
  (->JThread (short-id te) (title-of idx te) c (proj/condition-emoji idx c))))

(defn- ready-curated-tes [idx ^String today before? activity ^Boolean all?]
  (let [raw (proj/ready idx today before? activity)
   rs (if all? raw (filterv (fn [^String te] (= (kind-of idx te) "thread")) raw))
   ranked (queue-order idx rs)]
  (if all? ranked (vec (take 15 ranked)))))

(defn- board-curated-tes [idx ^String today before? activity ^Boolean all?]
  (let [nonterm (filterv (fn [^String te] (not (proj/terminal-i? idx te))) (proj/work-thread-ids-i idx))]
  (if all? nonterm (let [threads (filterv (fn [^String te] (= (kind-of idx te) "thread")) nonterm)
   active (in-condition idx threads today before? activity "active")
   unresolved (in-condition idx threads today before? activity "unresolved")
   ready (vec (take 15 (queue-order idx (in-condition idx threads today before? activity "ready"))))]
  (vec (concat active (vec (concat unresolved ready))))))))

(defn- recent-terminal-tes [idx]
  (let [terminal (filterv (fn [^String te] (and (= (kind-of idx te) "thread") (proj/terminal-i? idx te))) (proj/work-thread-ids-i idx))]
  (vec (take 15 (reverse (sort-by (fn [^String te] (let [updated (proj/string-value-at idx te "updated_at")
   created (proj/string-value-at idx te "created_at")]
  (str (if (some? updated) updated "") "|" (if (some? created) created "") "|" te))) terminal))))))

(defn- matching-subjects [facts ^String predicate ^String value]
  (reduce (fn [subjects fact] (if (and (= (triple-predicate fact) predicate) (= (triple-value fact) value)) (assoc subjects (triple-subject fact) true) subjects)) {} facts))

(defn- direct-child-subjects [facts ^String coordinator]
  (reduce (fn [subjects fact] (if (and (= (triple-predicate fact) "coordinator") (= (triple-value fact) coordinator) (str/starts-with? (triple-subject fact) "@agent:")) (assoc subjects (triple-subject fact) true) subjects)) {} facts))

(defn- child-agent-ids [subjects]
  (reduce-kv (fn [ids ^String subject ^Boolean _present] (assoc ids (subs subject (count "@agent:")) true)) {} subjects))

(defn- child-run-subjects [facts children committed-runs]
  (reduce (fn [subjects fact] (if (and (= (triple-predicate fact) "agent") (get children (triple-value fact) false) (get committed-runs (triple-subject fact) false)) (assoc subjects (triple-subject fact) true) subjects)) {} facts))

(defn- subject-fact-projection [facts subjects]
  (mapv (fn [fact] (->JSubjectFact (short-id (triple-subject fact)) (triple-predicate fact) (triple-value fact))) (filterv (fn [fact] (get subjects (triple-subject fact) false)) facts)))

(defn- unresolved-assignment-reviews [idx ^String today before? activity]
  (reduce (fn [acc ^String te] (if (and (= (kind-of idx te) "thread") (and (not (proj/terminal-i? idx te)) (unresolved-assignment? idx te activity))) (let [d (proj/string-value-at idx te "driver")
   eligibility (proj/explain idx te today before? activity)]
  (conj acc (stale/->Review te "driver" (str "unresolved assignment " (if (some? d) d "?") " has no valid live lease; lifecycle=" (:state eligibility) " — reassign or retract driver")))) acc)) [] (proj/work-thread-ids-i idx)))

(defn- canonical-grooming-reviews [idx live-idx ^String today before? activity]
  (let [base (vec (concat (stale/time-stale idx today before?) (vec (concat (stale/edge-stale idx) (vec (concat (stale/bars-missing idx) (stale/bars-unevidenced idx)))))))
   live-base (filterv (fn [rv] (if (= (:pred rv) "done_when") (= (activity live-idx (:te rv)) proj/live-proven) true)) base)]
  (vec (concat live-base (unresolved-assignment-reviews live-idx today before? activity)))))

(defn- cmd-json-show [^String log ^String arg]
  (println (to-json (mapv (fn [c] (->JFact (triple-predicate c) (triple-value c))) (subject-facts log (str "@" arg))))))

(defn- cmd-json-database [^String log ^String what ^String arg ^Boolean all?]
  (let [facts (live-facts log)
   idx (proj/index-triples facts)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)]
  (cond
  (= what "board") (println (to-json (mapv (fn [^String te] (jthread idx te today before? activity)) (board-curated-tes idx today before? activity all?))))
  (= what "ready") (println (to-json (mapv (fn [^String te] (jthread idx te today before? activity)) (ready-curated-tes idx today before? activity all?))))
  (= what "blocked") (println (to-json (mapv (fn [^String te] (jthread idx te today before? activity)) (filterv (fn [^String te] (= (proj/condition-i idx te today before? activity) "blocked")) (proj/work-thread-ids-i idx)))))
  (= what "done") (println (to-json (mapv (fn [^String te] (jthread idx te today before? activity)) (recent-terminal-tes idx))))
  (= what "needs-review") (let [reviews (canonical-grooming-reviews idx idx today before? activity)]
  (println (to-json (mapv (fn [rv] (->JReview (short-id (:te rv)) (title-of idx (:te rv)) (:pred rv) (:detail rv))) reviews))))
  (= what "show-many") (let [subjects (filterv (fn [^String s] (not (str/blank? s))) (mapv (fn [^String s] (short-id s)) (vec (str/split arg #","))))
   subject-set (reduce (fn [m ^String s] (assoc m (str "@" s) true)) {} subjects)]
  (println (to-json (mapv (fn [c] (->JSubjectFact (short-id (triple-subject c)) (triple-predicate c) (triple-value c))) (filterv (fn [c] (get subject-set (triple-subject c) false)) facts)))))
  (= what "child-settlement") (let [children (direct-child-subjects facts arg)
   child-ids (child-agent-ids children)
   committed-runs (matching-subjects facts "kind" "run")
   runs (child-run-subjects facts child-ids committed-runs)]
  (println (to-json (->JChildSettlementProjection "north.child-settlement" 1 arg (subject-fact-projection facts children) (subject-fact-projection facts runs)))))
  (= what "children") (println (to-json (vec (sort (mapv short-id (set (keys (matching-subjects facts "part_of" (str "@" arg)))))))))
  (= what "agents") (println (to-json (mapv (fn [c] (->JAgentFact (subs (triple-subject c) (count "@agent:")) (triple-predicate c) (triple-value c))) (filterv (fn [c] (let [^String l (triple-subject c)]
  (and (some? l) (str/starts-with? l "@agent:")))) facts))))
  (= what "presentation") (println (to-json (->JPresentation (proj/condition-emoji idx "active") (proj/condition-emoji idx "unresolved") (proj/condition-emoji idx "ready") (proj/condition-emoji idx "blocked") (proj/condition-emoji idx "draft"))))
  :else (println "usage: json board|ready|blocked|done|needs-review|show <id>|show-many <id,id,...>|children <parent>|child-settlement <coordinator>|agents|presentation"))))

(defn cmd-json [^String log ^String what ^String arg ^Boolean all?]
  (if (= what "show") (cmd-json-show log arg) (cmd-json-database log what arg all?)))

(defn cmd-needs-review [^String log]
  (let [live-idx-now (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)
   reviews (canonical-grooming-reviews live-idx-now live-idx-now today before? activity)
   promo (stale/promotable live-idx-now)]
  (println (str "NEEDS REVIEW — " (count reviews) " judgment(s) whose inputs moved (" today ")"))
  (doseq [rv reviews]
  (println (str "  [" (:pred rv) "] " (short-id (:te rv)) "  " (trunc (title-of live-idx-now (:te rv)) 44)))
  (println (str "      " (:detail rv))))
  (println (str "\nPROMOTABLE — " (count promo) " uncommitted draft(s) that grew real structure"))
  (doseq [te promo]
  (println (str "  " (short-id te) "  " (trunc (title-of live-idx-now te) 52))))))

(defrecord EntryPoint [te note created])

(defn entrypoint-te [r] (:te r))

(defn entrypoint-note [r] (:note r))

(defn entrypoint-created [r] (:created r))

(defn- ^String entry-note [idx ^String te]
  (reduce (fn [^String acc ^String v] (if (and (str/blank? acc) (str/starts-with? v "SESSION ENTRY POINT")) v acc)) "" (proj/string-values-at idx te "note")))

(defn- ^EntryPoint find-entry [idx]
  (reduce (fn [^EntryPoint best ^String te] (let [^String note (entry-note idx te)]
  (if (str/blank? note) best (let [^String c (let [cc (proj/string-value-at idx te "created_at")]
  (if (some? cc) cc ""))]
  (if (or (str/blank? (:te best)) (store.rt/str-lt? (:created best) c)) (->EntryPoint te note c) best))))) (->EntryPoint "" "" "") (proj/thread-subjects idx)))

(defn cmd-boot [^String log]
  (let [idx (live-idx log)
   ^String today (store.rt/today-iso)
   before? store.rt/str-lt?
   activity (default-activity)]
  (let [^EntryPoint e (find-entry idx)]
  (if (str/blank? (:te e)) (println "\nENTRY POINT — none (no thread carries a `SESSION ENTRY POINT` note)") (do
  (println (str "\nENTRY POINT — " (short-id (:te e)) "  " (title-of idx (:te e))))
  (println (:note e))
  (let [ls (proj/string-values-at idx (:te e) "learning")]
  (if (not (empty? ls)) (do
  (println "\nSTANDING MANDATES (learning):")
  (doseq [l ls]
  (println (str "  - " l)))))))))
  (let [nonterm (filterv (fn [^String te] (not (proj/terminal-i? idx te))) (proj/work-thread-ids-i idx))]
  (println (str "\nBOARD — active " (count (in-condition idx nonterm today before? activity "active")) "  unresolved " (count (in-condition idx nonterm today before? activity "unresolved")) "  ready " (count (in-condition idx nonterm today before? activity "ready")) "  blocked " (count (in-condition idx nonterm today before? activity "blocked")) "  draft " (count (in-condition idx nonterm today before? activity "draft"))))
  (let [cands (filterv (fn [^String te] (not (proj/terminal-i? idx te))) nonterm)
   items (filterv (fn [^LevItem it] (> (:score it) 0)) (mapv (fn [^String te] (->LevItem te (proj/leverage-score idx te))) cands))
   ranked (vec (take 5 (sort-by (fn [^LevItem it] (- 0 (:score it))) items)))]
  (println "TOP LEVERAGE — finishing these transitively frees the most stuck threads")
  (doseq [it ranked]
  (println (str "  unblocks " (:score it) "  " (short-id (:te it)) "  " (title-of idx (:te it)))))))))

(defn cmd-tools []
  (do
  (println "NORTH — curated tool surface (the MCP verbs; bin/north-mcp is authoritative):")
  (println "  work queue : ready · next · threads · blocked · agenda · leverage · needs-review")
  (println "  vocabulary : schema (census by kind) · predicate (metadata + connected examples) · teaching-coverage")
  (println "  read/write : show · capture · tell · retract · validate")
  (println "  agents     : dispatch · spawn")
  (println "  view       : presentation")
  (println "")
  (println "Engine core underneath: Beagle Store = 10 tools (tell/retract/show/ask/validate + 5 graph-edit verbs).")
  (println "Vocabulary is DATA, not tools: `north show <pred>` reveals metadata and connected teaching facts")
  (println "(cardinality/value_kind/acyclic/predicate_example facts). Govern it with `north predicate`.")))

(defrecord PredCount [pred n])

(defn predcount-pred [r] (:pred r))

(defn predcount-n [r] (:n r))

(defrecord KindStat [kind subjects facts preds])

(defn kindstat-kind [r] (:kind r))

(defn kindstat-subjects [r] (:subjects r))

(defn kindstat-facts [r] (:facts r))

(defn kindstat-preds [r] (:preds r))

(def ^String KP-SEP "\u0001")

(defn- census [idx facts]
  (let [subj-list (proj/all-subjects idx)
   skind (reduce (fn [m ^String s] (assoc m s (kind-of idx s))) {} subj-list)
   ksub (reduce (fn [m ^String s] (let [^String kd (get skind s "other")]
  (assoc m kd (+ 1 (int (get m kd 0)))))) {} subj-list)
   kfacts (reduce (fn [m c] (let [^String kd (get skind (triple-subject c) "other")]
  (assoc m kd (+ 1 (int (get m kd 0)))))) {} facts)
   kpreds (reduce (fn [m c] (let [^String kd (get skind (triple-subject c) "other")
   ^String kk (str kd KP-SEP (triple-predicate c))]
  (assoc m kk (+ 1 (int (get m kk 0)))))) {} facts)
   kp-keys (vec (sort (set (keys kpreds))))
   stats (mapv (fn [^String kd] (let [^String pfx (str kd KP-SEP)
   off (+ (count kd) 1)
   plist (mapv (fn [^String kk] (->PredCount (subs kk off) (int (get kpreds kk 0)))) (filterv (fn [^String kk] (str/starts-with? kk pfx)) kp-keys))
   ptop (vec (take 8 (sort-by (fn [^PredCount pc] (- 0 (:n pc))) plist)))]
  (->KindStat kd (int (get ksub kd 0)) (int (get kfacts kd 0)) ptop))) (vec (sort (set (keys ksub)))))]
  (vec (sort-by (fn [^KindStat ks] (- 0 (:facts ks))) stats))))

(def ^String SP24 "                        ")

(defn- ^String padr [^String s n]
  (if (>= (count s) n) s (str s (subs SP24 0 (- n (count s))))))

(defn- ^String pad7 [n]
  (let [^String s (str n)]
  (if (>= (count s) 7) s (str (subs "0000000" 0 (- 7 (count s))) s))))

(defn- kind-subjects [idx ^String kind]
  (filterv (fn [^String s] (= (kind-of idx s) kind)) (proj/all-subjects idx)))

(defrecord CovAcc [seen pc])

(defn covacc-seen [r] (:seen r))

(defn covacc-pc [r] (:pc r))

(defn- coverage [facts subjset]
  (:pc (reduce (fn [^CovAcc a c] (if (get subjset (triple-subject c) false) (let [^String sk (str (triple-subject c) KP-SEP (triple-predicate c))]
  (if (get (:seen a) sk false) a (->CovAcc (assoc (:seen a) sk true) (assoc (:pc a) (triple-predicate c) (+ 1 (int (get (:pc a) (triple-predicate c) 0))))))) a)) (->CovAcc {} {}) facts)))

(defrecord FieldStat [pred subs pct required])

(defn fieldstat-pred [r] (:pred r))

(defn fieldstat-subs [r] (:subs r))

(defn fieldstat-pct [r] (:pct r))

(defn fieldstat-required [r] (:required r))

(defn- schema-fields [idx facts ^String kind]
  (let [ksubs (kind-subjects idx kind)
   total (count ksubs)
   subjset (reduce (fn [m ^String s] (assoc m s true)) {} ksubs)
   pc (coverage facts subjset)
   stats (mapv (fn [^String p] (let [n (int (get pc p 0))
   pct (if (> total 0) (quot (* 100 n) total) 0)
   req (if (> total 0) (>= (* n 100) (* total 98)) false)]
  (->FieldStat p n pct req))) (vec (sort (set (keys pc)))))]
  (vec (sort-by (fn [^FieldStat fs] (str (if (:required fs) "0" "1") "|" (pad7 (- 9999999 (:subs fs))) "|" (:pred fs))) stats))))

(defn- ^String pred-ann [idx ^String p]
  (let [^String ps (if (or (some? (proj/string-value-at idx (str "@" p) "cardinality")) (some? (proj/string-value-at idx (str "@" p) "value_kind"))) (str "@" p) p)
   card (proj/string-value-at idx ps "cardinality")
   vk (proj/string-value-at idx ps "value_kind")]
  (str (if (some? card) (str "  cardinality=" card) "") (if (some? vk) (str " value_kind=" vk) ""))))

(defn- ^String kind-writer [^String kind]
  (cond
  (= kind "thread") "north capture -> src/north/main.bclj capture-facts (title, kind=thread, created_at, committed, …)"
  (= kind "concern") "concern declare -> cli/concern-cli.clj (put! kind=concern, intent, touches, reached)"
  (= kind "agent") "agent identity -> sdk/src/identity.ts writeIdentity + bin/north-on-spawn (tell agent:<id> kind/role/display_name)"
  (= kind "run") "managed task telemetry -> sdk/src/telemetry.ts recordRun (kind=run)"
  (= kind "message") "mail + commands -> cli/msg-cli.clj (@msg: mail, @cmd: commands)"
  (= kind "north/mine") "personal notes -> cli/north-mine.clj (@mine:<stem> facts)"
  (= kind "predicate") "executable schema -> north predicate define"
  (= kind "topic") "topic grouping anchors (topic- prefix)"
  :else "(writer not curated — query canonical Store log state for this kind's writer)"))

(defn- print-schema-kind [idx facts ^String kind]
  (let [ksubs (kind-subjects idx kind)
   total (count ksubs)]
  (if (= total 0) (println (str "SCHEMA · " kind " — no subjects of this kind. `north schema` lists the kinds in use.")) (let [fields (schema-fields idx facts kind)
   req (filterv (fn [^FieldStat fs] (:required fs)) fields)
   opt (filterv (fn [^FieldStat fs] (not (:required fs))) fields)]
  (println (str "SCHEMA · " kind " — " total " subjects · " (count fields) " distinct predicates"))
  (println (str "  REQUIRED — carried by ≥98% of " kind " subjects (≈ every one):"))
  (doseq [fs req]
  (println (str "    " (padr (:pred fs) 20) " " (:pct fs) "%" (pred-ann idx (:pred fs)))))
  (if (empty? req) (do
  (println "    (none)")))
  (println "  OPTIONAL — coverage % of subjects that carry it:")
  (doseq [fs opt]
  (println (str "    " (padr (:pred fs) 20) " " (:pct fs) "%" (pred-ann idx (:pred fs)))))
  (if (empty? opt) (do
  (println "    (none)")))
  (println (str "  written by: " (kind-writer kind)))))))

(defn cmd-schema [^String log ^String kind]
  (let [facts (live-facts log)
   idx (proj/index-triples facts)]
  (if (not (str/blank? kind)) (print-schema-kind idx facts kind) (let [stats (census idx facts)
   pred-subs (filterv (fn [^String s] (or (some? (proj/string-value-at idx s "cardinality")) (or (some? (proj/string-value-at idx s "value_kind")) (some? (proj/string-value-at idx s "acyclic"))))) (proj/all-subjects idx))]
  (println (str "SCHEMA — " (count (proj/all-subjects idx)) " subjects / " (count facts) " live facts across " (count stats) " kinds"))
  (doseq [ks stats]
  (println (str "  " (padr (:kind ks) 20) " " (:subjects ks) " subjects · " (:facts ks) " facts")))
  (println (str "  " (padr "predicate-meta" 20) " " (count pred-subs) " predicate(s) carry declared cardinality/value_kind/acyclic"))
  (println "→ north schema <kind> for the field spec — required vs optional preds, coverage %, who writes it")))))

(defn cmd-teaching-coverage [^String log]
  (let [facts (live-facts log)
   idx (proj/index-triples facts)
   predicates (filterv (fn [^String s] (= (proj/string-value-at idx s "entity_kind") "predicate")) (proj/all-subjects idx))
   taught (filterv (fn [^String s] (not (empty? (proj/string-values-at idx s "predicate_example")))) predicates)
   missing (sort (map (fn [^String s] (if (str/starts-with? s "@") (subs s 1) s)) (remove (set taught) predicates)))]
  (println (str "TEACHING COVERAGE — " (count taught) " / " (count predicates) " predicate entities have connected examples"))
  (if (empty? missing) (println "  ✓ every executable predicate has a connected example") (do
  (println (str "  missing examples — " (str/join ", " missing)))
  (println "  add predicate_example graph facts; bootstrap tables are not authority")))))

(defn- ^Boolean has-flag? [args ^String f]
  (not (empty? (filterv (fn [^String a] (= a f)) args))))

(defn run [args ^String threads-dir ^String log]
  (let [^String cmd (if (empty? args) "" (first args))]
  (cond
  (= cmd "capture") (if (and (>= (count args) 2) (or (= (nth args 1) "--help") (= (nth args 1) "-h"))) (println "usage: capture <title> [owner]") (if (>= (count args) 2) (cmd-capture threads-dir log (nth args 1) (if (>= (count args) 3) (nth args 2) "personal")) (println "usage: capture <title> [owner]")))
  (= cmd "ready") (cmd-ready log (has-flag? args "--all"))
  (= cmd "blocked") (cmd-blocked log)
  (= cmd "leverage") (cmd-leverage log)
  (= cmd "next") (cmd-next log)
  (= cmd "agenda") (cmd-agenda log)
  (= cmd "threads") (cmd-board log (has-flag? args "--all"))
  (= cmd "cockpit") (cmd-cockpit log)
  (= cmd "schema") (cmd-schema log (if (>= (count args) 2) (nth args 1) ""))
  (= cmd "teaching-coverage") (cmd-teaching-coverage log)
  (= cmd "needs-review") (cmd-needs-review log)
  (= cmd "audit") (cmd-audit log)
  (= cmd "resolve") (if (>= (count args) 2) (cmd-resolve log (nth args 1)) (println "usage: resolve <@handle|@id>"))
  (= cmd "done-bars") (if (>= (count args) 2) (cmd-done-bars log (nth args 1)) (println "usage: done-bars <@id|@handle>"))
  (= cmd "validate") (do
  (cmd-validate log)
  nil)
  (= cmd "tools") (cmd-tools)
  (= cmd "boot") (cmd-boot log)
  (= cmd "json") (cmd-json log (if (> (count args) 1) (nth args 1) "") (if (> (count args) 2) (nth args 2) "") (has-flag? args "--all"))
  :else (println "north usage: capture <title> [owner] | ready [--all] | blocked | leverage | next | agenda | threads [--all] | cockpit | schema | teaching-coverage | needs-review | audit | resolve <@handle|@id> | validate | tools | boot | listen <agent-id> | json <...>   (threads/ready default to a curated top slice; --all for the full dump. engine verbs import/export/show/tell/retract/merge route to Beagle Store)"))))

(defn run-status [args ^String threads-dir ^String log]
  (cond
  (and (not (empty? args)) (= (first args) "validate")) (cmd-validate log)
  :else (do
  (run args threads-dir log)
  0)))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [argv (vec args)
   ^String threads-dir (threads-dir)
   ^String log (log-path)
   status (run-status argv threads-dir log)]
  (if (not (= status 0)) (do
  (System/exit status))))))
