#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (.getParentFile (.getParentFile (.getParentFile (io/file *file*))))))
(def wip-cli (str root "/cli/wip-cli.clj"))

(System/setProperty "north.wip.lib" "1")
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
   ["@dependent" "depends_on" "@ready-a"]
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

(if (seq @failures)
  (do
    (binding [*out* *err*]
      (println (str "\nwip CLI fixture: " (count @failures) " failure(s)")))
    (System/exit 1))
  (println "\nwip CLI fixture: PASS"))
