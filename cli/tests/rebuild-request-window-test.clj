#!/usr/bin/env bb
;; Fixture time only — a window test that slept would be a flake generator.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (-> (or (System/getProperty "babashka.file") *file*)
              io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(load-file (str root "/cli/rebuild_request_state.clj"))
(System/setProperty "north.rebuild-request-cli.lib" "1")
(System/setProperty "babashka.file" (str root "/cli/rebuild-request-cli.clj"))
(load-file (str root "/cli/rebuild-request-cli.clj"))

(alias 'q 'north.rebuild-request-state)
(alias 'rq 'north.rebuild-request)

(def checks (atom []))
(defn check [label ok]
  (swap! checks conj [label ok])
  (println (if ok (str "PASS " label) (str "FAIL " label))))

(def hour (* 60 60 1000))
(def now 1000000000000)

(defn request [{:keys [id requester why at urgent satisfied]}]
  (cond-> {:id id :requester requester :why why :created-at-ms at :urgent (boolean urgent)}
    satisfied (assoc :satisfied {:generation "/nix/store/gen" :at-ms at})))

(def two-asks
  [(request {:id "a" :requester "agent-a" :why "hook wiring" :at (- now (* 20 60 1000))})
   (request {:id "b" :requester "agent-b" :why "guard change" :at (- now (* 5 60 1000))
             :urgent true})])

(defn plan [overrides]
  (q/window-plan (merge {:now-ms now
                         :last-window-ms nil
                         :window-ms hour
                         :requests two-asks
                         :coordination-on? true}
                        overrides)))

;; ---- collection ------------------------------------------------------------
(check "an empty queue is idle regardless of legacy clock telemetry"
       (= :idle (:action (plan {:requests []}))))

(check "satisfied requests are not re-collected"
       (let [p (plan {:requests [(request {:id "a" :requester "agent-a" :why "done"
                                           :at (- now hour) :satisfied true})]})]
         (and (= :idle (:action p)) (zero? (:count p)))))

(check "a first-ever window fires immediately"
       (= :fire (:action (plan {:last-window-ms nil}))))

(check "the fired window carries every open ask in arrival order"
       (let [p (plan {})]
         (and (= 2 (:count p))
              (= ["agent-a" "agent-b"] (:requesters p))
              (str/includes? (:why p) "queued rebuild requests (2)")
              (< (str/index-of (:why p) "hook wiring")
                 (str/index-of (:why p) "guard change"))
              (str/includes? (:why p) "[urgent]"))))

;; ---- immediate admission ---------------------------------------------------
(check "a plain request fires even when the previous window fired 1ms ago"
       (= :fire
          (:action
           (plan {:last-window-ms (dec now)
                  :requests [(first two-asks)]}))))

(check "urgent provenance does not change queue eligibility"
       (let [plain (request {:id "plain" :requester "agent-a"
                             :why "runtime stabilization" :at now})
             urgent (request {:id "urgent" :requester "agent-a"
                              :why "runtime stabilization" :at now
                              :urgent true})]
         (= [:fire :fire]
            (mapv (fn [queued]
                    (:action (plan {:last-window-ms (dec now)
                                    :requests [queued]})))
                  [plain urgent]))))

;; ---- the flip --------------------------------------------------------------
(check "coordination off queues and reports, never fires"
       (let [p (plan {:coordination-on? false})]
         (and (= :queued (:action p))
              (= :rebuild-coordination-off (:reason p))
              (= 2 (:count p)))))

(check "a parked queue never consumes a window it did not fire"
       ;; No last window + coordination off must still be :queued, so the flip
       ;; finds the queue intact rather than pre-burned.
       (= :queued (:action (plan {:coordination-on? false :last-window-ms nil}))))

(check "the legacy last-fired timestamp remains readable telemetry"
       (with-redefs
         [north.rebuild-request/queue-snapshot!
          (fn [_] {:snapshot {:last-fired-ms (- now 4000)}})]
         (= (- now 4000) (rq/last-fired-window-ms 7977))))

(check "a queue with no historical firing reports no timestamp"
       (with-redefs
         [north.rebuild-request/queue-snapshot!
          (fn [_] {:snapshot {:last-fired-ms nil}})]
         (nil? (rq/last-fired-window-ms 7977))))

;; ---- execution -------------------------------------------------------------
(def window-id "1000000000000-abcdef12")
(def request-id "1000000000001-abcdef34")
(def window-record {:id window-id :requests [request-id]})
(def decoded-request
  {:id request-id :requester "agent-a" :why "land queued work"
   :created-at-ms now :urgent false})

(let [calls (atom [])
      record
      (with-redefs
        [north.coord/show-rows
         (fn [port subject]
           (swap! calls conj [port subject])
           [["kind" "rebuild-window"]
            ["window_action" "launching"]
            ["window_request" request-id]])
         north.coord/indexed-query
         (fn [& _] (throw (ex-info "global query used" {})))]
        (rq/load-window-record 7977 window-id))]
  (check "the window owner loads one exact subject without a global query"
         (and (= [[7977 (str "@rebuild-window:" window-id)]] @calls)
              (= window-record (select-keys record [:id :requests]))
              (= "launching" (:action record)))))

(let [calls (atom [])
      subject (str "@rebuild-request:" request-id)
      decoded
      (with-redefs
        [north.coord/show-rows
         (fn [port actual-subject]
           (swap! calls conj [port actual-subject])
           [["rebuild_request"
             (str "{\"version\":1,\"requester\":\"agent-a\","
                  "\"why\":\"land queued work\",\"createdAtMs\":" now ","
                  "\"urgent\":false}")]
            ["rebuild_request_satisfied"
             "{\"intent\":\"intent-a\",\"generation\":\"/nix/store/gen\",\"atMs\":7}"]])
         north.coord/indexed-query
         (fn [& _] (throw (ex-info "global query used" {})))]
        (rq/decode-request 7977 subject))]
  (check "an exact request show supplies both request and settlement"
         (and (= [[7977 subject]] @calls)
              (= request-id (:id decoded))
              (= "land queued work" (:why decoded))
              (= {:intent "intent-a" :generation "/nix/store/gen" :at-ms 7}
                 (:satisfied decoded)))))

(let [attempts (atom 0)
      record
      (with-redefs
        [north.coord/show-rows
         (fn [_ _]
           (if (= 1 (swap! attempts inc))
             (throw (ex-info "timed out" {:type :coordinator-response-timeout}))
             [["window_action" "launching"]
              ["window_request" request-id]]))]
        (rq/load-window-record 7977 window-id))]
  (check "a typed transient exact-read failure retries and succeeds"
         (and (= 2 @attempts)
              (= window-record (select-keys record [:id :requests])))))

(let [attempts (atom 0)
      error
      (with-redefs
        [north.coord/show-rows
         (fn [_ _]
           (swap! attempts inc)
           (throw (ex-info "malformed" {:type :malformed-show-response})))]
        (try
          (rq/load-window-record 7977 window-id)
          nil
          (catch clojure.lang.ExceptionInfo error error)))]
  (check "a nonretryable exact-read failure fails immediately"
         (and (= 1 @attempts)
              (= :malformed-show-response (:type (ex-data error))))))

(let [shell-calls (atom [])
      settled (atom nil)
      satisfied (atom [])
      writes (atom [])
      action (atom nil)
      rc
      (with-redefs
        [rq/load-window-record (fn [_ _] window-record)
         rq/decode-request (fn [_ _] decoded-request)
         babashka.process/shell
         (fn [_ & args]
           (swap! shell-calls conj (vec args))
           {:exit 0 :out "" :err ""})
         rq/current-generation (fn [] "/nix/store/test-generation")
         north.rebuild-request/settle-window-queue!
         (fn [_ id request-ids] (reset! settled [id request-ids]))
         north.rebuild-request/write-satisfaction-projection!
         (fn [_ id outcome] (swap! satisfied conj [id outcome]))
         north.coord/put! (fn [& args] (swap! writes conj args))
         north.rebuild-request/write-window-action-projection!
         (fn [_ id value] (reset! action [id value]))]
        (rq/run-window! 7977 window-id))]
  (check "the window owner uses automatic mode without a second human intent ceremony"
         (and (zero? rc)
              (= "--automatic" (nth (first @shell-calls) 1))
              (= "--why" (nth (first @shell-calls) 2))
              (= [window-id [request-id]] @settled)
              (= [[request-id {:intent nil :generation "/nix/store/test-generation"}]]
                 @satisfied)
              (= [window-id "fired"] @action)
              (= "window_generation" (nth (first @writes) 2))))
  (check "a fired window schedules the canary outside the singleton rebuild unit"
         (let [canary-call (second @shell-calls)]
           (and (= 2 (count @shell-calls))
                (= "systemd-run" (first canary-call))
                (some #{(str "--unit=north-rebuild-canary-" window-id)} canary-call)
                (some #{"--property=Type=exec"} canary-call)
                (= [(str root "/bin/north") "rebuild" "run-canary"
                    window-id "/nix/store/test-generation"]
                   (subvec canary-call (- (count canary-call) 5)))
                (= 1 (count @writes))))))

(let [shell-calls (atom [])
      writes (atom [])]
  (with-redefs
    [babashka.process/shell
     (fn [_ & args]
       (swap! shell-calls conj (vec args))
       {:exit 0 :out "canary green\n" :err ""})
     north.coord/put! (fn [& args] (swap! writes conj args))]
    (rq/run-post-window-canary! 7977 window-id "/nix/store/test-generation"))
  (check "the detached canary records the landed generation result"
         (and (= [[(str root "/bin/north") "canary" "run" "--matrix"]]
                 @shell-calls)
              (= "window_canary" (nth (first @writes) 2))
              (str/includes? (nth (first @writes) 3) "\"status\":\"full-green\""))))

(let [writes (atom [])
      alerts (atom [])]
  (with-redefs
    [babashka.process/shell
     (fn [_ & _] {:exit 1 :out "" :err "systemd unavailable"})
     north.coord/put! (fn [& args] (swap! writes conj args))
     rq/send-urgent-alert! (fn [message] (swap! alerts conj message))]
    (check "canary admission failure is recorded without reopening a fired window"
           (false? (rq/schedule-post-window-canary!
                    7977 window-id "/nix/store/test-generation"))))
  (check "canary admission failure alerts with durable window provenance"
         (and (= "window_canary" (nth (first @writes) 2))
              (str/includes? (nth (first @writes) 3)
                             "\"status\":\"schedule-failure\"")
              (str/includes? (first @alerts) window-id))))

(let [satisfied (atom [])
      writes (atom [])
      action (atom nil)
      rc
      (with-redefs
        [rq/load-window-record (fn [_ _] window-record)
         rq/decode-request (fn [_ _] decoded-request)
         babashka.process/shell
         (fn [_ & _] {:exit 7 :out "" :err "failed\n"})
         rq/current-generation
         (fn [] (throw (ex-info "failed child reached generation read" {})))
         rq/mark-satisfied! (fn [& args] (swap! satisfied conj args))
         north.coord/put! (fn [& args] (swap! writes conj args))
         rq/set-window-action! (fn [_ id value] (reset! action [id value]))]
        (rq/run-window! 7977 window-id))]
  (check "a failed automatic child leaves requests open and the window retryable"
         (and (= 7 rc)
              (empty? @satisfied)
              (empty? @writes)
              (= [window-id "failed"] @action))))

;; ---- composition bound -----------------------------------------------------
(let [many (mapv #(request {:id (str %) :requester (str "agent-" %)
                            :why (str "reason " %) :at (+ now %)})
                 (range 12))
      why (q/compose-why many)]
  (check "composed reasons are bounded and report the remainder"
         (and (str/includes? why "queued rebuild requests (12)")
              (str/includes? why "reason 0")
              (str/includes? why "reason 7")
              (not (str/includes? why "reason 8"))
              (str/includes? why "+4 more"))))

;; ---- the request contract --------------------------------------------------
(defn throws? [f]
  (try (f) false (catch clojure.lang.ExceptionInfo _ true)))

(check "a request without --why is refused"
       (throws? #(q/new-request {:id "x" :requester "a" :why "  " :created-at-ms now})))
(check "an oversize --why is refused"
       (throws? #(q/new-request {:id "x" :requester "a"
                                 :why (apply str (repeat 513 "x")) :created-at-ms now})))
(check "a request without a requester is refused"
       (throws? #(q/new-request {:id "x" :requester nil :why "w" :created-at-ms now})))
(check "--urgent marks the request and keeps its reason"
       (let [r (q/new-request {:id "x" :requester "a" :why "w" :created-at-ms now
                               :urgent-reason "coordinator wedged"})]
         (and (true? (:urgent r)) (= "coordinator wedged" (:urgent-reason r)))))
(check "a plain request is not urgent"
       (false? (:urgent (q/new-request {:id "x" :requester "a" :why "w"
                                        :created-at-ms now}))))
(check "an optional thread is carried, a blank one is refused"
       (and (= "t1" (:thread (q/new-request {:id "x" :requester "a" :why "w"
                                             :created-at-ms now :thread "t1"})))
            (throws? #(q/new-request {:id "x" :requester "a" :why "w"
                                      :created-at-ms now :thread " "}))))

;; ---- gauges ----------------------------------------------------------------
(check "the gauge counts only rebuilds inside the trailing window"
       (= 2 (:count (q/rebuild-gauge [(- now (* 10 60 1000))
                                      (- now (* 50 60 1000))
                                      (- now (* 3 hour))]
                                     now hour))))
(check "the default reporting horizon is one hour"
       (= 3600 q/default-window-seconds))

(let [result @(proc/process ["env" (str "NORTH_HOME=" root)
                             "bb" (str root "/cli/config-cli.clj")
                             "rebuild-window"]
                            {:out :string :err :string})
      output (str (:out result) (:err result))]
  (check "rebuild-window config describes observation, never hourly admission"
         (and (zero? (:exit result))
              (str/includes? output "reporting horizon only; admission is immediate")
              (not (str/includes? output "rate cap"))
              (not (str/includes? output "at most one coordinated rebuild per window")))))

(check "the urgent rate covers only the reporting period"
       (let [r (q/urgent-rate [(request {:id "a" :requester "a" :why "w"
                                         :at (- now hour) :urgent true})
                               (request {:id "b" :requester "b" :why "w"
                                         :at (- now (* 2 hour))})
                               (request {:id "c" :requester "c" :why "w"
                                         :at (- now (* 40 hour)) :urgent true})]
                              now (* 24 hour))]
         (and (= 2 (:total r)) (= 1 (:urgent r)) (= 0.5 (:rate r)))))

(check "ages never run negative on clock skew"
       (zero? (q/age-ms (request {:id "a" :requester "a" :why "w" :at (+ now 5000)}) now)))
(check "ages humanize into one compact token"
       (= ["30s" "5m" "3h" "2d"]
          (mapv q/humanize-age [30000 (* 5 60 1000) (* 3 hour) (* 48 hour)])))

(let [failed (remove second @checks)]
  (println (str "rebuild request window: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
