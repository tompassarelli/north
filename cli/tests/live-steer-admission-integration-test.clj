#!/usr/bin/env bb
;; Fail-closed live-steer admission across the raw fact producer and public CLI.
;; A rejected steer must be a pure read: no partial @msg facts may reach the log.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (str (System/getProperty "user.home") "/code/fram")))
(def msg-cli (str root "/cli/msg-cli.clj"))
(def agents-cli (str root "/cli/agents-cli.clj"))
(def identity-writer (str root "/cli/agent-fact-internal.clj"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(def live-feed-cli (str root "/cli/north-live-feed.clj"))
(load-file (str root "/cli/message-contract.clj"))
(System/setProperty "north.live-feed.lib" "1")
(let [test-file (System/getProperty "babashka.file")]
  (System/setProperty "babashka.file" live-feed-cli)
  (try (load-file live-feed-cli)
       (finally (System/setProperty "babashka.file" test-file))))
(def checks (atom []))
(def test-log (atom nil))

(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))
(defn eventually [predicate]
  (loop [attempt 0]
    (cond
      (predicate) true
      (>= attempt 240) false
      :else (do (Thread/sleep 25) (recur (inc attempt))))))
(defn run-cli [path port & args]
  (apply proc/shell
         {:continue true :out :string :err :string
          :extra-env {"FRAM_LOG" @test-log
                      "NORTH_PORT" (str port)
                      "AGENT_TOPOLOGY" "orchestrator"
                      "NO_COLOR" "1"}}
         "bb" path args))
(defn publish! [port id provider live-input]
  (let [facts {"kind" "lane"
               "role" "integrator"
               "goal" "live steer admission probe"
               "provider" provider
               "provider_target" (str provider "-test")
               "live_input" live-input
               "live_input_state" (if (= "streaming" live-input) "armed" "frozen")
               "live_input_epoch" (if (= id "openai-unsupported")
                                    "00000000-0000-4000-8000-000000000001"
                                    (if (= id "anthropic-streaming")
                                      "00000000-0000-4000-8000-000000000002"
                                      "00000000-0000-4000-8000-000000000003"))
               "model" (if (= provider "anthropic") "claude-opus-4-8" "gpt-5.6-sol")
               "effort" "xhigh"
               "composition_kind" "preset"
               "composition_id" "integrator"
               "composition_overrides" "[]"
               "repo" "~/code/north"
               "spawned_at" "2026-07-19T00:00:00Z"
               "display_handle" (str provider "-test-integrator-" id)
               "display_name" (str provider " · integrator · " id)}
        result (run-cli identity-writer port (str port) "publish"
                        (str "agent:" id) (json/generate-string facts))]
    (when-not (zero? (:exit result))
      (throw (ex-info "identity fixture publication failed" result)))))
(defn register! [port id]
  (let [result (run-cli presence-cli port (str port) "register" id
                        (str "/tmp/" id) id)]
    (when-not (zero? (:exit result))
      (throw (ex-info "presence fixture publication failed" result)))))
(defn graph-message-ids [_port]
  (set (re-seq #"@msg:[A-Za-z0-9._:-]+" (slurp @test-log))))
(defn put-fact! [port subject predicate value]
  (let [result
        (north.coord/send-op-for-log
         port @test-log {:op :assert :te subject :p predicate :r value})]
    (when (:reject result)
      (throw (ex-info "fixture fact write failed" result)))
    result))
(defn fact-one [port subject predicate]
  (first
   (:values
    (north.coord/send-op-for-log
     port @test-log {:op :resolved :te subject :p predicate}))))
(defn fact-values [port subject predicate]
  (set
   (:values
    (north.coord/send-op-for-log
     port @test-log {:op :resolved :te subject :p predicate}))))
(defmacro with-test-coordinator [& body]
  `(with-redefs
     [north.coord/send-op
      (fn [port# operation#]
        (north.coord/send-op-for-log port# @test-log operation#))
      emit-error! (fn [& _#] nil)]
     ~@body))
(defn route! [port id state epoch]
  (let [facts {"provider" "anthropic"
               "provider_target" "anthropic-test"
               "live_input" "streaming"
               "live_input_state" state
               "live_input_epoch" epoch
               "model" "claude-opus-4-8"
               "effort" "xhigh"
               "display_handle" (str "anthropic-test-integrator-" id)
               "display_name" (str "anthropic · integrator · " id)}
        result (run-cli identity-writer port (str port) "route"
                        (str "agent:" id) (json/generate-string facts))]
    (when-not (zero? (:exit result))
      (throw (ex-info "identity fixture route update failed" result)))))

(let [port (free-port)
      tmp (.toFile (java.nio.file.Files/createTempDirectory
                    "north-live-steer" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "facts.log")
      daemon (do
               (spit log "")
               (proc/process
                {:dir fram :out :string :err :string
                 :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
                "bb" "-cp" "out" "coord_daemon.clj"
                "serve-flat" (str port) (.getPath log)))]
  (reset! test-log (.getCanonicalPath log))
  (try
    (check "throwaway coordinator starts" (eventually #(port-open? port)))

    (let [before (graph-message-ids port)
          invalid-cases
          [["blank sender" "" "recipient" "status" "body"]
           ["malformed sender" "bad sender" "recipient" "status" "body"]
           ["oversized sender" (apply str (repeat 1025 "s")) "recipient" "status" "body"]
           ["blank target" "sender" "" "status" "body"]
           ["malformed target" "sender" "../recipient" "status" "body"]
           ["oversized target" "sender" (apply str (repeat 513 "t")) "status" "body"]
           ["blank subject" "sender" "recipient" " " "body"]
           ["malformed subject" "sender" "recipient" (str "bad" \u0001 "subject") "body"]
           ["oversized subject" "sender" "recipient"
            (apply str (repeat (inc north.message-contract/max-subject-bytes) "s"))
            "body"]
           ["blank body" "sender" "recipient" "status" " "]]]
      (doseq [[label from to subject body] invalid-cases]
        (let [result (run-cli msg-cli port (str port) "send"
                              from to subject body)]
          (check (str label " rejects before the first graph write")
                 (and (= 2 (:exit result))
                      (str/includes? (:err result) "REJECTED: message")
                      (= before (graph-message-ids port))))))
      (check "oversized body is rejected by the same pure producer contract"
             (= "body is blank, malformed, or too large"
                (north.message-contract/input-problem
                 "sender" "recipient" "status"
                 (apply str
                        (repeat
                         (inc north.message-contract/max-body-bytes)
                         "b"))))))

    (publish! port "openai-unsupported" "openai" "unsupported")
    (register! port "openai-unsupported")
    (publish! port "anthropic-streaming" "anthropic" "streaming")
    (register! port "anthropic-streaming")
    (publish! port "anthropic-offline" "anthropic" "streaming")

    (let [before (graph-message-ids port)
          raw (run-cli msg-cli port (str port) "send" "director"
                       "openai-unsupported" "steer" "must not land")
          after-raw (graph-message-ids port)
          public (run-cli agents-cli port "steer"
                          "openai-unsupported" "must still not land")
          after-public (graph-message-ids port)]
      (check "raw OpenAI steer rejects with a stable nonzero status and stderr"
             (and (= 2 (:exit raw))
                  (str/blank? (:out raw))
                  (str/includes? (:err raw)
                                 "target adapter does not support live input (provider openai)")))
      (check "raw OpenAI rejection creates zero message facts"
             (= before after-raw))
      (check "public north steer propagates the exact rejection status and diagnostic"
             (and (= 2 (:exit public))
                  (str/includes? (:out public) "msg-cli.clj")
                  (str/includes? (:err public)
                                 "target adapter does not support live input (provider openai)")))
      (check "public OpenAI rejection also creates zero message facts"
             (= after-raw after-public)))

    (let [before (graph-message-ids port)
          offline (run-cli msg-cli port (str port) "send" "director"
                           "anthropic-offline" "steer" "must not land")]
      (check "stream-capable but offline lane rejects before publication"
             (and (= 2 (:exit offline))
                  (str/includes? (:err offline) "target is offline")
                  (= before (graph-message-ids port)))))

    (let [sent (run-cli msg-cli port (str port) "send" "director"
                        "anthropic-streaming" "steer" "continue with proof")
          message (second (re-find #"queued for live injection (@msg:[^ ]+)"
                                   (:out sent)))
          ids (graph-message-ids port)
          admitted-marker
          (fact-one port "@agent:anthropic-streaming"
                    "identity_manifest_sha256")]
      (check "active streaming Anthropic lane accepts one live-injection message"
             (and (zero? (:exit sent))
                  (str/includes? (:out sent) "queued for live injection @msg:")
                  (= 1 (count ids))))
      (check "accepted steer carries the exact committed route manifest"
             (= admitted-marker
                (fact-one port message target-identity-manifest-predicate)))
      (check "accepted steer route is valid before the freeze"
             (with-test-coordinator
               (current-steer-route?
                port message "anthropic-streaming" "steer")))
      (route! port "anthropic-streaming" "frozen"
              "00000000-0000-4000-8000-000000000004")
      (check "freeze after producer admission invalidates the queued steer"
             (not (current-steer-route?
                   port message "anthropic-streaming" "steer")))
      (route! port "anthropic-streaming" "armed"
              "00000000-0000-4000-8000-000000000005")
      (check "re-arm with a new route epoch cannot resurrect stale steer mail"
             (with-test-coordinator
               (not (current-steer-route?
                     port message "anthropic-streaming" "steer"))))
      (let [settlement
            (with-test-coordinator
              (deliver-message!
               port "anthropic-streaming" message
               (java.util.concurrent.LinkedBlockingQueue.)
               30000 1000))
            evidence
            (some-> (first (fact-values port message "delivery_rejection"))
                    json/parse-string)]
        (check "stale epoch replay terminally rejects without false acknowledgement"
               (and (= :rejected settlement)
                    (empty? (fact-values port message "acked_by"))
                    (= #{"anthropic-streaming"}
                       (fact-values port message "delivery_rejected_by"))))
        (check "stale epoch rejection is inspectable and tied to both manifests"
               (and (= "steer_route_stale" (get evidence "reason"))
                    (= admitted-marker (get evidence "expectedManifest"))
                    (= (fact-one port "@agent:anthropic-streaming"
                                 "identity_manifest_sha256")
                       (get evidence "observedManifest"))))
        (check "terminally rejected stale steer is absent from pending replay"
               (with-test-coordinator
                 (not (contains?
                       (set
                        (north.message-audience/pending-message-ids
                         port "anthropic-streaming"
                         #{"anthropic-streaming"}))
                       message)))))

      (let [manual "@msg:20260719-000000-00000000-0000-4000-8000-000000000099"
            marker
            (fact-one port "@agent:anthropic-streaming"
                      "identity_manifest_sha256")]
        ;; Deterministic check -> content -> freeze -> `to` schedule. The `to`
        ;; commit wakes the feed only after the old route generation is gone.
        (doseq [[predicate value]
                [["from" "director"]
                 ["subject" "steer"]
                 ["body" "must remain stale"]
                 ["sent_at" "2026-07-19T00:00:00Z"]
                 [target-identity-manifest-predicate marker]]]
          (put-fact! port manual predicate value))
        (route! port "anthropic-streaming" "frozen"
                "00000000-0000-4000-8000-000000000006")
        (put-fact! port manual "to" "anthropic-streaming")
        (check "check→freeze→to race leaves a durable but non-deliverable steer"
               (and (= "anthropic-streaming" (fact-one port manual "to"))
                    (with-test-coordinator
                      (not (current-steer-route?
                            port manual "anthropic-streaming" "steer")))))
        (check "check→freeze→to race settles as an honest rejection"
               (= :rejected
                  (with-test-coordinator
                    (deliver-message!
                     port "anthropic-streaming" manual
                     (java.util.concurrent.LinkedBlockingQueue.)
                     30000 1000))))))

    (let [recipient
          (apply str
                 (repeat
                  north.message-audience/max-rejection-recipient-bytes
                  "r"))
          message "@msg:20260719-000000-00000000-0000-4000-8000-000000000100"
          expected (apply str (repeat north.message-audience/manifest-sha256-bytes "a"))
          observed (apply str (repeat north.message-audience/manifest-sha256-bytes "b"))]
      (put-fact! port message "to" recipient)
      (let [settled
            (with-test-coordinator
              (when-let [claim
                         (north.message-audience/claim-delivery!
                          port message recipient)]
                (north.message-audience/reject-delivery!
                 port message recipient claim
                 {:reason "steer_route_stale"
                  :expected-manifest expected
                  :observed-manifest observed})))
            evidence (first (fact-values port message "delivery_rejection"))]
        (check "max-valid recipient with both stale-route manifests terminally rejects"
               (and (true? settled)
                    (= #{recipient}
                       (fact-values port message "delivery_rejected_by"))
                    (empty? (fact-values port message "acked_by"))))
        (check "max-valid stale-route evidence stays inside its derived byte bound"
               (and evidence
                    (<= (north.message-audience/utf8-bytes evidence)
                        north.message-audience/max-rejection-evidence-bytes)))
        (check "max-valid stale-route rejection cannot remain pending"
               (with-test-coordinator
                 (not (contains?
                       (set
                        (north.message-audience/pending-message-ids
                         port recipient #{recipient}))
                       message))))))

    (let [poison-ids
          (mapv
           (fn [index]
             (let [message
                   (format
                    "@msg:20260719-000001-00000000-0000-4000-8000-%012d"
                    index)]
               ;; Canonical producers cannot create this shape. The live feed
               ;; must still quarantine externally inserted poison instead of
               ;; replaying it until max-pending-replay disables the feed.
               (put-fact! port message "subject" "status")
               (put-fact! port message "body" "missing sender")
               (put-fact! port message "to" "anthropic-streaming")
               message))
           (range 1 33))]
      (doseq [message poison-ids]
        (check (str "poison frame " message " terminally rejects")
               (= :rejected
                  (with-test-coordinator
                    (deliver-message!
                     port "anthropic-streaming" message
                     (java.util.concurrent.LinkedBlockingQueue.)
                     30000 1000)))))
      (check "poison batch cannot consume the pending replay bound"
             (with-test-coordinator
               (empty?
                (set/intersection
                 (set poison-ids)
                 (set
                  (north.message-audience/pending-message-ids
                   port "anthropic-streaming"
                   #{"anthropic-streaming"}))))))
      (check "poison rejection remains distinct from successful delivery"
             (every?
              (fn [message]
                (and (empty? (fact-values port message "acked_by"))
                     (= #{"anthropic-streaming"}
                        (fact-values port message "delivery_rejected_by"))
                     (some #(str/includes? % "\"reason\":\"missing_sender\"")
                           (fact-values port message "delivery_rejection"))))
              poison-ids)))

    (let [before (graph-message-ids port)
          broadcast-steer
          (run-cli msg-cli port (str port) "send" "director" "*" "steer"
                   "broadcast steer must not land")]
      (check "steer never admits the ordinary broadcast address"
             (and (= 2 (:exit broadcast-steer))
                  (str/includes? (:err broadcast-steer)
                                 "steer target is malformed")
                  (= before (graph-message-ids port)))))
    (finally
      (try (proc/destroy-tree daemon) (catch Exception _ nil))
      (try @daemon (catch Exception _ nil))
      (doseq [child (.listFiles tmp)]
        (when child (.delete child)))
      (.delete tmp))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nlive steer admission: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
