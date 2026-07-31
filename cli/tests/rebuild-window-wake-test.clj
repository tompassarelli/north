#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root
  (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def watcher (str root "/cli/rebuild-window-watch.clj"))

(System/setProperty "north.rebuild-window-watch.lib" "1")
(System/setProperty "babashka.file" watcher)
(load-file watcher)

(alias 'owner 'north.rebuild-window-owner)
(alias 'request 'north.rebuild-request)
(alias 'watch 'north.rebuild-window-watch)

(def checks (atom []))
(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(check "subscription is scoped to the exact queue singleton"
       (= {:op :subscribe :filter {:watch #{"@rebuild-queue"}}}
          (watch/subscription-request))
       (watch/subscription-request))

(check "an activating fixed unit already owns the claim boundary"
       (= {:state :active}
          (owner/classify-window-unit-state
           {:exit 3 :out "activating\n" :err ""}))
       nil)

(let [claims (atom [])
      launches (atom [])
      unit-active? (atom false)
      plan {:action :fire
            :count 1
            :open [{:id "2000000000000-abcdef12" :urgent true}]
            :queue-read {:mode "steady" :caught-up true :corpus-queries 0}}
      event {:event :commit :version 9 :op "assert"
             :l "@rebuild-queue" :p "rebuild_queue" :r "{}"}
      started (System/nanoTime)
      first-wake-result (atom nil)
      duplicate-result (atom nil)
      _ (with-out-str
          (with-redefs
            [owner/with-owner-lock (fn [f] (f))
             request/plan-window (fn [_] plan)
             owner/window-unit-state
             (fn [] {:state (if @unit-active? :active :inactive)})
             request/open-window!
             (fn [_ ids]
               (swap! claims conj ids)
               "2000000000001-acde1234")
             owner/launch-window!
             (fn [_ window-id]
               (swap! launches conj window-id)
               (reset! unit-active? true)
               {:launched true :unit owner/rebuild-window-unit})]
            (reset! first-wake-result (watch/process-event! event))
            (reset! duplicate-result
                    (owner/collect-unlocked! 7977 false "/fixture/north"))))
      elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))]
  (check "urgent queue publication reaches one claim promptly"
         (and (= "fired" (:action @first-wake-result))
              (< elapsed-ms 250)
              (= [[(:id (first (:open plan)))]] @claims)
              (= ["2000000000001-acde1234"] @launches))
         {:elapsed-ms elapsed-ms :claims @claims :launches @launches
          :result @first-wake-result})
  (check "a duplicate wake observes the active fixed unit and makes no second claim"
         (and (= "active" (:action @duplicate-result))
              (= 1 (count @claims))
              (= 1 (count @launches)))
         {:result @duplicate-result :claims @claims :launches @launches}))

(let [collect-calls (atom 0)
      state-reads (atom 0)
      wake-result (atom nil)
      _ (with-out-str
          (with-redefs
            [owner/collect!
             (fn [& _]
               (if (= 1 (swap! collect-calls inc))
                 {:action "active" :count 1}
                 {:action "fired" :count 1 :window "rearmed-window"}))
             owner/window-unit-state
             (fn []
               {:state (if (= 1 (swap! state-reads inc))
                         :active
                         :inactive)})
             watch/active-wait-ms 0
             watch/active-wait-timeout-ms 1000]
            (reset! wake-result (watch/wake-owner! :queue-commit))))]
  (check "an active-window wake re-arms after unit completion without a new commit"
         (and (= "fired" (:action @wake-result))
              (= 2 @collect-calls)
              (= 2 @state-reads))
         {:result @wake-result
          :collect-calls @collect-calls
          :state-reads @state-reads}))

(let [durable (atom [])
      shell-called? (atom false)
      id
      (with-redefs
        [request/mint-id (constantly "2000000000002-deadbeef")
         request/ensure-schema! (fn [_] nil)
         request/assert-batch! (fn [_ subject _] (swap! durable conj subject))
         request/enqueue-request! (fn [_ queued] (swap! durable conj (:id queued)))
         babashka.process/shell
         (fn [& _]
           (reset! shell-called? true)
           (throw (ex-info "requester invoked a process" {})))]
        (request/record-request!
         7977 {:requester "test-agent"
               :why "wake separation"
               :urgent-reason "latency probe"}))]
  (check "requester publishes durable state without invoking rebuild code"
         (and (= "2000000000002-deadbeef" id)
              (= ["@rebuild-request:2000000000002-deadbeef"
                  "2000000000002-deadbeef"]
                 @durable)
              (false? @shell-called?))
         {:id id :durable @durable :shell-called @shell-called?}))

(let [opened? (atom false)
      failed-wake-result (atom nil)
      _ (with-out-str
          (with-redefs
            [owner/with-owner-lock (fn [f] (f))
             request/plan-window
             (fn [_] (throw (ex-info "fact server unavailable" {})))
             request/open-window! (fn [& _] (reset! opened? true))]
            (reset!
             failed-wake-result
             (watch/process-event!
              {:event :commit :version 10 :op "assert"
               :l "@rebuild-queue" :p "rebuild_queue" :r "{}"}))))]
  (check "wake failure claims nothing, leaving the durable request for fallback"
         (and (= "error" (:action @failed-wake-result))
              (false? @opened?))
         {:result @failed-wake-result :opened @opened?}))

(check "unrelated fact-server commits do not wake the owner"
       (nil?
        (with-redefs [watch/wake-owner!
                      (fn [& _] (throw (ex-info "unexpected wake" {})))]
          (watch/process-event!
           {:event :commit :l "@other" :p "rebuild_queue" :r "{}"})))
       nil)

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nrebuild window wake: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
