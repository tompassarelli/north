#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[clojure.java.io :as io]
         '[coord-daemon-wire :as wire]
         '[fram.types :as t])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH") "/home/tom/code/fram/main"))))

(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw (ex-info "Fram head checkout is required; set FRAM_PATH"
                  {:fram fram})))

(load-file (str root "/cli/framrpc-client.clj"))
(require '[north.framrpc-client :as rpc])
(load-file (str fram "/coord.clj"))
(require '[coord :as coord])

(def checks (atom []))
(defn check! [label value]
  (let [ok (boolean value)]
    (println (str (if ok "  [PASS] " "  [FAIL] ") label))
    (swap! checks conj [label ok])))

(defn thrown-data [f]
  (try
    (f)
    nil
    (catch clojure.lang.ExceptionInfo error (ex-data error))
    (catch Throwable error {:type (class error) :message (.getMessage error)})))

(defn eventually [f]
  (loop [attempt 0]
    (let [value (try (f) (catch Throwable _ nil))]
      (cond
        value value
        (>= attempt 400) nil
        :else (do (Thread/sleep 25) (recur (inc attempt)))))))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn query-plan [predicate]
  (let [subject (wire/rpc-query-variable! "subject")
        value (wire/rpc-query-variable! "value")]
    (wire/rpc-query-plan!
     (wire/rpc-query-find-relation! "client-page")
     [(wire/rpc-query-stratum!
       [(wire/rpc-query-rule!
         (wire/rpc-query-head! "client-page" [subject value])
         [(wire/rpc-query-relation!
           "triple"
           [subject (wire/rpc-query-constant! predicate) value]
           false)])])])))

(defn malformed-oversize-frame []
  (let [response (wire/rpc-response! "boundary" :rpc/version 0 nil nil
                                     wire/rpc-unit)
        frame (wire/encode-rpc-frame-v1!
               (wire/rpc-response-frame 1 response))
        header (byte-array wire/rpc-v1-header-bytes)
        body-length (inc rpc/max-body-bytes)]
    (System/arraycopy frame 0 header 0 wire/rpc-v1-header-bytes)
    (let [buffer (doto (java.nio.ByteBuffer/wrap header)
                   (.order java.nio.ByteOrder/LITTLE_ENDIAN))]
      (.position buffer 14)
      (.putInt buffer body-length))
    header))

(def private-root (io/file root "docs/private"))
(fs/create-dirs private-root)
(def scratch
  (.toFile
   (java.nio.file.Files/createTempDirectory
    (.toPath private-root) "framrpc-client-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def log-path (.getCanonicalPath (io/file scratch "stage1.framlog")))
(def daemon-output (io/file scratch "daemon.log"))
(def space-id "north-framrpc-client-stage1")
(def port (free-port))
(def client (atom nil))
(def daemon (atom nil))

(try
  (coord/create-triple-log! log-path space-id)
  (let [builder
        (doto (ProcessBuilder.
               ^java.util.List
               [(str fram "/bin/fram-daemon") "serve" (str port)
                log-path space-id])
          (.directory (io/file fram))
          (.redirectErrorStream true)
          (.redirectOutput daemon-output))
        environment (.environment builder)]
    (.put environment "FRAM_DAEMON_QUIET" "1")
    (.put environment "FRAM_DAEMON_XMX" "1g")
    (.put environment "CLJ_CACHE" (str (io/file scratch "clj-cache")))
    (reset! daemon (.start builder)))

  (reset! client
          (eventually
           #(rpc/connect "127.0.0.1" port space-id
                         {:connect-timeout-ms 100
                          :read-timeout-ms 5000
                          :max-attempts 1
                          :retry-delay-ms 0
                          :jitter-ms 0})))
  (when-not @client
    (throw (ex-info "real Fram head daemon did not become ready"
                    {:type :daemon-start-failed
                     :daemon-output (when (.exists daemon-output)
                                      (slurp daemon-output))})))
  (rpc/close! @client)
  (reset! client
          (rpc/connect "127.0.0.1" port space-id
                       {:connect-timeout-ms 500
                        :read-timeout-ms 5000
                        :max-attempts 3
                        :retry-delay-ms 1
                        :jitter-ms 1}))

  (let [status (rpc/status! @client)]
    (check! "status validates the configured SpaceId"
            (= space-id (:space-id status)))
    (check! "status decodes the head daemon state and engine"
            (and (= :ready (:state status)) (= :rpc/jvm (:engine status)))))
  (check! "version returns the served transaction version"
          (integer? (:served-version (rpc/version! @client))))

  (let [proposition (t/triple "@round-trip" :stage "asserted")
        asserted (rpc/assert! @client proposition)
        shown (rpc/scan-all! @client "@round-trip" :stage nil)
        retracted (rpc/retract! @client proposition)
        absent (rpc/scan-all! @client "@round-trip" :stage nil)]
    (check! "assert round-trip reports one changed action"
            (true? (get-in asserted [:results 0 :changed?])))
    (check! "scan observes the asserted recursive Term"
            (= [proposition] (:rows shown)))
    (check! "retract round-trip reports one changed action"
            (true? (get-in retracted [:results 0 :changed?])))
    (check! "scan observes the exact projection after retract"
            (empty? (:rows absent))))

  (let [actions
        [{:op :rpc/assert :proposition (t/triple "@batch" :value "a")}
         {:op :rpc/assert :proposition (t/triple "@batch" :value "b")}
         {:op :rpc/retract :proposition (t/triple "@batch" :value "a")}]
        result (rpc/batch! @client actions)
        values (mapv t/triple-slot2
                     (:rows (rpc/scan-all! @client "@batch" :value nil)))]
    (check! "batch returns one typed action result per input"
            (= [true true true] (mapv :changed? (:results result))))
    (check! "batch is visible atomically at the exact projection"
            (= ["b"] values)))

  (doseq [batch (partition-all 75 (range 450))]
    (rpc/batch!
     @client
     (mapv (fn [index]
             {:op :rpc/assert
              :proposition (t/triple (str "@page-" index) :page index)})
           batch)))

  (let [request (wire/rpc-query-request! (query-plan :page) wire/query-current)
        drained (rpc/query-all! @client request)]
    (check! "paged query drains every row from a real head daemon"
            (= 450 (count (:rows drained))))
    (check! "paged query uses the TermCodec-safe effective page size"
            (= 3 (:pages drained)))
    (check! "query page rows keep deterministic first/last values"
            (= #{0 449}
               (set (map second [(first (:rows drained))
                                 (last (:rows drained))])))))

  (let [depth-error
        (thrown-data #(rpc/scan! @client nil :page nil))]
    (check! "unpaged depth overflow surfaces :term-depth-exceeded"
            (= :term-depth-exceeded (:type depth-error))))

  (let [current (:served-version (rpc/version! @client))
        conflict
        (thrown-data
         #(rpc/assert! @client (t/triple "@occ" :value "stale")
                       {:expected-version (dec current)}))]
    (check! "OCC conflict is typed and never hidden by retry"
            (and (= :rpc/conflict (:type conflict))
                 (= 1 (:attempts conflict)))))

  (let [calls (atom [])
        transient? (atom true)
        response
        (binding [rpc/*round-trip!*
                  (fn [logical-client request]
                    (swap! calls conj (t/rpcrequest-op request))
                    (if (compare-and-set! transient? true false)
                      (throw (java.net.SocketTimeoutException. "injected transient"))
                      (rpc/transport-round-trip! logical-client request)))]
          (rpc/version! @client))]
    (check! "transient transport failure is retried within the bound"
            (= 2 (:attempts response)))
    (check! "retry asks the same operation, never a cold fallback"
            (= [:rpc/version :rpc/version] @calls)))

  (rpc/profile-write! @client "@profile" :color ["blue"]
                      {:cardinality :one})
  (let [first-batch? (atom true)
        result
        (binding [rpc/*round-trip!*
                  (fn [logical-client request]
                    (if (and (= :rpc/batch (t/rpcrequest-op request))
                             (compare-and-set! first-batch? true false))
                      (do
                        (rpc/transport-round-trip! logical-client request)
                        (throw (java.net.SocketTimeoutException.
                                "lost write acknowledgement")))
                      (rpc/transport-round-trip! logical-client request)))]
          (rpc/profile-write! @client "@profile" :color ["red"]
                              {:cardinality :one}))
        values (mapv t/triple-slot2
                     (:rows (rpc/scan-all! @client "@profile" :color nil)))]
    (check! "ambiguous profile write is resolved by exact projection readback"
            (true? (:resolved-ambiguity? result)))
    (check! "profile-aware single write atomically replaces the old value"
            (= ["red"] values)))

  ;; --- North projection semantics over the append-only head wire -------------
  (let [proposition (t/triple "@projected" "note" "one")
        first-write (rpc/assert-projected! @client proposition)
        repeat-write (rpc/assert-projected! @client proposition)
        values (mapv t/triple-slot2
                     (:rows (rpc/scan-all! @client "@projected" "note" nil)))]
    (check! "an undeclared predicate reconciles with multi cardinality"
            (= :many (:cardinality first-write)))
    (check! "a repeated identical assertion writes nothing"
            (and (true? (:changed? first-write))
                 (false? (:changed? repeat-write))))
    (check! "a repeated identical assertion leaves one live occurrence"
            (= ["one"] values)))

  (let [rival (rpc/assert-projected! @client (t/triple "@projected" "note" "two"))
        values (set (mapv t/triple-slot2
                          (:rows (rpc/scan-all! @client "@projected" "note" nil))))]
    (check! "multi-valued rivals coexist under the projection layer"
            (and (true? (:changed? rival)) (= #{"one" "two"} values))))

  (rpc/assert! @client (t/triple "@projected" "note" "one"))
  (let [retracted (rpc/retract-projected!
                   @client (t/triple "@projected" "note" "one"))
        values (mapv t/triple-slot2
                     (:rows (rpc/scan-all! @client "@projected" "note" nil)))
        absent (rpc/retract-projected!
                @client (t/triple "@projected" "note" "one"))]
    (check! "projected retract withdraws EVERY equal occurrence"
            (and (true? (:changed? retracted)) (= ["two"] values)))
    (check! "projected retract of an absent value writes nothing"
            (false? (:changed? absent))))

  (rpc/assert! @client (t/triple "@title" "cardinality" "single"))
  (rpc/assert! @client (t/triple "@note" "cardinality" "multi"))
  (rpc/reset-cardinality-cache!)
  (rpc/assert-projected! @client (t/triple "@declared" "title" "first"))
  (let [operations (atom [])
        superseded
        (binding [rpc/*round-trip!*
                  (fn [logical-client request]
                    (swap! operations conj (t/rpcrequest-op request))
                    (rpc/transport-round-trip! logical-client request))]
          (rpc/assert-projected! @client (t/triple "@declared" "title" "second")))
        values (mapv t/triple-slot2
                     (:rows (rpc/scan-all! @client "@declared" "title" nil)))]
    (check! "graph cardinality declares the predicate single-valued"
            (= :one (:cardinality superseded)))
    (check! "a declared-single assertion supersedes the live value"
            (= ["second"] values))
    (check! "supersession lands as ONE batch, never a retract then an assert"
            (and (= 1 (count (filter #{:rpc/batch} @operations)))
                 (empty? (filter #{:rpc/assert :rpc/retract} @operations)))))

  (rpc/reset-cardinality-cache!)
  (let [graph-silent
        (binding [rpc/*env* {"FRAM_SINGLE_VALUED" "owner lead driver"}]
          (rpc/cardinality-of @client "lead"))
        graph-wins
        (binding [rpc/*env* {"FRAM_SINGLE_VALUED" "note"}]
          (rpc/reset-cardinality-cache!)
          (rpc/cardinality-of @client "note"))]
    (check! "FRAM_SINGLE_VALUED decides cardinality while the graph is silent"
            (= :one graph-silent))
    (check! "a graph multi declaration outranks the launcher export"
            (= :many graph-wins)))
  (rpc/reset-cardinality-cache!)

  (let [attempts (atom 0)
        response
        (binding [rpc/*round-trip!*
                  (fn [logical-client request]
                    (if (= 1 (swap! attempts inc))
                      (wire/rpc-response!
                       (t/rpcrequest-space request) (t/rpcrequest-op request) 0
                       nil
                       (wire/rpc-error! :query/archive-unavailable true
                                        "archive segment is not resident" nil)
                       nil)
                      (rpc/transport-round-trip! logical-client request)))]
          (rpc/version! @client))]
    (check! "the retry allowlist matches the daemon's own retryable set"
            (contains? rpc/retryable-error-codes :query/archive-unavailable))
    (check! "an archive-unavailable answer is retried, never surfaced"
            (= 2 (:attempts response))))

  (let [fence (:fence (rpc/lease-acquire! @client "resource" "holder" 30000))
        checked (rpc/lease-check! @client fence)
        next-fence (:fence (rpc/lease-renew! @client fence 30000))
        old-check (rpc/lease-check! @client fence)
        released (rpc/lease-release! @client next-fence)]
    (check! "lease acquire/check returns a valid typed fence"
            (:valid? checked))
    (check! "lease renew supersedes the previous fence"
            (and (not= fence next-fence) (not (:valid? old-check))))
    (check! "lease release accepts only the current fence"
            (:released? released)))

  (let [occurrences (rpc/occurrences-all! @client)
        validation (rpc/validate! @client)]
    (check! "paged occurrences drain returns native coordinates"
            (and (pos? (count (:rows occurrences)))
                 (every? t/triple? (:rows occurrences))))
    (check! "validate decodes the typed validation record"
            (and (:valid? validation) (empty? (:violations validation)))))

  (let [log-fence
        (binding [rpc/*round-trip!*
                  (fn [_ request]
                    (wire/rpc-response!
                     (t/rpcrequest-space request) (t/rpcrequest-op request) 7
                     nil
                     (wire/rpc-error! :log-fence-required false
                                      "expected log fence is required" nil)
                     nil))]
          (thrown-data #(rpc/version! @client)))]
    (check! "wire error decoding preserves :log-fence-required"
            (and (= :log-fence-required (:type log-fence))
                 (false? (:retryable log-fence)))))

  (let [oversize-response
        (thrown-data
         #(rpc/read-frame!
           (java.io.ByteArrayInputStream. (malformed-oversize-frame))))
        oversized-string (apply str (repeat rpc/max-body-bytes "x"))
        oversize-request
        (thrown-data
         #(rpc/assert! @client
                       (t/triple "@boundary" :payload oversized-string)))]
    (check! "declared response above 1 MiB is rejected before body allocation"
            (and (= :rpc-frame-too-large (:type oversize-response))
                 (= (inc rpc/max-body-bytes) (:body-length oversize-response))))
    (check! "encoded request above 1 MiB is rejected before socket write"
            (and (= :rpc-frame-too-large (:type oversize-request))
                 (false? (:request-sent? oversize-request)))))

  (let [wrong-space
        (thrown-data
         #(rpc/connect "127.0.0.1" port "wrong-space"
                       {:max-attempts 1 :retry-delay-ms 0 :jitter-ms 0}))]
    (check! "connect rejects a daemon serving a different SpaceId"
            (= :rpc/space-mismatch (:type wrong-space))))

  (let [subscription (thrown-data #(rpc/subscribe! @client {}))]
    (check! "subscription remains an explicit, unwired stage seam"
            (and (= :rpc/subscription-unavailable (:type subscription))
                 (= :rpc/subscribe (:operation subscription)))))

  (rpc/close! @client)
  (check! "close makes subsequent operations fail loudly"
          (= :rpc/client-closed
             (:type (thrown-data #(rpc/status! @client)))))

  (finally
    (when (and @client (not (rpc/closed? @client)))
      (rpc/close! @client))
    (when-let [^Process process @daemon]
      (.destroy process)
      (when-not (.waitFor process 5 java.util.concurrent.TimeUnit/SECONDS)
        (.destroyForcibly process)
        (.waitFor process 5 java.util.concurrent.TimeUnit/SECONDS)))
    (fs/delete-tree scratch)))

(let [failures (remove second @checks)]
  (println)
  (if (seq failures)
    (do
      (println "framrpc client:" (- (count @checks) (count failures)) "/"
               (count @checks) "PASS")
      (System/exit 1))
    (println "framrpc client:" (count @checks) "/" (count @checks) "PASS")))
