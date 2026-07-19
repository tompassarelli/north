#!/usr/bin/env bb
;; End-to-end durability contract for the managed SDK live-input feed.
;; A throwaway Fram coordinator keeps every assertion isolated from live North.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (str (System/getProperty "user.home") "/code/fram")))
(def live-feed-cli (str root "/cli/north-live-feed.clj"))
(def msg-cli (str root "/cli/msg-cli.clj"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(def identity-writer (str root "/cli/agent-fact-internal.clj"))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/message-audience.clj"))
(def checks (atom []))
(def feeds (atom []))
(def test-log (atom nil))

(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket
                (java.net.InetSocketAddress. "127.0.0.1" (int port))
                100)
      true)
    (catch Exception _ false)))

(defn await-predicate
  ([predicate] (await-predicate predicate 6000))
  ([predicate timeout-ms]
   (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
     (loop []
       (cond
         (predicate) true
         (>= (System/currentTimeMillis) deadline) false
         :else (do (Thread/sleep 20) (recur)))))))

(defn coordinator-op [port request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout socket 5000)
    (let [writer (.getOutputStream socket)
          reader (io/reader (.getInputStream socket))]
      (.write writer
              (.getBytes
               (str (pr-str {:op :for-log
                             :expected-log @test-log
                             :request request})
                    "\n")
               java.nio.charset.StandardCharsets/UTF_8))
      (.flush writer)
      (edn/read-string (.readLine reader)))))

(defn values-of [port subject predicate]
  (set (:values
        (coordinator-op port {:op :resolved
                              :te subject
                              :p predicate}))))

(defn assert-fact! [port subject predicate value]
  (let [result
        (coordinator-op port {:op :assert
                              :te subject
                              :p predicate
                              :r value})]
    (when (:reject result)
      (throw (ex-info "fixture fact write failed" result)))
    result))

