#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(load-file (str root "/cli/rebuild_intent_state.clj"))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))
(defn rejected? [f expected-type]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (= expected-type (:type (ex-data error))))))

(def base
  (north.rebuild-intent-state/new-intent
   {:id "intent-1"
    :who "session-a"
    :why "apply committed configuration"
    :planned-window "after coordination hold"
    :created-at-ms 1000000
    :hold-seconds 180
    :max-delay-seconds 900}))

(check! "intent opens with the default three-minute hold deadline"
        (and (= :holding (:phase base))
             (= 1180000 (:deadline-ms base))))

(def batched
  (north.rebuild-intent-state/apply-response
   base {:event-id "batch-1" :type :batch :from "session-b"
         :what "keyboard module" :received-at-ms 1010000}))
(check! "batch-with-me joins the rebuild without extending its deadline"
        (and (= 1 (count (:responses batched)))
             (= 1180000 (:deadline-ms batched))
             (= "keyboard module" (:what (first (:responses batched))))))

(def held
  (north.rebuild-intent-state/apply-response
   batched {:event-id "hold-1" :type :hold :from "session-c"
            :reason "finishing audio module" :eta-seconds 300
            :received-at-ms 1020000}))
(check! "hold extends the deadline to the sender ETA"
        (= 1320000 (:deadline-ms held)))
(check! "state remains held before the extended deadline"
        (= :holding (:phase
                     (north.rebuild-intent-state/advance held 1319999))))
(check! "silence becomes all-clear at the effective deadline"
        (= :all-clear (:phase
                       (north.rebuild-intent-state/advance held 1320000))))

(def capped
  (north.rebuild-intent-state/apply-response
   base {:event-id "hold-2" :type :hold :from "session-d"
         :reason "large build" :eta-seconds 3600
         :received-at-ms 1010000}))
(check! "a hold is capped by the bounded maximum delay"
        (= (:max-deadline-ms capped) (:deadline-ms capped)))

(def all-clear (north.rebuild-intent-state/advance base 1180000))
(check! "responses after all-clear are refused"
        (rejected?
         #(north.rebuild-intent-state/apply-response
           all-clear {:event-id "late" :type :batch :from "session-e"
                      :what "too late" :received-at-ms 1180001})
         :response-window-closed))
(check! "rebuild cannot start before all-clear"
        (rejected?
         #(north.rebuild-intent-state/mark-rebuild-started base 1100000)
         :invalid-transition))

(def rebuilding
  (north.rebuild-intent-state/mark-rebuild-started all-clear 1180001))
(def verified
  (north.rebuild-intent-state/mark-deployment-verified
   rebuilding 1190000 "firn rebuild rc 0; deployment path observed"))
(check! "deployment verification is terminal and carries its report"
        (and (= :deployment-verified (:phase verified))
             (= "firn rebuild rc 0; deployment path observed"
                (:deployment-report verified))))

(check! "duration flags accept seconds, minutes, and hours"
        (= [15 180 3600]
           (mapv north.rebuild-intent-state/parse-duration-seconds
                 ["15s" "3m" "1h"])))

(doseq [[label ok?] @checks]
  (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
(let [failed (remove second @checks)]
  (println (format "%d/%d passed"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (when (seq failed) (System/exit 1)))

