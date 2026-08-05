#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

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

(defn queue-payload [ids last-fired-ms]
  (cheshire.core/generate-string
   (sorted-map
    "version" request/queue-version
    "requests" (mapv (fn [id]
                        (sorted-map
                         "id" id
                         "version" north.rebuild-request-state/protocol-version
                         "requester" "test-agent"
                         "why" "wake fixture"
                         "createdAtMs"
                         (parse-long (first (clojure.string/split id #"-")))
                         "urgent" false))
                      ids)
    "lastFiredMs" last-fired-ms)))

(defn reset-watch-state! []
  (watch/reset-observed-state!))

(defn with-zero-coalesce [operation]
  (with-redefs [watch/coalesce-ms 0]
    (operation)))

(let [raw (queue-payload ["2000000000000-abcdef12"] nil)
      observed
      (with-redefs [watch/with-client (fn [operation] (operation :client))
                    watch/queue-scan!
                    (fn [client]
                      (when-not (= :client client)
                        (throw (ex-info "wrong client" {:client client})))
                      {:served-version 12 :rows [raw]})
                    watch/triple-value identity]
        (watch/current-queue-observation))]
  (check "queue observation is one exact FRAMRPC subject scan"
         (= {:version 12
             :semantic {:request-ids ["2000000000000-abcdef12"]
                        :last-fired-ms nil}}
            observed)
         observed))

(let [error
      (try
        (with-redefs [watch/with-client (fn [operation] (operation :client))
                      watch/queue-scan!
                      (fn [_] {:served-version 12 :rows ["a" "b"]})
                      watch/triple-value identity]
          (watch/current-queue-observation))
        nil
        (catch Throwable caught caught))]
  (check "queue observation rejects duplicate singleton values"
         (= :malformed-rebuild-queue-projection (:type (ex-data error)))
         (some-> error ex-data)))

(let [coalesce-var (ns-resolve 'north.nix-rebuild-worker 'coalesce-ms)
      bounded-coalesce (ns-resolve 'north.nix-rebuild-worker
                                   'bounded-coalesce-ms)]
  (check "queue coalescing defaults to one second"
         (= 1000 (bounded-coalesce))
         (bounded-coalesce))
  (check "queue coalescing has a hard two-second ceiling"
         (= 2000 (with-redefs-fn {coalesce-var 5000} bounded-coalesce))
         (with-redefs-fn {coalesce-var 5000} bounded-coalesce)))

(let [wakes (atom [])]
  (reset-watch-state!)
  (with-redefs [watch/run-worker!
                (fn [reason]
                  (swap! wakes conj reason)
                  {:action "idle"
                   :queue-observation
                   {:version 1
                    :semantic {:request-ids [] :last-fired-ms nil}}})]
    (watch/initial-catch-up!))
  (check "every connection performs one unconditional catch-up"
         (= [:connected] @wakes)
         @wakes))

(let [incoming
      {:version 20
       :semantic {:request-ids ["2000000000020-acde0020"]
                  :last-fired-ms nil}}
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-coalesce
    #(with-redefs [watch/run-worker!
                   (fn [reason]
                     (swap! wakes conj reason)
                     {:action "idle" :queue-observation incoming})]
       (watch/process-observation! incoming :queue-poll)
       (watch/process-observation! (assoc incoming :version 21) :queue-poll)))
  (check "polling wakes once for one semantic projection change"
         (= [:queue-poll] @wakes)
         @wakes))

(let [id "2000000000001-acde1234"
      empty-observation
      {:version 3 :semantic {:request-ids [] :last-fired-ms nil}}
      open-observation
      {:version 4 :semantic {:request-ids [id] :last-fired-ms nil}}
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-coalesce
    #(with-redefs [watch/run-worker!
                   (fn [reason]
                     (swap! wakes conj reason)
                     {:action "fired" :queue-observation open-observation})]
       (watch/process-observation! empty-observation :queue-poll)
       (watch/process-observation! open-observation :queue-poll)))
  (check "empty to nonempty queue transition produces exactly one wake"
         (= [:queue-poll] @wakes)
         @wakes))

(let [old "2000000000020-dddd0020"
      retained "2000000000021-eeee0021"
      before
      {:version 8
       :semantic {:request-ids [old retained] :last-fired-ms nil}}
      settled
      {:version 9
       :semantic {:request-ids [retained] :last-fired-ms 2000000000030}}
      wakes (atom [])]
  (reset-watch-state!)
  (with-zero-coalesce
    #(with-redefs [watch/run-worker!
                   (fn [reason]
                     (swap! wakes conj reason)
                     {:action "fired"})]
       (watch/process-observation! before :queue-poll)
       (watch/process-observation! settled :queue-poll)))
  (check "settlement retaining a new id wakes its follow-up"
         (= [:queue-poll :queue-poll] @wakes)
         @wakes))

(let [incoming
      {:version 10
       :semantic {:request-ids ["2000000000040-ffff0040"]
                  :last-fired-ms nil}}
      calls (atom 0)]
  (reset-watch-state!)
  (with-zero-coalesce
    #(with-redefs [watch/run-worker!
                   (fn [_]
                     (swap! calls inc)
                     {:action "failed" :count 1})]
       (watch/process-observation! incoming :queue-poll)
       (watch/process-observation! incoming :queue-poll)))
  (check "a failed rebuild without a new projection is not retried inline"
         (= 1 @calls)
         @calls))

(let [claims (atom [])
      runs (atom [])
      plan {:action :fire
            :count 1
            :open [{:id "2000000000000-abcdef12" :urgent true}]
            :queue-read {:mode "framrpc-projection"
                         :served-version 12
                         :pages 1
                         :caught-up true
                         :corpus-queries 0}}
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

(let [source (slurp watcher)]
  (check "rebuild worker names the canonical Fram connection contract"
         (and (str/includes? source "FRAM_SERVER_CONNECT")
              (str/includes? source "FRAM_SERVER_PORT")
              (str/includes? source "FRAM_OUT"))
         nil))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nNix rebuild worker: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