(defn delivery-resource [message recipient]
  (let [preimage (str message "\u0000" recipient)
        digest
        (.digest (java.security.MessageDigest/getInstance "SHA-256")
                 (.getBytes preimage java.nio.charset.StandardCharsets/UTF_8))]
    (str "message-delivery:"
         (apply str
                (map #(format "%02x" (bit-and (int %) 0xff)) digest)))))

(defn retract-fact! [port subject predicate value]
  (let [result
        (coordinator-op port {:op :retract
                              :te subject
                              :p predicate
                              :r value})]
    (when (:reject result)
      (throw (ex-info "fixture fact retraction failed" result)))
    result))

(defn run-cli [path port & args]
  (apply proc/shell
         {:continue true
          :out :string
          :err :string
          :extra-env {"FRAM_LOG" @test-log}}
         "bb" path (str port) args))

(defn run-msg [port & args]
  (apply run-cli msg-cli port args))

(defn register! [port handle]
  (run-cli presence-cli port "register" handle (str "/tmp/" handle) handle))

(defn sent-subject [result]
  (second (re-find #"sent (@msg:[^ ]+)" (:out result))))

(defn send-message! [port from to subject body]
  (let [result (run-msg port "send" from to subject body)
        message (sent-subject result)]
    (when-not (and (zero? (:exit result)) message)
      (throw
       (ex-info "message fixture failed"
                {:exit (:exit result)
                 :out (:out result)
                 :err (:err result)})))
    message))

(defn start-line-reader! [reader queue]
  (future
    (try
      (loop []
        (if-let [line (.readLine ^java.io.BufferedReader reader)]
          (do (.put queue {:kind :line :line line})
              (recur))
          (.put queue {:kind :eof})))
      (catch Exception error
        (.put queue {:kind :error :error error})))))

(defn start-error-reader! [reader errors]
  (future
    (try
      (loop []
        (when-let [line (.readLine ^java.io.BufferedReader reader)]
          (swap! errors conj line)
          (recur)))
      (catch Exception _ nil))))

(defn start-feed!
  [port recipient claim-ttl-ms ack-timeout-ms & flags]
  (let [command (into ["bb" live-feed-cli (str port) recipient
                       "--claim-ttl-ms" (str claim-ttl-ms)
                       "--ack-timeout-ms" (str ack-timeout-ms)]
                      flags)
        builder (ProcessBuilder. ^java.util.List command)
        _ (.directory builder (io/file root))
        _ (.put (.environment builder) "FRAM_LOG" @test-log)
        process (.start builder)
        output (io/reader (.getInputStream process))
        errors-reader (io/reader (.getErrorStream process))
        writer (io/writer (.getOutputStream process))
        queue (java.util.concurrent.LinkedBlockingQueue.)
        errors (atom [])
        feed {:process process
              :writer writer
              :queue queue
              :errors errors}]
    (start-line-reader! output queue)
    (start-error-reader! errors-reader errors)
    (swap! feeds conj feed)
    feed))

(defn stop-feed! [feed]
  (when feed
    (let [process ^Process (:process feed)]
      (try (.close ^java.io.Writer (:writer feed)) (catch Exception _ nil))
      (when (.isAlive process)
        (.destroyForcibly process)
        (.waitFor process 2 java.util.concurrent.TimeUnit/SECONDS))
      (swap! feeds
             (fn [current]
               (vec (remove #(identical? % feed) current)))))))

(defn read-frame!
  ([feed] (read-frame! feed 2000))
  ([feed timeout-ms]
   (let [item (.poll ^java.util.concurrent.BlockingQueue
                     (:queue feed)
                     timeout-ms
                     java.util.concurrent.TimeUnit/MILLISECONDS)]
     (cond
       (nil? item) nil
       (= :line (:kind item))
       (try
         (json/parse-string-strict (:line item))
         (catch Exception error
           (throw
            (ex-info "live feed emitted malformed JSON"
                     {:line (:line item)}
                     error))))
       :else
       (throw
        (ex-info "live feed closed unexpectedly"
                 {:item item
                  :errors @(:errors feed)}))))))

(defn frames-until-eof!
  "Drain a terminated feed's remaining output under one wall-clock bound."
  [feed timeout-ms]
  (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
    (loop [frames []]
      (let [remaining (- deadline (System/currentTimeMillis))]
        (when-not (pos? remaining)
          (throw (ex-info "timed out draining terminated feed output"
                          {:frames frames :errors @(:errors feed)})))
        (let [item (.poll ^java.util.concurrent.BlockingQueue
                          (:queue feed)
                          remaining
                          java.util.concurrent.TimeUnit/MILLISECONDS)]
          (cond
            (nil? item)
            (throw (ex-info "terminated feed did not close stdout"
                            {:frames frames :errors @(:errors feed)}))

            (= :line (:kind item))
            (recur (conj frames (json/parse-string-strict (:line item))))

            (= :eof (:kind item))
            frames

            :else
            (throw (ex-info "terminated feed output reader failed"
                            {:item item :errors @(:errors feed)}))))))))

(defn require-frame! [feed type]
  (let [frame (read-frame! feed)]
    (when-not (= type (get frame "type"))
      (throw
       (ex-info "live feed emitted an unexpected frame"
                {:expected type
                 :actual frame
                 :errors @(:errors feed)})))
    frame))

(defn send-control! [feed frame]
  (let [writer ^java.io.Writer (:writer feed)]
    (.write writer (str (json/generate-string frame) "\n"))
    (.flush writer)))

(defn send-control-line! [feed line]
  (let [writer ^java.io.Writer (:writer feed)]
    (.write writer (str line "\n"))
    (.flush writer)))

(defn start! [feed recipient]
  (let [ready (require-frame! feed "ready")]
    (check (str recipient " receives a separate readiness frame")
           (and (= "north-live-feed-v1" (get ready "protocol"))
                (= recipient (get ready "recipient"))
                (integer? (get ready "subscribed"))))
    (send-control! feed (array-map "type" "start"))))

(defn ack! [feed message]
  (send-control! feed (array-map "type" "ack" "id" message)))

(defn mail-id [frame]
  (when (= "mail" (get frame "type"))
    (get frame "id")))

(defn await-acked! [port recipient messages]
  (await-predicate
   #(every? (fn [message]
              (= #{recipient} (values-of port message "acked_by")))
            messages)))

(defn publish-route! [port id verb state epoch]
  (let [base {"kind" "lane"
              "role" "integrator"
              "goal" "terminal drain integration"
              "provider" "anthropic"
              "provider_target" "anthropic-test"
              "live_input" "streaming"
              "live_input_state" state
              "live_input_epoch" epoch
              "model" "claude-opus-4-8"
              "effort" "xhigh"
              "composition_kind" "preset"
              "composition_id" "integrator"
              "composition_overrides" "[]"
              "repo" "~/code/north"
              "spawned_at" "2026-07-19T00:00:00Z"
              "display_handle" (str "anthropic-test-integrator-" id)
              "display_name" (str "anthropic · integrator · " id)}
        payload (if (= verb "publish")
                  base
                  (select-keys
                   base
                   ["provider" "provider_target" "live_input"
                    "live_input_state" "live_input_epoch" "model" "effort"
                    "display_handle" "display_name"]))
        result (run-cli identity-writer port verb
                        (str "agent:" id)
                        (json/generate-string payload))]
    (when-not (zero? (:exit result))
      (throw (ex-info "route fixture publication failed" result)))))

(defn pending-steers [port recipient]
  (with-redefs
   [north.coord/send-op
    (fn [query-port operation]
      (coordinator-op query-port operation))]
    (:messages
     (north.message-audience/pending-steer-page
      port recipient #{recipient}))))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-live-feed"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "facts.log")
      daemon
      (do
        (spit facts "")
        (proc/process
         {:dir fram
          :out :string
          :err :string
          :extra-env
          {"FRAM_REQUIRE_LOG_FENCE" "1"
           "FRAM_SINGLE_VALUED"
           "from subject body sent_at to acked_at broadcast_audience_version agent dir session_id started_at"}}
         "bb" "-cp" "out" "coord_daemon.clj"
         "serve-flat" (str port) (.getPath facts)))]
  (reset! test-log (.getCanonicalPath facts))
  (try
    (check "throwaway Fram coordinator starts"
           (await-predicate #(port-open? port)))
    (doseq [handle ["sender" "recipient"]]
      (check (str handle " has a live session lease")
             (zero? (:exit (register! port handle)))))

    ;; A message committed before the transport exists is replayed only after
    ;; the already-armed feed advertises readiness and receives `start`.
    (let [message
          (send-message!
           port "sender" "recipient" "before-arm" "durable replay")
          feed (start-feed! port "recipient" 3000 2000)]
      (check "commit-before-arm starts unacknowledged"
             (empty? (values-of port message "acked_by")))
      (start! feed "recipient")
      (let [mail (require-frame! feed "mail")]
        (check "commit-before-arm is replayed"
               (= message (mail-id mail)))
        (check "output alone never commits acknowledgement"
               (empty? (values-of port message "acked_by")))
        (ack! feed message)
        (check "host admission ack becomes durable"
               (await-acked! port "recipient" [message])))
      (stop-feed! feed))

    ;; The coordinator subscription is armed before `ready`. A commit landing
    ;; after readiness but before `start` cannot fall through an arm/replay gap.
    (let [feed (start-feed! port "recipient" 3000 2000)]
      (let [ready (require-frame! feed "ready")
            message
            (send-message!
             port "sender" "recipient" "during-arm" "queued while host starts")]
        (check "commit-during-arm readiness names the recipient"
               (= "recipient" (get ready "recipient")))
        (send-control! feed (array-map "type" "start"))
        (let [mail (require-frame! feed "mail")]
          (check "commit-during-arm is delivered"
                 (= message (mail-id mail)))
          (ack! feed message)
          (check "commit-during-arm is durably acknowledged"
                 (await-acked! port "recipient" [message]))
          (check "replay/live overlap cannot emit a duplicate"
                 (nil? (read-frame! feed 250)))))
      (stop-feed! feed))

    ;; Control input is authority-bearing. Duplicate JSON members must fail
    ;; closed, release the claim, and leave the message available to a clean
    ;; replacement instead of silently accepting last-key-wins parsing.
    (let [malformed-feed (start-feed! port "recipient" 500 300)]
      (start! malformed-feed "recipient")
      (let [message
            (send-message!
             port "sender" "recipient" "duplicate-control" "reject and replay")
            mail (require-frame! malformed-feed "mail")]
        (check "duplicate-control fixture reaches the ack boundary"
               (= message (mail-id mail)))
        (send-control-line!
         malformed-feed
         (str "{\"type\":\"ack\",\"type\":\"nack\",\"id\":"
              (json/generate-string message)
              "}"))
        (check "duplicate-key control terminates the feed"
               (await-predicate
                #(not (.isAlive ^Process (:process malformed-feed)))))
        (check "duplicate-key control cannot acknowledge mail"
               (empty? (values-of port message "acked_by")))
        (let [replacement (start-feed! port "recipient" 3000 2000)]
          (start! replacement "recipient")
          (let [replay (require-frame! replacement "mail")]
            (check "mail rejected by malformed control is replayed"
                   (= message (mail-id replay)))
            (ack! replacement message)
            (check "clean replacement can durably acknowledge replay"
                   (await-acked! port "recipient" [message])))
          (stop-feed! replacement)))
      (stop-feed! malformed-feed))

    ;; Role membership is dynamic authority. Replay may discover candidates from
    ;; its startup snapshot, but each claimed delivery must re-read the current
    ;; role. Conversely, a later role acquisition must wake replay for mail that
    ;; was committed before the acquisition.
    (let [agent "@agent:recipient"
          role "@role:reviewer"]
      (assert-fact! port agent "holds" role)
      (let [role-mail
            #{(send-message!
               port "sender" "reviewer" "role-before-replay-a" "candidate a")
              (send-message!
               port "sender" "reviewer" "role-before-replay-b" "candidate b")}
            feed (start-feed! port "recipient" 3000 2000)]
        (start! feed "recipient")
        (let [first-frame (require-frame! feed "mail")
              first-id (mail-id first-frame)
              withheld (first (disj role-mail first-id))]
          (check "role replay initially authorizes one current-role message"
                 (contains? role-mail first-id))
          (retract-fact! port agent "holds" role)
          (ack! feed first-id)
          (check "already-admitted role mail can finish after revocation"
                 (await-acked! port "recipient" [first-id]))
          (check "stale startup role snapshot cannot authorize later replay"
                 (and (nil? (read-frame! feed 300))
                      (empty? (values-of port withheld "acked_by"))))
          (let [committed-without-role
                (send-message!
                 port "sender" "reviewer" "role-before-acquire" "wake on acquire")]
            (check "role mail remains quiet before role acquisition"
                   (nil? (read-frame! feed 250)))
            (assert-fact! port agent "holds" role)
            (let [first-recovered (require-frame! feed "mail")
                  _ (ack! feed (mail-id first-recovered))
                  second-recovered (require-frame! feed "mail")
                  _ (ack! feed (mail-id second-recovered))
                  recovered
                  #{(mail-id first-recovered) (mail-id second-recovered)}]
              (check "role acquisition replays every newly eligible message"
                     (= #{withheld committed-without-role} recovered))
              (check "newly eligible role messages become durable"
                     (await-acked!
                      port "recipient" [withheld committed-without-role]))))
          (retract-fact! port agent "holds" role))
        (stop-feed! feed)))

    ;; A process death after output but before ack deliberately permits replay.
    ;; A second commit while no feed exists proves there is no rearm window.
    (let [first-feed (start-feed! port "recipient" 500 300)]
      (start! first-feed "recipient")
      (let [crashed
            (send-message!
             port "sender" "recipient" "crash-before-ack" "must replay")
            first-mail (require-frame! first-feed "mail")]
        (check "crash fixture reached the pre-ack boundary"
               (= crashed (mail-id first-mail)))
        (stop-feed! first-feed)
        (check "crash-before-ack leaves no durable acknowledgement"
               (empty? (values-of port crashed "acked_by")))
        (let [gap
              (send-message!
               port "sender" "recipient" "between-processes" "must replay too")]
          (Thread/sleep 600)
          (let [replacement (start-feed! port "recipient" 3000 2000)]
            (start! replacement "recipient")
            (let [first-replay (require-frame! replacement "mail")
                  _ (ack! replacement (mail-id first-replay))
                  second-replay (require-frame! replacement "mail")
                  _ (ack! replacement (mail-id second-replay))
                  observed (set (map mail-id [first-replay second-replay]))]
              (check "crash replay and no-listener gap both recover"
                     (= #{crashed gap} observed))
              (check "both recovered messages become durable"
                     (await-acked! port "recipient" [crashed gap]))
              (check "recovery emits each pending message once"
                     (nil? (read-frame! replacement 250))))
            (stop-feed! replacement)))))

    ;; Two live consumers may observe the same commit, but the coordinator lease
    ;; grants output authority to only one of them.
    (let [left (start-feed! port "recipient" 3000 2000)
          right (start-feed! port "recipient" 3000 2000)]
      (start! left "recipient")
      (start! right "recipient")
      (let [message
            (send-message!
             port "sender" "recipient" "competing-listeners" "one winner")
            left-read (future (read-frame! left 700))
            right-read (future (read-frame! right 700))
            left-frame @left-read
            right-frame @right-read
            deliveries (keep identity [left-frame right-frame])
            winner (if left-frame left right)]
        (check "coordinator claim elects exactly one live delivery"
               (and (= 1 (count deliveries))
                    (= message (mail-id (first deliveries)))))
        (ack! winner message)
        (check "the elected listener commits one durable acknowledgement"
               (await-acked! port "recipient" [message])))
      (stop-feed! left)
      (stop-feed! right))

    ;; Claim expiry is a clock edge, not a coordinator commit. Pre-acquire the
    ;; exact production lease before publishing `to` so the armed feed
    ;; deterministically loses the live-event claim. With no later graph write,
    ;; only its scheduled retry can make progress after expiry.
    (let [left (start-feed! port "recipient" 500 300)
          right (start-feed! port "recipient" 500 300)
          message
          "@msg:20260719-000002-00000000-0000-4000-8000-000000000001"]
      (start! left "recipient")
      (start! right "recipient")
      (doseq [[predicate value]
              [["from" "sender"]
               ["subject" "silent-claim-expiry"]
               ["body" "wake without another commit"]
               ["sent_at" "2026-07-19T00:00:00Z"]]]
        (assert-fact! port message predicate value))
      (let [claim
            (coordinator-op
             port
             {:op :acquire-lease
              :res (delivery-resource message "recipient")
              :holder "fixture-dead-consumer"
              :ttl-ms 500})]
        (check "fixture pre-acquires the exact production delivery lease"
               (and (:ok claim) (some? (:epoch claim)))))
      (assert-fact! port message "to" "recipient")
      (let [left-silent (future (read-frame! left 250))
            right-silent (future (read-frame! right 250))]
        (check "both armed feeds emit nothing while the foreign claim is live"
               (and (nil? @left-silent)
                    (nil? @right-silent))))
      (let [noise-deadline (+ (System/currentTimeMillis) 900)
            noise
            (future
              (loop [index 0]
                (when (< (System/currentTimeMillis) noise-deadline)
                  (assert-fact! port
                                (str "@noise:claim-expiry-" index)
                                "note" "unrelated coordinator traffic")
                  (recur (inc index)))))
            winner-promise (promise)
            read-one
            (fn [feed]
              (future
                (let [frame (read-frame! feed 1250)]
                  (when frame (deliver winner-promise [feed frame]))
                  frame)))
            left-read (read-one left)
            right-read (read-one right)
            winner (deref winner-promise 1500 nil)
            _ (when winner (ack! (first winner) message))
            left-frame @left-read
            right-frame @right-read
            _ @noise
            deliveries (keep identity [left-frame right-frame])]
        (check "two feeds retry nil claims on time under a commit storm and elect one output"
               (and winner
                    (= 1 (count deliveries))
                    (= message (mail-id (first deliveries)))))
        (check "two-feed claim-expiry replay becomes durable without another message write"
               (await-acked! port "recipient" [message])))
      (stop-feed! left)
      (stop-feed! right))

    ;; Terminal teardown is a generation-specific graph barrier, not an
    ;; in-process promise. More than one bounded page of producer-admitted
    ;; steers is accepted under one armed manifest, the route freezes, and the
    ;; real feed protocol rejects every steer before emitting `drained`. The
    ;; final message is held by a foreign claim until it expires; no subsequent
    ;; producer commit exists to wake it.
    (let [recipient "drain-recipient"
          armed-epoch "00000000-0000-4000-8000-000000000010"
          frozen-epoch "00000000-0000-4000-8000-000000000011"
          rearmed-epoch "00000000-0000-4000-8000-000000000012"
          wrong-epoch "00000000-0000-4000-8000-000000000013"
          ordinary
          (mapv #(format "@msg:drain-%03d" %) (range 257))
          poison (first ordinary)
          foreign "@msg:zz-foreign-claim"
          messages (conj ordinary foreign)]
      (publish-route! port recipient "publish" "armed" armed-epoch)
      (check "terminal-drain lane has a live session lease"
             (zero? (:exit (register! port recipient))))
      (let [manifest
            (first (values-of
                    port (str "@agent:" recipient)
                    "identity_manifest_sha256"))]
        (check "armed route exposes one producer admission manifest"
               (and (string? manifest)
                    (re-matches #"^[0-9a-f]{64}$" manifest)))
        (doseq [message messages]
          (doseq [[predicate value]
                  [["from" "director"]
                   ["subject" (if (= message poison) " Steer " "steer")]
                   ["body" (str "terminal drain payload " message)]
                   ["sent_at" "2026-07-19T00:00:00Z"]
                   ["target_identity_manifest_sha256" manifest]
                   ["to" recipient]]]
            (assert-fact! port message predicate value)))
        (check "fixture spans more than one bounded pending-steer page"
               (> (count messages)
                  north.message-audience/pending-page-limit))
        (publish-route! port recipient "route" "frozen" frozen-epoch)
        (let [claim
              (coordinator-op
               port
               {:op :acquire-lease
                :res (delivery-resource foreign recipient)
                :holder "fixture-terminal-dead-consumer"
                :ttl-ms 5000})]
          (check "terminal fixture holds the final steer with a foreign claim"
                 (and (:ok claim) (some? (:epoch claim)))))
        (let [feed
              (start-feed!
               port recipient 5000 1000
               "--settlement-only" "true")]
          (start! feed recipient)
          (send-control!
           feed (array-map "type" "drain" "epoch" frozen-epoch))
          (let [deadline (+ (System/currentTimeMillis) 30000)
                frames
                (loop [observed []]
                  (when (>= (System/currentTimeMillis) deadline)
                    (throw
                     (ex-info "terminal drain integration timed out"
                              {:observed (count observed)
                               :errors @(:errors feed)})))
                  (if-let [frame (read-frame! feed 2000)]
                    (let [next-observed (conj observed frame)]
                      (if (= "drained" (get frame "type"))
                        next-observed
                        (recur next-observed)))
                    (recur observed)))
                progress (filter #(= "drain_progress" (get % "type")) frames)
                receipt (last frames)]
            (check "real drain emits progress across the multi-page backlog"
                   (and (= (count messages) (count progress))
                        (every? #(= frozen-epoch (get % "epoch")) progress)))
            (check "drained receipt names the exact frozen route generation"
                   (and (= "drained" (get receipt "type"))
                        (= recipient (get receipt "recipient"))
                        (= frozen-epoch (get receipt "epoch")))))
          (check "every accepted steer is terminally rejected without false ack"
                 (every?
                  (fn [message]
                    (and (= #{recipient}
                            (values-of port message "delivery_rejected_by"))
                         (empty? (values-of port message "acked_by"))))
                  messages))
          (check "foreign-claim steer settles after expiry without a producer wake"
                 (= #{recipient}
                    (values-of port foreign "delivery_rejected_by")))
          (let [evidence
                (map json/parse-string
                     (values-of port poison "delivery_rejection"))]
            (check "noncanonical managed steer subject is poison, not acknowledged"
                   (and (some #(= "steer_type_invalid" (get % "reason"))
                              evidence)
                        (empty? (values-of port poison "acked_by")))))
          (check "generation receipt leaves zero producer-admitted steers pending"
                 (empty? (pending-steers port recipient)))
          (stop-feed! feed))

        ;; A well-formed but non-current epoch cannot mint a receipt even while
        ;; the route remains frozen.
        (let [wrong (start-feed!
                     port recipient 500 300
                     "--settlement-only" "true")]
          (start! wrong recipient)
          (send-control! wrong
                         (array-map "type" "drain" "epoch" wrong-epoch))
          (check "wrong frozen epoch terminates without a drained receipt"
                 (await-predicate
                  #(not (.isAlive ^Process (:process wrong)))))
          (check "wrong-epoch failure is explicit"
                 (some #(str/includes?
                         % "terminal-steer-drain-route-mismatch")
                       @(:errors wrong)))
          (stop-feed! wrong))

        ;; Re-arm changes the graph generation. Replaying the prior frozen epoch
        ;; into a fresh settlement process must likewise fail closed.
        (publish-route! port recipient "route" "armed" rearmed-epoch)
        (let [stale (start-feed!
                     port recipient 500 300
                     "--settlement-only" "true")]
          (start! stale recipient)
          (send-control! stale
                         (array-map "type" "drain" "epoch" frozen-epoch))
          (check "re-arm prevents a stale frozen-generation receipt"
                 (await-predicate
                  #(not (.isAlive ^Process (:process stale)))))
          (check "stale post-rearm drain reports route mismatch"
                 (some #(str/includes?
                         % "terminal-steer-drain-route-mismatch")
                       @(:errors stale)))
          (stop-feed! stale))))

    ;; One restart scenario exercises all three terminal barriers explicitly:
    ;; (1) freeze failure -> late steer remains admissible -> fresh settlement;
    ;; (2) drain failure -> pending remains and retry cannot inherit success;
    ;; (3) already-frozen + no original subscription -> a fresh settlement-only
    ;; process must either prove the exact epoch and settle, or fail closed.
    (let [recipient "retry-drain-recipient"
          armed-epoch "00000000-0000-4000-8000-000000000020"
          frozen-epoch "00000000-0000-4000-8000-000000000021"
          wrong-epoch "00000000-0000-4000-8000-000000000022"]
      (publish-route! port recipient "publish" "armed" armed-epoch)
      (check "retry-drain lane has a live session lease"
             (zero? (:exit (register! port recipient))))
      (let [premature
            (start-feed! port recipient 500 300
                         "--settlement-only" "true")]
        (start! premature recipient)
        (send-control! premature
                       (array-map "type" "drain" "epoch" frozen-epoch))
        (check "failed freeze attempt cannot emit a terminal settlement receipt"
               (await-predicate
                #(not (.isAlive ^Process (:process premature)))))
        (check "freeze failure leaves no buffered drained receipt"
               (not-any? #(= "drained" (get % "type"))
                         (frames-until-eof! premature 1000)))
        (check "failed freeze attempt reports the generation mismatch"
               (some #(str/includes?
                       % "terminal-steer-drain-route-mismatch")
                     @(:errors premature)))
        (stop-feed! premature))
      (let [admitted
            (run-msg port "send" "director" recipient "steer"
                     "accepted after the failed freeze attempt")
            message
            (second
             (re-find #"queued for live injection (@msg:[^ ]+)"
                      (:out admitted)))]
        (check "an armed route honestly accepts a late steer after freeze failure"
               (and (zero? (:exit admitted)) message))
        (publish-route! port recipient "route" "frozen" frozen-epoch)
        (let [failed
              (start-feed! port recipient 500 300
                           "--settlement-only" "true")]
          (start! failed recipient)
          (send-control! failed
                         (array-map "type" "drain" "epoch" wrong-epoch))
          (check "failed drain process cannot false-succeed or consume pending steer"
               (and (await-predicate
                       #(not (.isAlive ^Process (:process failed))))
                      (contains? (set (pending-steers port recipient)) message)))
          (check "drain failure emits no buffered terminal receipt"
                 (not-any? #(= "drained" (get % "type"))
                           (frames-until-eof! failed 1000)))
          (stop-feed! failed))
        (let [retry
              (start-feed! port recipient 1000 300
                           "--settlement-only" "true")]
          (start! retry recipient)
          (send-control! retry
                         (array-map "type" "drain" "epoch" frozen-epoch))
          (let [frames
                (loop [observed [] deadline (+ (System/currentTimeMillis) 5000)]
                  (when (>= (System/currentTimeMillis) deadline)
                    (throw
                     (ex-info "retry settlement timed out"
                              {:observed observed :errors @(:errors retry)})))
                  (if-let [frame (read-frame! retry 1000)]
                    (let [next-observed (conj observed frame)]
                      (if (= "drained" (get frame "type"))
                        next-observed
                        (recur next-observed deadline)))
                    (recur observed deadline)))
                receipt (last frames)]
            (check "fresh settlement process proves the exact current frozen epoch"
                   (and (= "drained" (get receipt "type"))
                        (= recipient (get receipt "recipient"))
                        (= frozen-epoch (get receipt "epoch"))))
            (check "retry settles the late steer without a false acknowledgement"
                   (and (= #{recipient}
                           (values-of port message "delivery_rejected_by"))
                        (empty? (values-of port message "acked_by"))
                        (empty? (pending-steers port recipient)))))
          (stop-feed! retry))))

    ;; Broadcast replay uses the same durable feed, but its authority comes from
    ;; the finite send-time audience snapshot.
    (let [message
          (send-message!
           port "sender" "*" "broadcast-before-arm" "finite replay")
          feed (start-feed! port "recipient" 3000 2000)]
      (start! feed "recipient")
      (let [mail (require-frame! feed "mail")]
        (check "pending finite broadcast is replayed"
               (= message (mail-id mail)))
        (ack! feed message)
        (check "broadcast acknowledgement is durable"
               (await-acked! port "recipient" [message])))
      (stop-feed! feed))

    (finally
      (doseq [feed @feeds]
        (stop-feed! feed))
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println
     (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println
   (format "\nlive feed integration: %d / %d PASS"
           passed
           (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
