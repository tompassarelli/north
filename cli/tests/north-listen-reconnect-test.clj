#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file test-script)) "../..")))
(def store
  (.getCanonicalPath
   (io/file (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
                (System/getenv "BEAGLE_STORE_HOME")
                "/home/tom/code/beagle/main/store"))))

(when-not (= "1" (System/getenv "NORTH_LISTEN_LIB"))
  (let [result @(proc/process
                 ["env" "NORTH_LISTEN_LIB=1" "bb" "-cp"
                  (str store "/out") test-script]
                 {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))

(let [source (str root "/cli/north-listen.clj")]
  (System/setProperty "babashka.file" source)
  (try
    (load-file source)
    (finally
      (System/setProperty "babashka.file" test-script))))

(def checks (atom []))
(defn check [label value]
  (swap! checks conj [label (boolean value)])
  (println (if value (str "PASS " label) (str "FAIL " label))))

(let [coordinate (store.types/occurrence-coordinate
                  (store.types/transaction-coordinate "listener-test" 7) 0)
      lease (store.types/triple "@holder:test" :kernel/expires-at 1234)
      proposition (store.types/triple "@resource:test" :kernel/lease lease)
      occurrence-event! (ns-resolve 'north.coord 'occurrence-event!)
      malformed-error
      (try
        (occurrence-event!
         [coordinate store.types/assert-action "not-a-triple"])
        nil
        (catch clojure.lang.ExceptionInfo error error))]
  (check "typed lease occurrence decodes without changing its proposition terms"
         (= {:operation :assert
             :subject "@resource:test"
             :predicate :kernel/lease
             :value lease
             :version 7}
            (occurrence-event!
             [coordinate store.types/assert-action proposition])))
  (check "malformed occurrence outer rows still reject"
         (= :malformed-occurrence-window
            (:type (ex-data malformed-error)))))

(let [events (atom [])]
  (with-redefs [fenced-listener-state!
                (fn [_ state] (swap! events conj state))]
    (arm-listener-generation! {:generation :test}))
  (check "arming publishes only the canonical armed route state"
         (= ["armed"] @events)))

(let [events (atom [])
      generation {:generation :test
                  :stop-renewal? (atom false)
                  :renewal-error (atom nil)}]
  (with-redefs
   [acquire-listener-generation!
    (fn [_ _ _] (swap! events conj :acquire) generation)
    start-listener-renewer! (fn [_] (future nil))
    finish-listener-generation! (fn [_] (swap! events conj :finish))]
    (with-native-listener-generation!
     0 "@agent:test" "test" "session"
     (fn [_] (swap! events conj :scope))))
  (check "native listener fences before scope projection and always finalizes"
         (= [:acquire :scope :finish] @events)))

(let [events (atom [])
      version-calls (atom 0)
      result
      (with-redefs
       [validate-listener-corpus!
        (fn [_] (swap! events conj :status) {:state :ready})
        listener-kind-projection
        (fn [_ _] (swap! events conj :kind) "session")
        with-native-listener-generation!
        (fn [_ _ _ _ body]
          (swap! events conj :lease)
          (body {:generation :test}))
        north.coord/cur-ver!
        (fn [_]
          (if (= 1 (swap! version-calls inc))
            (do (swap! events conj :baseline) 5)
            (do (swap! events conj :head) (stop-listener!))))
        listener-node-projection
        (fn [_ _]
          (swap! events conj :scope)
          {:kind "session" :holds ["@role:engine"] :watches []})
        arm-listener-generation!
        (fn [_] (swap! events conj :arm))
        replay-listener-mail!
        (fn [& _] (swap! events conj :replay))
        ensure-listener-generation-current!
        (fn [_] (swap! events conj :fence-check))]
        (run-listener-pass!
         0 "test" "@agent:test" false false false (atom #{}) (atom #{})))]
  (check "listener freezes a poll baseline before scope and arms before polling"
         (and (= :stop (:reason result))
              (= [:status :kind :lease :baseline :scope :arm :replay
                  :fence-check :head]
                 @events))))

(let [calls (atom 0)
      projection
      (with-redefs
       [north.coord/show-rows!
        (fn [_ _]
          (swap! calls inc)
          [["kind" "session"]
           ["holds" "@role:engine"]
           ["watches" "@thread:one"]])]
        (listener-node-projection 0 "@agent:test"))]
  (check "listener identity scope uses one exact subject projection"
         (and (= 1 @calls)
              (= {:kind "session"
                  :holds ["@role:engine"]
                  :watches ["@thread:one"]}
                 projection))))

(let [held-error
      (try
        (require-listener-lease-grant!
         "rival" {:reject :held :holder "owner" :exp 1234})
        nil
        (catch clojure.lang.ExceptionInfo error error))
      calls (atom 0)
      sleeps (atom [])
      result
      (run-with-reconnect!
       (fn []
         (swap! calls inc)
         (listener-pass-failure held-error))
       #(swap! sleeps conj %)
       (fn [_ _]))]
  (check "a held listener generation exits without scope-query retries"
         (and (= :listener-generation-held (:type (ex-data held-error)))
              (= :superseded (:reason result))
              (= 1 @calls)
              (empty? @sleeps))))

(let [status {:state :starting}
      error
      (try
        (with-redefs [north.coord/status! (fn [_] status)]
          (validate-listener-corpus! 1))
        nil
        (catch clojure.lang.ExceptionInfo caught caught))]
  (check "listener refuses a SpaceId that is not ready"
         (and (= :listener-space-unavailable (:type (ex-data error)))
              (= status (:status (ex-data error))))))

(let [passes (atom [{:reason :unavailable :message "connection refused"}
                    {:reason :unavailable :message "restart EOF"}
                    {:reason :stop :message "re-armed"}])
      sleeps (atom [])
      notices (atom [])
      result
      (run-with-reconnect!
       #(let [value (first @passes)]
          (swap! passes subvec 1)
          value)
       #(swap! sleeps conj %)
       #(swap! notices conj [%1 %2]))]
  (check "restart sequence reaches the re-armed pass instead of exiting"
         (= {:reason :stop :message "re-armed"} result))
  (check "restart failures back off exponentially"
         (= [250 500] @sleeps))
  (check "each interruption is surfaced before retry"
         (= ["connection refused" "restart EOF"]
            (mapv (comp :message first) @notices))))

(let [mismatch
      (ex-info "configured coordination SpaceId does not match"
               {:type :rpc/space-mismatch})
      sleeps (atom [])
      notices (atom [])
      caught
      (try
        (run-with-reconnect!
         #(listener-pass-failure mismatch)
         #(swap! sleeps conj %)
         #(swap! notices conj [%1 %2]))
        nil
        (catch clojure.lang.ExceptionInfo error error))]
  (check "a SpaceId mismatch exits with its original refusal"
         (identical? mismatch caught))
  (check "a SpaceId mismatch is never slept or retried"
         (and (empty? @sleeps) (empty? @notices))))

(let [mismatch
      (ex-info "Store RPC magic does not match"
               {:type :rpc-invalid-magic})
      sleeps (atom [])
      notices (atom [])
      caught
      (try
        (run-with-reconnect!
         #(listener-pass-failure mismatch)
         #(swap! sleeps conj %)
         #(swap! notices conj [%1 %2]))
        nil
        (catch clojure.lang.ExceptionInfo error error))]
  (check "a Store protocol mismatch exits with its original refusal"
         (identical? mismatch caught))
  (check "a Store protocol mismatch is never slept or retried"
         (and (empty? @sleeps) (empty? @notices))))

(let [failure (listener-pass-failure (ex-info "connection closed" {}))]
  (check "ordinary connection loss remains transient"
         (= {:reason :unavailable :message "connection closed"} failure)))

(let [result
      (proc/shell
       {:continue true :out :string :err :string}
       "timeout" "--signal=TERM" "--kill-after=0.1s" "0.3s"
       "env" "-u" "NORTH_LISTEN_LIB"
       "NORTH_LISTEN_INITIAL_BACKOFF_MS=10"
       "NORTH_LISTEN_MAX_BACKOFF_MS=20"
       "bb" "-cp" (str store "/out")
       (str root "/cli/north-listen.clj") "59999" "restart-probe")
      diagnostics (str (:out result) "\n" (:err result))]
  (check "connection refusal is transient and listener remains running"
         (= 124 (:exit result)))
  (check "connection refusal retries loudly with bounded backoff"
         (and (str/includes? diagnostics "reconnecting in 10ms")
              (str/includes? diagnostics "reconnecting in 20ms"))))

(let [failed (remove second @checks)]
  (println (str "north listen reconnect: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
