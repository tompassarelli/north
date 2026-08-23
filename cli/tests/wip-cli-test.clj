#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (.getParentFile (.getParentFile (.getParentFile (io/file *file*))))))
(def wip-cli (str root "/cli/wip-cli.clj"))

(System/setProperty "north.wip.lib" "1")
(create-ns 'north.coord)
(intern 'north.coord 'PORT "7977")
(intern 'north.coord 'query-page-row-limit 4096)
(intern 'north.coord 'telemetry-partition-enabled?
        (fn [] (throw (ex-info "unstubbed telemetry query" {}))))
(intern 'north.coord 'query-page-in-domain
        (fn [& _] (throw (ex-info "unstubbed Store query" {}))))
(System/setProperty "babashka.file" wip-cli)
(load-file wip-cli)

(def failures (atom []))
(defn check [label pass?]
  (if pass?
    (println "PASS" label)
    (do
      (println "FAIL" label)
      (swap! failures conj label))))

(def now-ms
  (.toEpochMilli
   (.toInstant
    (.atStartOfDay
     (java.time.LocalDate/of 2026 7 28)
     java.time.ZoneOffset/UTC))))

(def fixture-rows
  [["@ready-a" "title" "highest leverage"]
   ["@ready-a" "committed" "2026-07-28"]
   ["@ready-a" "created_at" "2026-07-28"]
   ["@ready-b" "title" "second pull"]
   ["@ready-b" "committed" "2026-07-28"]
   ["@ready-b" "created_at" "2026-07-28"]
   ["@uncertain" "title" "assigned without live control"]
   ["@uncertain" "committed" "2026-07-28"]
   ["@uncertain" "driver" "@missing-control"]
   ["@uncertain" "updated_at" "2026-07-28"]
   ["@dependent" "depends_on" "@ready-a"]
   ["@concern-ready" "title" "not work"]
   ["@concern-ready" "kind" "concern"]
   ["@concern-ready" "committed" "2026-07-28"]
   ["@floor-thread" "title" "coordinator"]
   ["@floor-thread" "wip_floor" "3"]
   ["lane-a" north.wip-cli/lane-kind-tag "lane"]
   ["lane-a" north.wip-cli/lane-thread-tag "@lane-thread-a"]
   ["@lane-thread-a" "title" "lane A work"]
   ["lane-b" north.wip-cli/lane-kind-tag "lane"]
   ["lane-b" north.wip-cli/lane-thread-tag "@lane-thread-b"]
   ["@lane-thread-b" "title" "lane B work"]
   ["coordinator" north.wip-cli/coordinator-thread-tag "@floor-thread"]])

(def controls ["coordinator" "lane-a" "lane-b" "native-session"])
(def fact-options
  {:check true :coordinator "coordinator" :coordinator-thread nil})

(let [{:keys [exit output report]}
      (north.wip-cli/execute fixture-rows controls fact-options now-ms)]
  (check "floor fact is read from the invoking coordinator thread"
         (and (= 3 (:floor report))
              (= 2 (:live report))
              (= 2 (:ready-depth report))))
  (check "--check exits 3 only for a pullable shortfall"
         (and (= 3 exit)
              (str/ends-with?
               output
               "WIP 2/3 — SHORTFALL: pull @ready-a @ready-b")))
  (check "unresolved assignment is rendered but never pullable"
         (and (= 1 (:unresolved-depth report))
              (= [{:thread "@uncertain"
                   :title "assigned without live control"
                   :driver "@missing-control"}]
                 (:unresolved report))
              (str/includes? output "UNRESOLVED ASSIGNMENTS — 1")
              (not (some #{"@uncertain"} (map :thread (:top report))))))
  (check "live managed lanes carry their reservation thread bindings"
         (= [{:control "lane-a" :thread "@lane-thread-a"
              :title "lane A work" :binding-conflict false}
             {:control "lane-b" :thread "@lane-thread-b"
              :title "lane B work" :binding-conflict false}]
            (:lanes report)))
  (check "pulls reuse leverage ranking"
         (= [{:thread "@ready-a" :title "highest leverage" :leverage 1}
             {:thread "@ready-b" :title "second pull" :leverage 0}]
            (:top report))))

(let [{:keys [exit output report]}
      (north.wip-cli/execute
       fixture-rows controls (assoc fact-options :floor 2) now-ms)]
  (check "--floor overrides the coordinator fact"
         (and (= 2 (:floor report))
              (= 0 exit)
              (str/ends-with? output "WIP 2/2 — OK"))))

(let [{:keys [exit report]}
      (north.wip-cli/execute
       fixture-rows controls (assoc fact-options :check false) now-ms)]
  (check "report mode stays exit 0 during a shortfall"
         (and (:shortfall report) (= 0 exit))))

(let [no-ready (remove #(contains? #{"@ready-a" "@ready-b" "@dependent"}
                                    (first %))
                       fixture-rows)
      {:keys [exit output report]}
      (north.wip-cli/execute
       (vec no-ready) controls fact-options now-ms)]
  (check "--check stays exit 0 when the ready queue is empty"
         (and (= 0 (:ready-depth report))
              (= 0 exit)
              (str/ends-with? output "WIP 2/3 — OK"))))

(let [without-fact
      (vec (remove #(= "wip_floor" (second %)) fixture-rows))
      {:keys [report]}
      (north.wip-cli/execute without-fact controls fact-options now-ms)]
  (check "missing floor fact uses default 4"
         (= north.wip-cli/default-floor (:floor report))))

(check "option parser accepts --check and --floor in either order"
       (and (= {:check true :floor 5}
               (north.wip-cli/parse-options ["--check" "--floor" "5"]))
            (= {:check true :floor 5}
               (north.wip-cli/parse-options ["--floor" "5" "--check"]))))

(let [idx (north.wip-cli/rows->index
           [["@absent" "title" "absent"]
            ["@live" "title" "live"]
            ["@live" "driver" "@lane-a"]
            ["@unresolved" "title" "unresolved"]
            ["@unresolved" "driver" "@offline"]
            ["@conflict" "title" "conflict"]
            ["@conflict" "driver" "@lane-a"]
            ["@conflict" "driver" "@offline"]])
      activity (north.wip-cli/driver-activity-predicate #{"lane-a"})]
  (check "hosted WIP derives the same three activity judgments"
         (and (= north.projections/absent-proven (activity idx "@absent"))
              (= north.projections/live-proven (activity idx "@live"))
              (= north.projections/unresolved (activity idx "@unresolved"))
              (= north.projections/unresolved (activity idx "@conflict")))))

(check "malformed, mismatched, or duplicate session leases prove no live control"
       (empty?
        (north.wip-cli/live-controls-from-rows
         [["session:broken" :kernel/lease "bad"]
          [(store.types/triple "nested" "resource" "id") :kernel/lease
           (store.types/triple "nested" :kernel/expires-at 2000000000000)]
          ["session:wrong" :kernel/lease
           (store.types/triple "somebody-else" :kernel/expires-at 2000000000000)]
          ["session:duplicate" :kernel/lease
           (store.types/triple "duplicate" :kernel/expires-at 2000000000000)]
          ["session:duplicate" :kernel/lease
           (store.types/triple "duplicate" :kernel/expires-at 3000000000000)]
          ["session:repeated" :kernel/lease
           (store.types/triple "repeated" :kernel/expires-at 2000000000000)]
          ["session:repeated" :kernel/lease
           (store.types/triple "repeated" :kernel/expires-at 2000000000000)]]
         now-ms)))

(let [coordination-rows
      [["@work" "title" "new \"quoted\" title"]
       ["@work" "committed" "2026-07-28"]
       ["@agent:lane-a" "kind" "lane"]
       ["@agent:coordinator" "current_thread" "@floor-thread"]
       ["session:coordinator" :kernel/lease
        (store.types/triple "coordinator" :kernel/expires-at 2000000000000)]
       ["session:lane-a" :kernel/lease
        (store.types/triple "lane-a" :kernel/expires-at 2000000000000)]]
      telemetry-rows
      [["@run:one" "run_reservation_agent" "@agent:lane-a"]
       ["@run:one" "run_reservation_thread" "@work"]]
      calls (atom [])
      state
      (with-redefs
       [north.coord/telemetry-partition-enabled? (constantly true)
        north.coord/query-page-in-domain
        (fn [_port domain query limit after at-version]
          (swap! calls conj [domain query limit after at-version])
          {:rows (if (= :coordination domain) coordination-rows telemetry-rows)
           :done? true :cursor nil :served-version 12})]
        (north.wip-cli/coordination-state))]
  (check "STORE RPC projection returns materialized work state"
         (some #{["@work" "title" "new \"quoted\" title"]}
               (:work state)))
  (check "STORE RPC projection carries presence and reservation bindings"
         (and (= #{"lane-a"} (:managed state))
              (= "@floor-thread"
                 (get (:session-threads state) "coordinator"))
              (= {"lane-a" #{"@work"}}
                 (north.wip-cli/reservation-bindings
                  (:reservations state)))))
  (check "WIP reads coordination and telemetry through bounded query pages"
         (and (= #{:coordination :telemetry} (set (map first @calls)))
              (every? #(= north.wip-cli/selected-page-limit (nth % 2)) @calls)
              (every? nil? (mapcat #(subvec (vec %) 3) @calls))
              (some #{:kernel/lease}
                    (mapcat (fn [[_ query]]
                              (map #(get-in % [:head :args 1]) (:rules query)))
                            @calls))))
  (let [presence
        (with-redefs
         [north.wip-cli/coordination-state (constantly state)]
          (north.wip-cli/presence-state 1000000000000))]
    (check "WIP derives live controls from the same coherent fact projection"
           (and (= ["coordinator" "lane-a"] (:controls presence))
                (= #{"lane-a"} (:managed presence))))))

(if (seq @failures)
  (do
    (binding [*out* *err*]
      (println (str "\nwip CLI fixture: " (count @failures) " failure(s)")))
    (System/exit 1))
  (println "\nwip CLI fixture: PASS"))
