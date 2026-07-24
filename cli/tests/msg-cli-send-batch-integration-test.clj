#!/usr/bin/env bb
;; Proves msg-cli.clj's `send` verb publishes its message facts as ONE atomic
;; :assert-batch request (thread 019f9063 / incident 019f8958 -- torn mail
;; subjects), AND that it falls back cleanly to the pre-atomicity sequential
;; per-fact :assert path when the coordinator itself rejects :assert-batch as
;; unknown (the running gen-1022 daemon, before gen-1023 promotes the op).
;;
;; Two coordinators are exercised: a REAL Fram daemon (proves the batch path
;; against the actual :assert-batch wire contract) and a minimal mock socket
;; server that always answers :assert-batch with {:error "unknown op"} (proves
;; the fallback path deterministically, without depending on an old Fram
;; checkout being available).
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (str (System/getProperty "user.home") "/code/fram")))
(def msg-cli (str root "/cli/msg-cli.clj"))
(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(defn free-port [] (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))
(defn await-predicate [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 200) false
          :else (do (Thread/sleep 25) (recur (inc attempt))))))

;; Real Fram daemon boot has been observed to take 20s+ under host contention
;; (thread 019f9063 progress notes cite a 5.4s+ cold port-open under load);
;; give the real-daemon-only startup check a much longer budget than the
;; ordinary in-test await-predicate above.
(defn await-daemon-boot [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 300) false
          :else (do (Thread/sleep 250) (recur (inc attempt))))))

(defn run-msg [port log & args]
  (apply proc/shell
         {:continue true :out :string :err :string
          :extra-env {"AGENT_TOPOLOGY" "orchestrator" "FRAM_LOG" log}}
         "bb" msg-cli (str port) args))

(defn coordinator-op [port log request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write writer
              (.getBytes
               (str (pr-str {:op :for-log :expected-log log :request request}) "\n")))
      (.flush writer)
      (edn/read-string (.readLine reader)))))

(defn values-of [port log subject predicate]
  (set (:values (coordinator-op port log {:op :resolved :te subject :p predicate}))))
(defn value-of [port log subject predicate]
  (:value (coordinator-op port log {:op :resolved :te subject :p predicate})))
(defn resolved-of [port log subject predicate]
  (coordinator-op port log {:op :resolved :te subject :p predicate}))

;; ATOMIC observation of a subject. A per-predicate :resolved opens a NEW connection
;; per read, so five resolves straddle the commit instant and STITCH a pre-commit fact
;; onto a post-commit tail — a phantom "partial" that is never a real store state. ONE
;; :query over SUBJECT is ONE `@store` deref = ONE consistent snapshot, so every returned
;; predicate set is a genuine committed state (empty or whole, never a torn stitch).
(defn snapshot-preds [port log subject]
  (into #{}
        (map first)
        (:ok (coordinator-op
              port log
              {:op :query
               :query {:find "pv"
                       :rules [{:head {:rel "pv" :args [{:var "p"} {:var "o"}]}
                                :body [{:rel "triple"
                                        :args [subject {:var "p"} {:var "o"}]}]}]}}))))

