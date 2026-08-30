#!/usr/bin/env bb
(require '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/wake-receipt-internal.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(def wake-ns (find-ns 'north.wake-receipt-internal))
(defn wake-var [name] (or (ns-resolve wake-ns name) (throw (ex-info (str name) {}))))
(defn coord-var [name] (or (ns-resolve 'north.coord name) (throw (ex-info (str name) {}))))
(defn wake-call [name & args] (apply (wake-var name) args))
(defn ledger-call [name & args]
  (apply (or (ns-resolve 'north.run-ledger name) (throw (ex-info (str name) {}))) args))

(def message "@msg:wake-fixture")
(def attempt (str "wake:" (apply str (repeat 64 "a"))))
(def target "wake-target")
(def epoch "11111111-1111-4111-8111-111111111111")
(def manifest (apply str (repeat 64 "b")))
(def port 7977)
(def at "2026-08-29T16:00:00.000Z")
(def message-admission-version 50)
(def occurrence-key ::occurrences)

(defn add-fact! [store subject predicate value]
  (swap! store update-in [subject predicate] (fnil conj #{}) value))

(defn add-occurrence!
  [store subject predicate value version ordinal]
  (swap! store update occurrence-key (fnil conj [])
         {:subject subject :predicate predicate :value value
          :operation :assert :version version :ordinal ordinal}))

(defn base-store []
  (let [store (atom {})]
    (doseq [[subject predicate value]
            [[message "wake_attempt_id" attempt]
             [message "to" target]
             [message "wake_listener_epoch" epoch]
             [message "wake_listener_manifest_sha256" manifest]
             [message "target_identity_manifest_sha256" manifest]
             [(str "@agent:" target) "live_input" "turn-messages"]
             [(str "@agent:" target) "live_input_state" "armed"]
             [(str "@agent:" target) "live_input_epoch" epoch]
             [(str "@agent:" target) "identity_manifest_sha256" manifest]]]
      (add-fact! store subject predicate value))
    (add-occurrence! store message "to" target message-admission-version 0)
    store))

(defn add-event!
  [store {:keys [id run sequence kind model-call role agent commit-version]
          :or {agent target}}]
  (let [event (cond-> (sorted-map
                       "version" "north:wire:v2"
                       "requiredSemantics" ["north.event-order.v1"]
                       "id" id
                       "runId" run
                       "sequence" sequence
                       "at" at
                       "kind" kind
                       "essential" true)
                model-call (assoc "modelCallId" model-call)
                role (assoc "role" role "stage" "started" "messageId" (str "message:" sequence))
                (= kind "model-call.started")
                (assoc "model" {"provider" "openai"} "attempt" 1)
                (= kind "model-call.completed")
                (assoc "status" "succeeded" "origin" "provider"
                       "usage" {"lifetime" {"inputTokens" 0 "outputTokens" 0
                                             "cacheReadTokens" 0 "cacheWriteTokens" 0
                                             "reasoningTokens" 0}
                                "currentContext" {"inputTokens" 0 "cacheReadTokens" 0}}
                       "usageCoverage" "exact")
                (= kind "tool.admitted")
                (assoc "toolCallId" (str "tool-call:" sequence)
                       "name" "fixture"
                       "schema" {"kind" "unknown"}))
        raw (json/generate-string event)
        subject (ledger-call 'event-subject run sequence)
        facts [["kind" "wire_event"]
               ["wire_ledger_version" north.run-ledger/version]
               ["wire_version" north.run-ledger/wire-version]
               ["wire_run_id" run]
               ["thread" "@thread:wake-fixture"]
               ["agent" agent]
               ["wire_event_id" id]
               ["wire_event_sequence" (str sequence)]
               ["wire_event_at" at]
               ["wire_event_kind" kind]
               ["wire_event_essential" "true"]
               ["wire_event_json" raw]
               ["wire_event_sha256" (ledger-call 'sha256 raw)]]]
    (doseq [[predicate value] facts]
      (add-fact! store subject predicate value))
    (add-occurrence! store subject "wire_event_id" id
                     (or commit-version (* 10 sequence)) 6)
    subject))

(defn envelope [store subject predicate]
  (let [values (vec (sort (get-in @store [subject predicate] #{})))]
    {:members (count values)
     :ambiguous? (> (count values) 1)
     :values values
     :value (when (= 1 (count values)) (first values))}))

(defn query-rows [store query]
  (case (:find query)
    "wake_wire_event_subject"
    (let [event-id (get-in query [:rules 0 :body 0 :args 2])]
      (->> @store
           (keep (fn [[subject facts]]
                   (when (contains? (get facts "wire_event_id" #{}) event-id)
                     [subject])))
           vec))

    "wake_wire_event_fact"
    (let [subject (get-in query [:rules 0 :body 0 :args 0])]
      (->> (get @store subject {})
           (mapcat (fn [[predicate values]]
                     (map (fn [value] [predicate value]) values)))
           vec))

    "wake_wire_run_subject"
    (let [run (get-in query [:rules 0 :body 0 :args 2])
          agent (get-in query [:rules 0 :body 1 :args 2])]
      (->> @store
           (keep (fn [[subject facts]]
                   (when (and (contains? (get facts "wire_run_id" #{}) run)
                              (contains? (get facts "agent" #{}) agent))
                     [subject])))
           vec))

    (throw (ex-info "unexpected fixture query" {:query query}))))

(defn proposition-occurrences [store subject predicate value]
  (->> (get @store occurrence-key [])
       (filter #(and (= subject (:subject %))
                     (= predicate (:predicate %))
                     (= value (:value %))))
       (mapv #(select-keys % [:operation :version :ordinal]))))

(defn publish! [store reject? operations]
  (if @reject?
    {:reject "fixture publication failure"}
    (let [conflict
          (some (fn [{:keys [subject predicate value cardinality]}]
                  (let [values (get-in @store [subject predicate] #{})]
                    (when (and (= :one cardinality)
                               (seq values)
                               (not= values #{value}))
                      predicate)))
                operations)]
      (if conflict
        {:reject (str "conflicting " conflict)}
        (do
          (doseq [{:keys [subject predicate value]} operations]
            (add-fact! store subject predicate value))
          {:ok true})))))

(defn fake-coord [store reject? operation args]
  (case operation
    "resolved-envelope!" (let [[_ subject predicate] args]
                           (envelope store subject predicate))
    "query-rows!" (query-rows store (second args))
    "proposition-occurrences!"
    (let [[_ subject predicate value] args]
      (proposition-occurrences store subject predicate value))
    "publish!" (publish! store reject? (second args))
    (throw (ex-info "unexpected fixture operation" {:operation operation}))))

(defn with-store [store reject? f]
  (with-redefs-fn
    {(wake-var 'coord-invoke!)
     (fn [operation args] (fake-coord store reject? operation args))}
    f))

(defn throws? [f]
  (try (f) false (catch Exception _ true)))

(defn context! [] (wake-call 'current-message-context! port message attempt target epoch))
(defn idle! [event] (wake-call 'idle-phase! port (context!) event))
(defn turn! [event] (wake-call 'turn-phase! port (context!) event))
(defn action! [event kind] (wake-call 'action-phase! port (context!) event kind))

(let [calls (atom [])
      expected-envelope {:members 1 :ambiguous? false :values ["value"] :value "value"}
      expected-rows [["row"]]
      expected-occurrence {:operation :assert :version 7 :ordinal 2}]
  (with-redefs-fn
    {(coord-var 'resolved-envelope!)
     (fn [& args]
       (swap! calls conj [:resolved-envelope! args])
       expected-envelope)
     (coord-var 'query-rows!)
     (fn [& args]
       (swap! calls conj [:query-rows! args])
       expected-rows)
     (coord-var 'proposition-occurrences!)
     (fn [& args]
       (swap! calls conj [:proposition-occurrences! args])
       [expected-occurrence])}
    (fn []
      (check
       "wake facade dynamically resolves the effectful coordination authority"
       (try
         (and (= expected-envelope
                 (wake-call 'envelope! port "@subject:dispatch" "predicate"))
              (= expected-rows
                 (wake-call 'query-rows! port {:find "dispatch"}))
              (= expected-occurrence
                 (wake-call 'exact-assertion-boundary!
                            port "@subject:dispatch" "predicate" "value"))
              (= [[:resolved-envelope! [port "@subject:dispatch" "predicate"]]
                  [:query-rows! [port {:find "dispatch"}]]
                  [:proposition-occurrences!
                   [port "@subject:dispatch" "predicate" "value"]]]
                 @calls))
         (catch Exception _ false))))))

(let [store (base-store)
      reject? (atom false)
      run "run:wake-happy"]
  (add-event! store {:id "event:idle" :run run :sequence 4
                     :kind "model-call.completed" :model-call "model-call:old"})
  (add-event! store {:id "event:turn" :run run :sequence 5
                     :kind "model-call.started" :model-call "model-call:wake"})
  (add-event! store {:id "event:action" :run run :sequence 6
                     :kind "message.recorded" :model-call "model-call:wake"
                     :role "assistant"})
  (with-store store reject?
    (fn []
      (check "idle intent commits exactly once" (= :created (idle! "event:idle")))
      (check "idle intent binds the exact message admission occurrence"
             (and (= #{(str message-admission-version)}
                     (get-in @store [message "wake_message_admission_version"]))
                  (= #{"0"}
                     (get-in @store [message "wake_message_admission_ordinal"]))
                  (= #{"40"}
                     (get-in @store [message "wake_idle_commit_version"]))))
      (check "post-intent pre-acceptance duplicate remains unknown"
             (= :unknown (idle! "event:idle")))
      (check "turn binds the distinct first later model call"
             (= :created (turn! "event:turn")))
      (check "post-acceptance duplicate joins without replay"
             (= :accepted (idle! "event:idle")))
      (check "first assistant action binds the accepted model call"
             (= :created (action! "event:action" "assistant.message.recorded")))
      (check "complete status revalidates exact durable Wire ancestry"
             (= "complete" (:status (wake-call 'wake-status! port message)))))))

(let [store (base-store)
      reject? (atom false)
      run "run:older-idle"]
  (add-event! store {:id "event:older-idle" :run run :sequence 1
                     :kind "model-call.completed" :model-call "model-call:older"})
  (add-event! store {:id "event:unrelated-start" :run run :sequence 2
                     :kind "model-call.started" :model-call "model-call:unrelated"})
  (add-event! store {:id "event:unrelated-complete" :run run :sequence 3
                     :kind "model-call.completed" :model-call "model-call:unrelated"})
  (with-store store reject?
    #(check "an older completion cannot skip a model call wholly before message admission"
            (throws? (fn [] (idle! "event:older-idle"))))))

(let [store (base-store)
      reject? (atom false)
      run "run:message-boundary"]
  (add-event! store {:id "event:boundary-idle" :run run :sequence 1
                     :kind "model-call.completed" :model-call "model-call:idle"})
  (add-event! store {:id "event:pre-message-start" :run run :sequence 2
                     :kind "model-call.started" :model-call "model-call:pre"
                     :commit-version 40})
  (add-event! store {:id "event:post-message-start" :run run :sequence 3
                     :kind "model-call.started" :model-call "model-call:post"
                     :commit-version 60})
  (with-store store reject?
    (fn []
      (idle! "event:boundary-idle")
      (check "a model call committed before message admission cannot accept the wake"
             (throws? (fn [] (turn! "event:pre-message-start"))))
      (check "the first distinct model call at or after message admission is accepted"
             (= :created (turn! "event:post-message-start"))))))

(let [store (base-store)
      reject? (atom false)]
  (add-event! store {:id "event:ambiguous" :run "run:one" :sequence 2
                     :kind "model-call.completed" :model-call "model-call:one"})
  (add-event! store {:id "event:ambiguous" :run "run:two" :sequence 2
                     :kind "model-call.completed" :model-call "model-call:two"})
  (with-store store reject?
    #(check "ambiguous Wire event IDs fail closed" (throws? (fn [] (idle! "event:ambiguous"))))))

(let [store (base-store)
      reject? (atom false)
      run "run:negative"]
  (add-event! store {:id "event:idle-negative" :run run :sequence 4
                     :kind "model-call.completed" :model-call "model-call:reused"})
  (add-event! store {:id "event:turn-reused" :run run :sequence 5
                     :kind "model-call.started" :model-call "model-call:reused"})
  (add-event! store {:id "event:turn-first" :run run :sequence 6
                     :kind "model-call.started" :model-call "model-call:first"})
  (add-event! store {:id "event:turn-later" :run run :sequence 7
                     :kind "model-call.started" :model-call "model-call:later"})
  (with-store store reject?
    (fn []
      (idle! "event:idle-negative")
      (check "reused idle model-call identity is rejected"
             (throws? (fn [] (turn! "event:turn-reused"))))
      (check "a later model call cannot replace the first candidate"
             (throws? (fn [] (turn! "event:turn-later")))))))

(let [store (base-store)
      reject? (atom false)
      run "run:action-negative"]
  (add-event! store {:id "event:idle-action" :run run :sequence 4
                     :kind "model-call.completed" :model-call "model-call:old-action"})
  (add-event! store {:id "event:turn-action" :run run :sequence 5
                     :kind "model-call.started" :model-call "model-call:wake-action"})
  (add-event! store {:id "event:first-action" :run run :sequence 6
                     :kind "message.recorded" :model-call "model-call:wake-action"
                     :role "assistant"})
  (add-event! store {:id "event:later-action" :run run :sequence 7
                     :kind "tool.admitted" :model-call "model-call:wake-action"})
  (with-store store reject?
    (fn []
      (idle! "event:idle-action")
      (turn! "event:turn-action")
      (check "a later action cannot replace the exact first assistant/tool event"
             (throws? (fn [] (action! "event:later-action" "tool.admitted")))))))

(let [store (base-store)
      reject? (atom true)]
  (add-event! store {:id "event:publication-failure" :run "run:publication-failure"
                     :sequence 3 :kind "model-call.completed"
                     :model-call "model-call:publication-failure"})
  (with-store store reject?
    (fn []
      (check "publication failure leaves no intent receipt"
             (throws? (fn [] (idle! "event:publication-failure"))))
      (check "failed publication has no false durable idle fact"
             (empty? (get-in @store [message "wake_idle_event"] #{}))))))

(doseq [[label mutate]
        [["stale target epoch is rejected"
          (fn [store]
            (swap! store assoc-in [(str "@agent:" target) "live_input_epoch"]
                   #{"22222222-2222-4222-8222-222222222222"}))]
         ["ambiguous target epoch is rejected"
          (fn [store]
            (add-fact! store (str "@agent:" target) "live_input_epoch"
                       "22222222-2222-4222-8222-222222222222"))]
         ["expired target route is rejected"
          (fn [store]
            (swap! store assoc-in [(str "@agent:" target) "live_input_state"] #{"frozen"}))]]]
  (let [store (base-store)]
    (mutate store)
    (with-store store (atom false)
      #(check label (throws? context!)))))

(let [store (base-store)
      reject? (atom false)]
  (add-event! store {:id "event:wrong-target" :run "run:wrong-target" :sequence 2
                     :kind "model-call.completed" :model-call "model-call:wrong"
                     :agent "another-target"})
  (with-store store reject?
    #(check "unrelated target event is rejected"
            (throws? (fn [] (idle! "event:wrong-target"))))))

(let [failed (remove second @checks)]
  (doseq [[label _] failed] (println "FAIL:" label))
  (println (str "wake receipt internal: " (- (count @checks) (count failed))
                "/" (count @checks) " passed"))
  (when (seq failed) (System/exit 1)))
