#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(load-file (str root "/cli/rebuild_intent_state.clj"))

(def observed-broadcasts (atom []))
(def state
  (atom
   (north.rebuild-intent-state/new-intent
    {:id "simulated-intent"
     :who "rebuild-session"
     :why "integration probe"
     :planned-window "after hold"
     :created-at-ms 0
     :hold-seconds 180
     :max-delay-seconds 600})))

(swap! observed-broadcasts conj {:type :intent :to :live-roster})

;; Two independent simulated live sessions respond to the same durable intent.
(swap! state north.rebuild-intent-state/apply-response
       {:event-id "session-one-event" :type :batch :from "session-one"
        :what "pending display change" :received-at-ms 30000})
(swap! state north.rebuild-intent-state/apply-response
       {:event-id "session-two-event" :type :hold :from "session-two"
        :reason "finish audio change" :eta-seconds 300
        :received-at-ms 60000})

(let [before (north.rebuild-intent-state/advance @state 359999)
      cleared (north.rebuild-intent-state/advance @state 360000)
      responses (:responses cleared)]
  (assert (= :holding (:phase before)))
  (assert (= :all-clear (:phase cleared)))
  (assert (= #{"session-one" "session-two"} (set (map :from responses))))
  (assert (= "pending display change"
             (:what (first (filter #(= :batch (:type %)) responses)))))
  (assert (= 360000 (:deadline-ms cleared)))
  (swap! observed-broadcasts conj {:type :all-clear :to :live-roster})
  (assert (= [:intent :all-clear] (mapv :type @observed-broadcasts)))
  (println "PASS two simulated sessions: one batch joined, one hold extended, silence produced all-clear"))