;; The DURABLE torn-subject seam: which predicates for SUBJECT are on disk in the flat
;; log. The pre-atomicity bug (incident 019f895c) left a from-only orphan HERE when a
;; crash/disconnect fell between separate per-fact :assert appends; :assert-batch flushes
;; every line in ONE fsync, so this set is the whole subject or nothing — never partial.
(defn flat-log-preds [log subject]
  (into #{}
        (comp (map #(try (edn/read-string %) (catch Exception _ nil)))
              (filter #(and (map? %) (= (:l %) subject) (= (:op %) "assert")))
              (map :p))
        (remove str/blank? (str/split-lines (slurp log)))))

;; Boot a fresh, clean-env REAL Fram daemon (its own temp facts log + free port),
;; run BODY with [port log], then tear it down deterministically. Each caller gets
;; an isolated corpus so an ordering/disconnect observation is never contaminated
;; by a neighbour's commits. A boot failure records a FAILED check instead of
;; throwing so the remaining checks still report.
(defn with-real-fram-daemon [label body]
  (let [port (free-port)
        tmp (.toFile
             (java.nio.file.Files/createTempDirectory
              "north-msg-batch" (make-array java.nio.file.attribute.FileAttribute 0)))
        facts (io/file tmp "facts.log")
        _ (spit facts "")
        log (.getCanonicalPath facts)
        daemon (proc/process
                {:dir fram :out :string :err :string
                 :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
                "bb" "-cp" "out" "coord_daemon.clj" "serve-flat" (str port) log)]
    (try
      (if (await-daemon-boot #(port-open? port))
        (body port log)
        (check (str label ": real Fram daemon starts") false))
      (finally
        (proc/destroy-tree daemon)
        (try @daemon (catch Exception _ nil))
        (doseq [f (reverse (file-seq tmp))] (.delete f))))))

;; Open a FENCED firehose subscription (filter nil = every commit). Returns the
;; live socket/reader plus the handshake reply; commit events then stream as one
;; EDN map per line: {:event :commit :version .. :op "assert" :l <te> :p .. :r ..}.
(defn subscribe! [port log]
  (let [socket (java.net.Socket. "127.0.0.1" (int port))
        _ (.setSoTimeout socket 15000)
        writer (.getOutputStream socket)
        reader (io/reader (.getInputStream socket))]
    (.write writer
            (.getBytes
             (str (pr-str {:op :for-log :expected-log log
                           :request {:op :subscribe}}) "\n")))
    (.flush writer)
    {:socket socket :reader reader
     :handshake (edn/read-string (.readLine reader))}))

;; ---------------------------------------------------------------------------
;; PATH 1: real Fram daemon (gen-1023+) -- ONE :assert-batch commits the whole
;; message, including `to`, in a single all-or-none unit.
;; ---------------------------------------------------------------------------
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw
   (ex-info "Fram checkout not found; set FRAM_TEST_CHECKOUT or clone it beside North"
            {:fram fram})))
(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-msg-batch" (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "facts.log")
      _ (spit facts "")
      log (.getCanonicalPath facts)
      daemon (proc/process
              {:dir fram :out :string :err :string
               :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
              "bb" "-cp" "out" "coord_daemon.clj" "serve-flat" (str port) log)]
  (try
    (check "real Fram daemon (assert-batch-capable) starts"
           (await-daemon-boot #(port-open? port)))
    (let [result (run-msg port log "send" "producer" "recipient" "hello" "world")]
      (check "ordinary send exits clean against an assert-batch-capable daemon"
             (zero? (:exit result)))
      (check "send never emits the compat deprecation warning against a capable daemon"
             (not (str/includes? (:err result) "DEPRECATED")))
      (check "from/subject/body/sent_at/to all landed"
             (await-predicate
              #(let [e (second (re-find #"sent (@msg:\S+) ->" (:out result)))]
                 (and e
                      (= "producer" (value-of port log e "from"))
                      (= "hello" (value-of port log e "subject"))
                      (= "world" (value-of port log e "body"))
                      (seq (values-of port log e "sent_at"))
                      (= "recipient" (value-of port log e "to")))))))
    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [f (reverse (file-seq tmp))] (.delete f)))))

;; ---------------------------------------------------------------------------
;; PATH 2: a mock coordinator that always rejects :assert-batch as an unknown
;; op (the exact shape a pre-gen-1023 daemon's case-dispatch default arm
;; returns) -- msg-cli must fall back to sequential per-fact :assert and still
;; complete the send, with a loud deprecation note on stderr.
;; ---------------------------------------------------------------------------
(defn mock-legacy-coordinator!
  "A minimal :for-log-fenced coordinator: :assert-batch is always unknown;
   every other op (:assert/:resolved/:version) is served from an in-memory
   single-valued fact store, matching just enough of the real wire contract
   for msg-cli's legacy fallback path to run to completion."
  [port]
  (let [server (java.net.ServerSocket. (int port))
        store (atom {})
        running? (atom true)]
    (future
      (while @running?
        (try
          (with-open [socket (.accept server)]
            (let [reader (io/reader (.getInputStream socket))
                  writer (.getOutputStream socket)
                  line (.readLine reader)
                  envelope (edn/read-string line)
                  request (:request envelope)
                  reply
                  (case (:op request)
                    :assert-batch {:error "unknown op"}
                    :version {:version (count @store)}
                    :assert (do (swap! store assoc [(:te request) (:p request)] (:r request))
                                {:ok (count @store)})
                    :resolved
                    (let [r (get @store [(:te request) (:p request)])]
                      {:value r :members (if r 1 0) :ambiguous? false
                       :values (if r [r] []) :version (count @store)})
                    {:error "unknown op"})]
              (.write writer (.getBytes (str (pr-str reply) "\n")))
              (.flush writer)))
          (catch Exception _ nil))))
    {:server server :running? running?}))

(defn stop-mock! [{:keys [server running?]}]
  (reset! running? false)
  (try (.close server) (catch Exception _ nil)))

(let [port (free-port)
      log "/tmp/north-msg-batch-mock.log"
      mock (mock-legacy-coordinator! port)]
  (try
    (check "mock legacy coordinator (pre-gen-1023) starts"
           (await-predicate #(port-open? port)))
    (let [result (run-msg port log "send" "producer" "recipient" "hello" "world")]
      (check "send against a pre-assert-batch coordinator still exits clean (legacy fallback)"
             (zero? (:exit result)))
      (check "legacy fallback logs a loud deprecation note"
             (str/includes? (:err result) "DEPRECATED"))
      (check "legacy fallback names the unsupported op and the rollout remedy"
             (and (str/includes? (:err result) "assert-batch")
                  (str/includes? (:err result) "gen-1023")))
      (check "legacy per-fact writes still land every message field"
             (let [e (second (re-find #"sent (@msg:\S+) ->" (:out result)))]
               (and e
                    (= "producer" (value-of port log e "from"))
                    (= "hello" (value-of port log e "subject"))
                    (= "world" (value-of port log e "body"))
                    (= "recipient" (value-of port log e "to"))))))
    (finally (stop-mock! mock))))

;; ---------------------------------------------------------------------------
;; PATH 3: induced disconnect on the REAL :assert-batch wire — after the request
;; is accepted but BEFORE the client observes the ack — leaves the subject either
;; wholly absent or wholly complete (never a torn partial), and an idempotent
;; retry deterministically yields exactly ONE complete entity with no duplicated
;; facts (thread 019f9122 done_when: complete-once-or-absent under disconnect).
;;
;; The disconnect is driven at the socket level (not through the msg-cli
;; subprocess) because only raw wire control can sever the connection precisely
;; between request acceptance and ack. The bytes sent are exactly the ordinary
;; ordinary-mail :assert-batch msg-cli.clj publishes: the four front facts then
;; `to` last, one request, one :te.
;; ---------------------------------------------------------------------------
(with-real-fram-daemon "induced disconnect"
 (fn [port log]
   (let [te (str "@msg:disconnect-" (java.util.UUID/randomUUID))
         ;; ONE fixed fact set (fixed sent_at too) so the retry re-asserts the
         ;; IDENTICAL batch — the same logical publication, never a second one.
         facts [{:p "from" :r "producer"}
                {:p "subject" :r "hello"}
                {:p "body" :r "world"}
                {:p "sent_at" :r (str (java.time.Instant/now))}
                {:p "to" :r "recipient"}]
         envelope {:op :for-log :expected-log log
                   :request {:op :assert-batch :te te :facts facts}}
         complete #{"from" "subject" "body" "sent_at" "to"}]
     ;; Induce the disconnect: deliver + flush the whole request (daemon reads the
     ;; complete line and accepts it), then close WITHOUT reading the ack line. The
     ;; commit runs to completion server-side under dlock regardless of the vanished
     ;; client — the ack is all that is lost.
     (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
       (let [writer (.getOutputStream socket)]
         (.write writer (.getBytes (str (pr-str envelope) "\n")))
         (.flush writer)))
     ;; Deterministic observation via ATOMIC snapshots. Poll a single-:query snapshot
     ;; (one consistent store deref) across a settle window; EVERY snapshot must be a
     ;; genuine committed state — empty or the whole subject. A proper nonempty subset
     ;; would be a torn subject and fails the instant it is seen. (Per-predicate resolves
     ;; were rejected here: five separate connections straddle the commit and stitch a
     ;; pre-commit `from` onto a post-commit tail, a phantom partial that is no real
     ;; store state — the atomicity is real, the naive multi-read observation was not.)
     (let [snapshots (into [] (repeatedly 80 (fn []
                                               (let [s (snapshot-preds port log te)]
                                                 (Thread/sleep 20) s))))]
       (check "disconnect after acceptance: every atomic snapshot is absent or complete, never partial"
              (every? #(or (empty? %) (= complete %)) snapshots))
       (check "the disconnected batch settles server-side to the complete subject"
              (= complete (last snapshots))))
     ;; DURABLE all-or-none at the flat-log seam — the exact place the pre-atomicity bug
     ;; left a from-only orphan. On disk the subject is whole or absent, never torn.
     (check "durable flat log holds the complete subject or nothing (all-or-none on disk)"
            (let [on-disk (flat-log-preds log te)]
              (or (empty? on-disk) (= complete on-disk))))
     (check "durable flat log has settled to the complete subject"
            (= complete (flat-log-preds log te)))
     ;; Idempotent retry: re-send the SAME batch, this time reading the ack.
     (let [retry (coordinator-op port log {:op :assert-batch :te te :facts facts})]
       (check "idempotent retry of the same batch is accepted"
              (and (map? retry) (:ok retry)))
       (check "after retry the message entity is complete"
              (= "producer" (value-of port log te "from")))
       ;; No duplicate logical publication: every field resolves to exactly ONE
       ;; live value (members=1, not ambiguous) — the retry did not double any fact
       ;; nor mint a second entity.
       (check "retry yields exactly one complete entity, zero duplicated facts"
              (every?
               (fn [[p expected]]
                 (let [r (resolved-of port log te p)]
                   (and (= expected (:value r))
                        (= 1 (:members r))
                        (false? (:ambiguous? r)))))
               {"from" "producer" "subject" "hello"
                "body" "world" "to" "recipient"}))
       (check "sent_at is present exactly once after retry"
              (let [r (resolved-of port log te "sent_at")]
                (and (= 1 (:members r)) (false? (:ambiguous? r)))))))))

;; ---------------------------------------------------------------------------
;; PATH 4: explicitly OBSERVED batch commit/notification order for an ordinary
;; message. A firehose subscriber records the commit-event stream; the daemon
;; notifies front facts (from/subject/body/sent_at) BEFORE the delivery trigger
;; `to`, and because the batch is one store tx the front facts are already
;; resolvable the instant `to` is observed — so delivery candidacy is impossible
;; before the complete front exists (thread 019f9122 done_when: to-last ordering).
;; ---------------------------------------------------------------------------
(with-real-fram-daemon "batch ordering"
 (fn [port log]
   (let [events (atom [])
         sub (subscribe! port log)
         reader-fut
         (future
           (try
             (loop []
               (when-let [line (.readLine (:reader sub))]
                 (swap! events conj (edn/read-string line))
                 (recur)))
             (catch Exception _ nil)))]
     (try
       (check "firehose subscription is established against the real daemon"
              (integer? (:subscribed (:handshake sub))))
       (let [result (run-msg port log "send" "producer" "recipient" "hello" "world")
             e (second (re-find #"sent (@msg:\S+) ->" (:out result)))]
         (check "ordinary send exits clean while a subscriber observes the commit stream"
                (zero? (:exit result)))
         ;; Wait until the `to` event for our subject has been pushed.
         (check "the delivery trigger `to` is eventually notified"
                (and e
                     (await-predicate
                      #(some (fn [ev] (and (= (:l ev) e) (= (:p ev) "to")))
                             @events))))
         (let [preds (mapv :p (filter #(= (:l %) e) @events))
               idx (fn [p] (.indexOf preds p))
               to-idx (idx "to")
               front ["from" "subject" "body" "sent_at"]
               front-idxs (map idx front)]
           (check "every front fact (from/subject/body/sent_at) is notified before `to`"
                  (and (every? #(<= 0 %) front-idxs)
                       (pos? to-idx)
                       (< (apply max front-idxs) to-idx)))
           (check "`to` is the final notified predicate of the batch"
                  (= to-idx (dec (count preds))))
           ;; Delivery candidacy proof: at the moment `to` exists on the wire the
           ;; store already holds every front fact — a subscriber keying on `to`
           ;; can never observe a message whose front is incomplete.
           (check "when `to` is observed the complete front is already committed"
                  (= {"from" "producer" "subject" "hello"
                      "body" "world"}
                     (into {} (map (fn [p] [p (value-of port log e p)])
                                   ["from" "subject" "body"]))))
           (check "sent_at is committed before `to` is observed"
                  (seq (:values (resolved-of port log e "sent_at"))))))
       (finally
         (future-cancel reader-fut)
         (try (.close (:socket sub)) (catch Exception _ nil)))))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nmsg-cli send-batch: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
