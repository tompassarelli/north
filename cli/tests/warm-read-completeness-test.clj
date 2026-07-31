#!/usr/bin/env bb
;; A nonempty warm view is not evidence that every corpus domain contributed.
;; Incomplete composed state must fall back to the cold fold, while exact-subject
;; reads remain on the subject-sized coordinator path.
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[fram.kernel :as kernel]
         '[fram.rt :as rt]
         '[north.main :as main])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file *file*)) "../..")))

;; partitioned-main resolves coord.clj beside the executable named by this
;; property. Use a non-existent peer so loading defines its functions without
;; satisfying the executable entrypoint guard.
(System/setProperty "babashka.file" (str root "/cli/warm-read-test-loader.clj"))
(load-file (str root "/cli/partitioned-main.clj"))

(def failures (atom 0))
(def checks (atom 0))

(defn check! [label pass?]
  (swap! checks inc)
  (println (if pass? "PASS" "FAIL") label)
  (when-not pass? (swap! failures inc)))

(def fixture-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-warm-completeness-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def fixture-log (.getCanonicalPath (io/file fixture-dir "facts.log")))
(def legacy-subject "@2026-06-15-150040")
(def legacy-id (subs legacy-subject 1))
(def exact-subject "@019fa542-98de-73bb-a2da-9acc68adca4b")
(def exact-id (subs exact-subject 1))

(def cold-rows
  [[legacy-subject "title" "Cold fixture thread"]
   [legacy-subject "owner" "personal"]
   [legacy-subject "kind" "thread"]
   [legacy-subject "created_at" "2026-07-30T00:00:00Z"]
   [legacy-subject "committed" "2026-07-30"]])

(spit fixture-log
      (apply str
             (map-indexed
              (fn [index [subject predicate value]]
                (str (pr-str {:tx (inc index) :op "assert" :l subject
                              :p predicate :r value :by "warm-read-test"})
                     "\n"))
              cold-rows)))

(def incomplete-warm-facts
  [(kernel/->Fact "@run:warm-only" "kind" "run")])

(def incomplete-warm-state
  {:version 9
   :facts incomplete-warm-facts
   :complete false
   :domains {:coordination {:available false}
             :telemetry {:available true}}})

(def exact-rows
  [["title" "Exact UUID subject"]
   ["kind" "thread"]])

(def fixture-output
  (with-redefs [main/compose-telemetry-log? (constantly false)
                rt/coord-live-facts (fn [& _] incomplete-warm-facts)
                rt/coord-live-state (fn [& _] incomplete-warm-state)
                rt/coord-show-for-log
                (fn [_ _ subject]
                  (when (= exact-subject subject)
                    {:version 9 :rows exact-rows}))]
    (with-out-str
      (println "BOARD")
      (main/cmd-board fixture-log true)
      (println "LEGACY")
      (main/cmd-resolve fixture-log legacy-subject)
      (println "EXACT UUID")
      (main/cmd-json fixture-log "show" exact-id false))))

(println fixture-output)

(check! "incomplete warm state falls back to the cold board"
        (str/includes? fixture-output "THREADS — 1 open"))
(check! "legacy date-format id resolves from the cold fold"
        (str/includes? fixture-output (str "LEGACY\n" legacy-subject "\n")))
(check! "exact UUID show stays on the subject-sized path"
        (and (str/includes? fixture-output "EXACT UUID")
             (str/includes? fixture-output "Exact UUID subject")))

(def page-cursor "fram-query-page-v1.YQ")

(def complete-page-calls (atom []))
(def complete-view
  (with-redefs [north.coord/telemetry-partition-enabled? (constantly false)
                north.coord/query-page-in-domain
                (fn [_ domain _query _limit after & _]
                  (swap! complete-page-calls conj [domain after])
                  (if (nil? after)
                    {:ok (subvec cold-rows 0 2)
                     :more true :next page-cursor :version 12 :engine "scan"}
                    {:ok (subvec cold-rows 2)
                     :more false :next nil :version 12 :engine "scan"}))]
    (north.coord/live-facts-view 7977)))

(def complete-cold-reads (atom 0))
(def complete-output
  (with-redefs [main/compose-telemetry-log? (constantly false)
                rt/coord-live-state
                (fn [& _]
                  {:facts (mapv (fn [[subject predicate value]]
                                  (kernel/->Fact subject predicate value))
                                (:facts complete-view))
                   :complete (:complete complete-view)})
                rt/read-log
                (fn [& _]
                  (swap! complete-cold-reads inc)
                  (throw (ex-info "complete warm path attempted a cold fold" {})))]
    (with-out-str (main/cmd-board fixture-log true))))

(check! "all warm pages produce the correct board without a cold fold"
        (and (true? (:complete complete-view))
             (= cold-rows (:facts complete-view))
             (= [[:coordination nil] [:coordination page-cursor]]
                @complete-page-calls)
             (zero? @complete-cold-reads)
             (str/includes? complete-output "THREADS — 1 open")
             (str/includes? complete-output "Cold fixture thread")))

(def aborted-page-calls (atom []))
(def aborted-view
  (with-redefs [north.coord/telemetry-partition-enabled? (constantly false)
                north.coord/query-page-in-domain
                (fn [_ domain _query _limit after & _]
                  (swap! aborted-page-calls conj [domain after])
                  (if (nil? after)
                    {:ok (subvec cold-rows 0 2)
                     :more true :next page-cursor :version 12 :engine "scan"}
                    (throw (ex-info "injected pagination abort" {}))))]
    (north.coord/live-facts-view 7977)))

(def original-read-log rt/read-log)
(def aborted-cold-reads (atom 0))
(def aborted-output
  (with-redefs [main/compose-telemetry-log? (constantly false)
                rt/coord-live-state
                (fn [& _]
                  {:facts (mapv (fn [[subject predicate value]]
                                  (kernel/->Fact subject predicate value))
                                (:facts aborted-view))
                   :complete (:complete aborted-view)})
                rt/read-log
                (fn [log]
                  (swap! aborted-cold-reads inc)
                  (original-read-log log))]
    (with-out-str (main/cmd-board fixture-log true))))

(check! "aborted warm pagination is incomplete and falls back to the correct cold board"
        (and (false? (:complete aborted-view))
             (empty? (:facts aborted-view))
             (= ["coordination"] (:unavailable aborted-view))
             (= [[:coordination nil] [:coordination page-cursor]]
                @aborted-page-calls)
             (pos? @aborted-cold-reads)
             (str/includes? aborted-output "THREADS — 1 open")
             (str/includes? aborted-output "Cold fixture thread")))

(def composed-state
  (with-redefs [north.coord/telemetry-partition-enabled? (constantly true)
                north.coord/live-facts-view
                (fn [_]
                  {:facts [["@run:warm-only" "kind" "run"]]
                   :domains {:coordination {:available false}
                             :telemetry {:available true}}
                   :unavailable ["coordination"]
                   :unavailable-detail
                   [["coordination" "injected incomplete domain"]]
                   :complete false})]
    (composed-live-facts 7977 fixture-log)))

(check! "split adapter carries incomplete corpus-domain state"
        (and (map? composed-state)
             (false? (:complete composed-state))
             (= incomplete-warm-facts (:facts composed-state))))

(println (format "warm-read-completeness: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
