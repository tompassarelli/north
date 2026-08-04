#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root
  (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def watcher (str root "/cli/coordinated-nix-rebuild-worker-host.clj"))

(System/setProperty "north.coordinated-nix-rebuild-worker-host.lib" "1")
(System/setProperty "babashka.file" watcher)
(load-file watcher)

(alias 'owner 'north.coordinated-nix-rebuild)
(alias 'request 'north.rebuild-request)
(alias 'watch 'north.coordinated-nix-rebuild-worker-host)

(def checks (atom []))
(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(defn queue-payload [ids last-fired-ms offset]
  (cheshire.core/generate-string
   (sorted-map
    "version" request/queue-version
    "requests" (mapv (fn [id]
                        (sorted-map "id" id
                                    "version" north.rebuild-request-state/protocol-version
                                    "requester" "test-agent"
                                    "why" "wake fixture"
                                    "createdAtMs" (parse-long (first (clojure.string/split id #"-")))
                                    "urgent" false))
                      ids)
    "lastFiredMs" last-fired-ms
    "legacy" (sorted-map "path" "/fixture/coordination.log"
                         "fileKey" "fixture"
                         "offset" offset))))

(defn queue-event [version raw]
  {:event :commit :version version :op "assert"
   :l request/queue-subject :p request/queue-predicate :r raw})

(defn reset-watch-state! []
  (when-let [reset-fn (ns-resolve 'north.coordinated-nix-rebuild-worker-host
                                  'reset-event-state!)]
    (reset-fn)))

(defn with-zero-debounce [f]
  (if-let [debounce-var (ns-resolve 'north.coordinated-nix-rebuild-worker-host
                                    'event-debounce-ms)]
    (with-redefs-fn {debounce-var 0} f)
    (f)))

(check "subscription is scoped to the exact queue singleton"
       (= {:op :subscribe :filter {:watch #{"@rebuild-queue"}}}
          (watch/subscription-request))
       (watch/subscription-request))

(check "an activating fixed unit already owns the claim boundary"
       (= {:state :active}
          (owner/classify-window-unit-state
           {:exit 3 :out "activating\n" :err ""}))
       nil)

(let [debounce-var (ns-resolve 'north.coordinated-nix-rebuild-worker-host
                               'event-debounce-ms)
      bounded-debounce (ns-resolve 'north.coordinated-nix-rebuild-worker-host
                                   'bounded-event-debounce-ms)]
  (check "queue event debounce defaults to one second"
         (= 1000 (bounded-debounce))
         (bounded-debounce))
  (check "queue event debounce has a hard two-second ceiling"
         (= 2000 (with-redefs-fn {debounce-var 5000} bounded-debounce))
         (with-redefs-fn {debounce-var 5000} bounded-debounce)))

(let [wakes (atom [])]
  (reset-watch-state!)
  (with-redefs [watch/wake-owner!
                (fn [reason]
                  (swap! wakes conj reason)
                  {:action "idle"
                   :queue-observation
                   {:version 1
                    :semantic {:request-ids [] :last-fired-ms nil}}})]
    (watch/connected-catch-up!))
  (check "every reconnect performs one unconditional catch-up"
         (= [:connected] @wakes)
         @wakes))

(let [a "2000000000000-abcdef12"
      b "2000000000001-acde1234"
      first-raw (queue-payload [a b] nil 10)
      cursor-only-raw (queue-payload [b a] nil 99)
      current-raw (atom first-raw)
      current-version (atom 1)
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-debounce
    #(with-redefs [north.coord/show-envelope
                   (fn [& _] {:version @current-version
                              :rows [[request/queue-predicate @current-raw]]})
                   watch/wake-owner! (fn [reason]
                                       (swap! wakes conj reason)
                                       {:action "fired"})]
       (watch/process-event! (queue-event 1 first-raw))
       (reset! current-raw cursor-only-raw)
       (reset! current-version 2)
       (watch/process-event! (queue-event 2 cursor-only-raw))))
  (check "legacy cursor-only singleton churn does not produce a second wake"
         (= [:queue-commit] @wakes)
         @wakes))

(let [id "2000000000001-acde1234"
      empty-raw (queue-payload [] nil 10)
      open-raw (queue-payload [id] nil 20)
      current-raw (atom empty-raw)
      current-version (atom 3)
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-debounce
    #(with-redefs [north.coord/show-envelope
                   (fn [& _] {:version @current-version
                              :rows [[request/queue-predicate @current-raw]]})
                   watch/wake-owner! (fn [reason]
                                       (swap! wakes conj reason)
                                       {:action "fired"})]
       (watch/process-event! (queue-event 3 empty-raw))
       (reset! current-raw open-raw)
       (reset! current-version 4)
       (watch/process-event! (queue-event 4 open-raw))))
  (check "empty to nonempty queue transition produces exactly one wake"
         (= [:queue-commit] @wakes)
         @wakes))

(let [a "2000000000010-aaaa0010"
      b "2000000000011-bbbb0011"
      c "2000000000012-cccc0012"
      one (queue-payload [a] nil 10)
      two (queue-payload [a b] nil 20)
      three (queue-payload [a b c] nil 30)
      collect-calls (atom 0)
      state-reads (atom 0)
      current-raw (atom three)
      results (atom [])]
  (reset-watch-state!)
  (with-zero-debounce
    #(with-redefs [north.coord/show-envelope
                   (fn [& _] {:version 7
                              :rows [[request/queue-predicate @current-raw]]})
                   owner/collect!
                   (fn [& _]
                     (let [n (swap! collect-calls inc)]
                       (if (= 1 n)
                         {:action "active" :count 3}
                         {:action "fired" :count 3 :window "follow-up"})))
                   owner/window-unit-state
                   (fn []
                     {:state (if (= 1 (swap! state-reads inc))
                               :active
                               :inactive)})
                   watch/active-wait-ms 0
                   watch/active-wait-timeout-ms 1000]
       (doseq [[version raw] [[5 one] [6 two] [7 three]]]
         (swap! results conj (watch/process-event! (queue-event version raw))))))
  (check "three admissions during one active rebuild coalesce into one immediate follow-up"
         (and (= 2 @collect-calls)
              (= 2 @state-reads)
              (= 1 (count (filter #(= "fired" (:action %)) @results))))
         {:collect-calls @collect-calls :state-reads @state-reads
          :results @results}))

(let [old "2000000000020-dddd0020"
      retained "2000000000021-eeee0021"
      before (queue-payload [old retained] nil 40)
      settled (queue-payload [retained] 2000000000030 50)
      current-raw (atom before)
      current-version (atom 8)
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-debounce
    #(with-redefs [north.coord/show-envelope
                   (fn [& _] {:version @current-version
                              :rows [[request/queue-predicate @current-raw]]})
                   watch/wake-owner! (fn [reason]
                                       (swap! wakes conj reason)
                                       {:action "fired"})]
       (watch/process-event! (queue-event 8 before))
       (reset! current-raw settled)
       (reset! current-version 9)
       (watch/process-event! (queue-event 9 settled))))
  (check "settlement that retains newly admitted ids wakes their follow-up"
         (= [:queue-commit :queue-commit] @wakes)
         @wakes))

(let [raw (queue-payload ["2000000000040-ffff0040"] nil 60)
      current-raw (atom raw)
      calls (atom 0)]
  (reset-watch-state!)
  (with-zero-debounce
    #(with-redefs [north.coord/show-envelope
                   (fn [& _] {:version 10
                              :rows [[request/queue-predicate @current-raw]]})
                   owner/collect! (fn [& _]
                                    (swap! calls inc)
                                    {:action "deferred" :count 1
                                     :reason "launch failed"})]
       (watch/process-event! (queue-event 10 raw))))
  (check "failed launch without a new semantic event is not retried inline"
         (= 1 @calls)
         @calls))

(let [claims (atom [])
      launches (atom [])
      unit-active? (atom false)
      plan {:action :fire
            :count 1
            :open [{:id "2000000000000-abcdef12" :urgent true}]
            :queue-read {:mode "steady" :caught-up true :corpus-queries 0}}
      event-raw (queue-payload ["2000000000000-abcdef12"] nil 70)
      event {:event :commit :version 9 :op "assert"
             :l "@rebuild-queue" :p "rebuild_queue" :r event-raw}
      started (System/nanoTime)
      first-wake-result (atom nil)
      duplicate-result (atom nil)
      reset-state (reset-watch-state!)
      run
      (with-zero-debounce
          #(with-out-str
             (with-redefs
               [north.coord/show-envelope
                (fn [& _] {:version 9
                           :rows [[request/queue-predicate event-raw]]})
                owner/with-owner-lock (fn [f] (f))
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
                       (owner/collect-unlocked! 7977 false "/fixture/north")))))
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
            [north.coord/show-envelope
             (fn [& _] {:version 0 :rows []})
             owner/collect!
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
      raw (queue-payload ["2000000000050-abcd0050"] nil 80)
      failed-wake-result (atom nil)
      reset-state (reset-watch-state!)
      run
      (with-zero-debounce
          #(with-out-str
             (with-redefs
               [north.coord/show-envelope
                (fn [& _] {:version 10
                           :rows [[request/queue-predicate raw]]})
                owner/with-owner-lock (fn [f] (f))
                request/plan-window
                (fn [_] (throw (ex-info "fact server unavailable" {})))
                request/open-window! (fn [& _] (reset! opened? true))]
               (reset!
                failed-wake-result
                (watch/process-event!
                 {:event :commit :version 10 :op "assert"
                  :l "@rebuild-queue" :p "rebuild_queue" :r raw})))))]
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
  (println (format "\ncoordinated Nix rebuild worker: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
