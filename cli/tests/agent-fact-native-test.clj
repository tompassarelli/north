#!/usr/bin/env bb
(require '[clojure.test :refer [deftest is run-tests]])

(def root (.getCanonicalPath
           (java.io.File.
            (.getParent (java.io.File. (System/getProperty "babashka.file")))
            "../..")))
(load-file (str root "/cli/agent-fact-internal.clj"))

(def subject "@agent:native-writer-unit")
(def operation-id "00000000-0000-4000-8000-000000000301")
(def holder "managed-agent-writer:00000000-0000-4000-8000-000000000201")
(def preset
  {"kind" "lane" "role" "integrator" "model" "gpt-5.6-sol"
   "provider" "openai" "provider_target" "codex-a" "effort" "high"
   "live_input" "turn-framed" "live_input_state" "frozen"
   "live_input_epoch" "00000000-0000-4000-8000-000000000101"
   "composition_kind" "preset" "composition_id" "integrator"
   "composition_overrides" "[]" "repo" "north"
   "goal" "native writer probe" "spawned_at" "2026-08-04T00:00:00Z"
   "display_handle" "native-writer-probe"
   "display_name" "native writer probe"})

(defn fake-native
  [{:keys [projections batch-outcomes readbacks]}]
  (let [projections (atom (vec projections))
        batches (atom (vec batch-outcomes))
        readbacks (atom (vec readbacks))
        calls (atom [])
        ranks (atom nil)
        pop-first! (fn [values fallback]
                     (let [value (or (first @values) fallback)]
                       (swap! values #(if (seq %) (subvec % 1) %))
                       value))
        invoke
        (fn [operation & args]
          (swap! calls conj operation)
          (case operation
            version! {:served-version 10}
            lease-acquire-at-version! {:outcome :applied :fence :test-fence}
            subject-projection!
            {:served-version 12
             :occurrences (pop-first! projections {})}
            plan-subject-actions
            (let [options (nth args 3)
                  rank (:rank options)]
              (reset! ranks [(rank {:proposition "body"})
                             (rank {:proposition marker-predicate})])
              [{:op :rpc/assert :proposition "planned"}])
            fenced-batch! (pop-first! batches {:outcome :applied})
            subject-readback!
            (pop-first! readbacks {:state :committed})
            lease-release! {:released? true}
            lease-check! {:valid? true}
            (throw (ex-info "unexpected fake native operation"
                            {:operation operation :args args}))))]
    {:invoke invoke :calls calls :ranks ranks}))

(defn publish-with [fake desired expected]
  (with-redefs [native-rpc! (:invoke fake)
                native-triple-predicate identity]
    (native-publish-identity! :client subject "publish" operation-id
                              desired desired expected holder)))

(deftest fresh-publish-is-one-marker-last-batch
  (let [fake (fake-native {:projections [{}]
                           :batch-outcomes [{:outcome :applied}]
                           :readbacks [{:state :committed}]})
        result (publish-with fake preset nil)]
    (is (= {:status "committed" :operation_id operation-id} result))
    (is (= [0 1] @(:ranks fake)) "identity marker ranks after body assertions")
    (is (= 1 (count (filter #{'fenced-batch!} @(:calls fake)))))
    (is (= 1 (count (filter #{'subject-projection!} @(:calls fake)))))
    (is (= 'lease-release! (last @(:calls fake))))))

(deftest exact-replay-is-a-no-op
  (let [fake (fake-native {:projections [(identity-occurrences preset)]})
        result (publish-with fake preset nil)]
    (is (= {:status "committed" :operation_id operation-id
            :reason "exact_replay"} result))
    (is (not-any? #{'fenced-batch!} @(:calls fake)))
    (is (= 'lease-release! (last @(:calls fake))))))

(deftest killed-prefix-repair-is-one-batch
  (let [desired (assoc preset "goal" "replacement")
        prefix {"kind" {"lane" 1}
                "goal" {"replacement" 1}
                "provider" {"openai" 1}}
        fake (fake-native {:projections [prefix]
                           :batch-outcomes [{:outcome :applied}]
                           :readbacks [{:state :committed}]})
        result (publish-with fake desired preset)]
    (is (= "committed" (:status result)))
    (is (= "recovered_killed_prefix" (:reason result)))
    (is (= 1 (count (filter #{'fenced-batch!} @(:calls fake)))))))

(deftest conflict-rescans-replans-and-commits
  (let [fake (fake-native {:projections [{} {}]
                           :batch-outcomes [{:outcome :conflict}
                                            {:outcome :applied}]
                           :readbacks [{:state :committed}]})
        result (publish-with fake preset nil)]
    (is (= "committed" (:status result)))
    (is (= 2 (count (filter #{'subject-projection!} @(:calls fake)))))
    (is (= 2 (count (filter #{'fenced-batch!} @(:calls fake)))))))

(deftest sent-ambiguous-readback-proves-commit
  (let [fake (fake-native {:projections [{}]
                           :batch-outcomes [{:outcome :sent-ambiguous}]
                           :readbacks [{:state :committed}]})
        result (publish-with fake preset nil)]
    (is (= "committed" (:status result)))
    (is (= 1 (count (filter #{'subject-readback!} @(:calls fake)))))))

(deftest durability-ambiguous-requires-restart-without-readback
  (let [fake (fake-native {:projections [{}]
                           :batch-outcomes [{:outcome :durability-ambiguous}]})
        result (publish-with fake preset nil)]
    (is (= "indeterminate" (:status result)))
    (is (= "restart_required_durability_ambiguous" (:reason result)))
    (is (not-any? #{'subject-readback!} @(:calls fake)))
    (is (= 'lease-release! (last @(:calls fake))))))

(deftest route-recovery-uses-the-same-single-batch-profile
  (let [route {"provider" "anthropic" "provider_target" "claude-a"
               "live_input" "streaming" "live_input_state" "armed"
               "live_input_epoch" "00000000-0000-4000-8000-000000000102"
               "model" "claude-opus-4-8" "effort" "high"
               "display_handle" "native-route-probe"
               "display_name" "native route probe"}
        desired (merge preset route)
        fake (fake-native {:projections [(identity-occurrences preset)]
                           :batch-outcomes [{:outcome :applied}]
                           :readbacks [{:state :committed}]})
        result
        (with-redefs [native-rpc! (:invoke fake)
                      native-triple-predicate identity]
          (native-publish-identity! :client subject "route" operation-id
                                    route desired preset holder))]
    (is (= "committed" (:status result)))
    (is (= 1 (count (filter #{'fenced-batch!} @(:calls fake)))))
    (is (= [0 1] @(:ranks fake)))))

(let [{:keys [fail error]} (run-tests)]
  (when (pos? (+ fail error)) (System/exit 1)))
