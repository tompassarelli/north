#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))

(when-not (= "1" (System/getenv "NORTH_LISTEN_LIB"))
  (let [result @(proc/process
                 ["env" "NORTH_LISTEN_LIB=1" "bb" test-script]
                 {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file test-script)) "../..")))
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

(let [events (atom [])
      refused?
      (try
        (with-redefs
         [north.coord/validate-subscription!
          (fn [_]
            (swap! events conj :validate)
            (throw (ex-info "subscription refused" {:type :refused})))
          arm-listener-generation!
          (fn [_] (swap! events conj :arm))]
          (with-validated-native-listener-generation!
           nil {:reject :refused}
           #(swap! events conj :body)))
        false
        (catch clojure.lang.ExceptionInfo _ true))]
  (check "a rejected subscription cannot publish an armed generation"
         (and refused? (= [:validate] @events))))

(let [events (atom [])]
  (with-redefs
   [north.coord/validate-subscription!
    (fn [_] (swap! events conj :validate))
    arm-listener-generation!
    (fn [_] (swap! events conj :arm))]
    (with-validated-native-listener-generation!
     {:generation :test} {:ok :subscribed}
     #(swap! events conj :body)))
  (check "listener generation is armed only after a validated handshake"
         (= [:validate :arm :body] @events)))

(let [events (atom [])
      generation {:generation :test
                  :stop-renewal? (atom false)
                  :renewal-error (atom nil)}]
  (with-redefs
   [acquire-listener-generation!
    (fn [_ _ _] (swap! events conj :acquire) generation)
    start-listener-renewer! (fn [_] (future nil))
    finish-listener-generation! (fn [_] (swap! events conj :finish))
    north.coord/validate-subscription!
    (fn [_] (swap! events conj :validate))
    arm-listener-generation!
    (fn [_] (swap! events conj :arm))]
    (with-native-listener-generation!
     0 "@agent:test" "test" "session"
     (fn [owned]
       (swap! events conj :scope)
       (with-validated-native-listener-generation!
        owned {:ok :subscribed}
        #(swap! events conj :body)))))
  (check "native listener fences before scope projection and arms after handshake"
         (= [:acquire :scope :validate :arm :body :finish] @events)))

(let [calls (atom 0)
      projection
      (with-redefs
       [north.coord/show-rows
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

(let [mismatch
      {:reject ["wrong log"]
       :code :log-mismatch
       :expected-log "/tmp/expected.log"
       :served-log "/tmp/served.log"}
      error
      (try
        (with-redefs [north.coord/send-op (fn [_ _] mismatch)]
          (validate-listener-corpus! 1))
        nil
        (catch clojure.lang.ExceptionInfo caught caught))]
  (check "listener corpus preflight preserves a typed fenced refusal"
         (and
          (= :invalid-subscription-handshake (:type (ex-data error)))
          (= mismatch (:reply (ex-data error)))
          (str/includes?
           (.getMessage error) "refused the fenced subscription"))))

(let [passes (atom [{:reason :unavailable :message "connection refused"}
                    {:reason :closed :message "restart EOF"}
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
      (ex-info
       "coordinator refused the fenced subscription"
       {:type :invalid-subscription-handshake
        :reply {:reject ["wrong log"] :code :log-mismatch}})
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
  (check "a fenced corpus mismatch exits with its original refusal"
         (identical? mismatch caught))
  (check "a fenced corpus mismatch is never slept or retried"
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
       "bb" (str root "/cli/north-listen.clj") "59999" "restart-probe")
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
