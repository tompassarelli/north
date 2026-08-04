#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root
  (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def watcher (str root "/cli/nix-rebuild-worker.clj"))

(System/setProperty "north.nix-rebuild-worker.lib" "1")
(System/setProperty "babashka.file" watcher)
(load-file watcher)

(alias 'worker 'north.nix-rebuild-worker)
(alias 'request 'north.rebuild-request)
(alias 'watch 'north.nix-rebuild-worker)

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
  (when-let [reset-fn (ns-resolve 'north.nix-rebuild-worker
                                  'reset-event-state!)]
    (reset-fn)))

(defn with-zero-debounce [f]
  (if-let [debounce-var (ns-resolve 'north.nix-rebuild-worker
                                    'event-debounce-ms)]
    (with-redefs-fn {debounce-var 0} f)
    (f)))

(check "subscription is scoped to the exact queue singleton"
       (= {:op :subscribe :filter {:watch #{"@rebuild-queue"}}}
          (watch/subscription-request))
       (watch/subscription-request))

(let [debounce-var (ns-resolve 'north.nix-rebuild-worker
                               'event-debounce-ms)
      bounded-debounce (ns-resolve 'north.nix-rebuild-worker
                                   'bounded-event-debounce-ms)]
  (check "queue event debounce defaults to one second"
         (= 1000 (bounded-debounce))
         (bounded-debounce))
  (check "queue event debounce has a hard two-second ceiling"
         (= 2000 (with-redefs-fn {debounce-var 5000} bounded-debounce))
         (with-redefs-fn {debounce-var 5000} bounded-debounce)))

(let [wakes (atom [])]
  (reset-watch-state!)
  (with-redefs [watch/run-worker!
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
                   watch/run-worker! (fn [reason]
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
                   watch/run-worker! (fn [reason]
                                       (swap! wakes conj reason)
                                       {:action "fired"})]
       (watch/process-event! (queue-event 3 empty-raw))
       (reset! current-raw open-raw)
       (reset! current-version 4)
       (watch/process-event! (queue-event 4 open-raw))))
  (check "empty to nonempty queue transition produces exactly one wake"
         (= [:queue-commit] @wakes)
         @wakes))

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
                   watch/run-worker! (fn [reason]
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
                   worker/process-queue! (fn [& _]
                                           (swap! calls inc)
                                           {:action "failed" :count 1})]
       (watch/process-event! (queue-event 10 raw))))
  (check "a failed rebuild without a new semantic event is not retried inline"
         (= 1 @calls)
         @calls))

(let [claims (atom [])
      runs (atom [])
      plan {:action :fire
            :count 1
            :open [{:id "2000000000000-abcdef12" :urgent true}]
            :queue-read {:mode "steady" :caught-up true :corpus-queries 0}}
      started (System/nanoTime)
      result (atom nil)
      _ (with-out-str
          (with-redefs
            [request/plan-window (fn [_] plan)
             request/open-window!
             (fn [_ ids]
               (swap! claims conj ids)
               "2000000000001-acde1234")
             request/run-window!
             (fn [port window-id]
               (swap! runs conj [port window-id])
               0)]
            (reset! result (worker/process-queue! 7977 false))))
      elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))]
  (check "one worker claims and performs the rebuild synchronously"
         (and (= "fired" (:action @result))
              (< elapsed-ms 250)
              (= [[(:id (first (:open plan)))]] @claims)
              (= [[7977 "2000000000001-acde1234"]] @runs))
         {:elapsed-ms elapsed-ms :claims @claims :runs @runs :result @result}))

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

(check "unrelated fact-server commits do not wake the Nix rebuild worker"
       (nil?
        (with-redefs [watch/run-worker!
                      (fn [& _] (throw (ex-info "unexpected wake" {})))]
          (watch/process-event!
           {:event :commit :l "@other" :p "rebuild_queue" :r "{}"})))
       nil)

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nNix rebuild worker: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
