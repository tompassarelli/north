;; orchestration-project-strictness-test.clj — proves the catalog PROJECTOR's
;; strict query envelopes and named-field guards (thread 019f9303).
;;
;; The regression this locks down: a rules query that timed out
;; (query-time-limit) came back WITHOUT an :ok vector, the old `(:ok resp)`
;; silently became nil -> an empty result set -> a downstream `(parse-long nil)`
;; NPE that named neither the failure nor the model. Now:
;;   - a non-:ok envelope THROWS :catalog-projection-query-failed carrying the
;;     original coordinator error, never collapsing to empty;
;;   - a graph row missing a required scalar THROWS :catalog-projection-missing-
;;     field naming the exact subject/model + field (never an NPE);
;;   - the happy path still returns rows unchanged.
;;
;; Daemon-free: load the projector as a library (main-guarded) and stub its
;; canonical named coordination projections.
;;   bb cli/tests/orchestration-project-strictness-test.clj
(require '[clojure.java.io :as io])

(def cli-dir (.getParentFile (io/file (System/getProperty "babashka.file"))))
(def projector (str (io/file (.getParentFile cli-dir) "orchestration-project-cli.clj")))
(load-file projector)

(def results (atom []))
(defn check [label pass?]
  (swap! results conj (boolean pass?))
  (println (format "  %s %s" (if pass? "✓" "✗") label)))

(defn ex-type [f]
  (try (f) ::no-throw
       (catch clojure.lang.ExceptionInfo e (:type (ex-data e)))))
(defn ex-message [f]
  (try (f) nil (catch clojure.lang.ExceptionInfo e (.getMessage e))))

(println "orchestration projector strictness — daemon-free stubs")

;; --- A. named-field guard identifies subject + field -------------------------
(let [msg (ex-message #(long! {"context_window_from" ["2026-07-16"]}
                              "context_window_tokens" "anthropic:claude-opus-4-8"))]
  (check "long! on a missing field throws :catalog-projection-missing-field"
         (= :catalog-projection-missing-field
            (ex-type #(long! {} "context_window_tokens" "anthropic:claude-opus-4-8"))))
  (check "missing-field error names the model AND the field"
         (and msg (re-find #"anthropic:claude-opus-4-8" msg)
                  (re-find #"context_window_tokens" msg))))

;; --- B. canonical projection failures throw, never empty --------------------
(with-redefs [north.coord/show-envelope
              (fn [& _]
                (throw (ex-info "query evaluation stopped: query-time-limit"
                                {:error "query evaluation stopped: query-time-limit"
                                 :code :query-time-limit})))]
  (check "facts throws on a query-time-limit envelope (never a silent empty map)"
         (= :catalog-projection-query-failed
            (ex-type #(facts 7977 "@catalog:v1:staffing"))))
  (check "the thrown query failure carries the original coordinator :code"
         (= :query-time-limit
            (try (facts 7977 "@catalog:v1:staffing") nil
                 (catch clojure.lang.ExceptionInfo e (:code (ex-data e)))))))

(with-redefs [north.coord/resolved-envelope
              (fn [& _]
                (throw (ex-info "query evaluation stopped: query-time-limit"
                                {:error "query evaluation stopped: query-time-limit"
                                 :code :query-time-limit})))]
  (check "current-version surfaces the timeout, not a misleading missing-pointer"
         (= :catalog-projection-query-failed (ex-type #(current-version 7977)))))

;; --- B2. an appended pointer is refused, never silently elected -------------
;; :value is the coexist-elect winner (the EARLIEST fact), so a pointer that
;; appended instead of superseding would project the STALE version in silence.
(with-redefs [north.coord/resolved-envelope
              (fn [& _]
                {:value "2" :members 2 :values ["2" "3"]
                 :ambiguous? true :version 1})]
  (check "current-version refuses an ambiguous @catalog:current"
         (= :catalog-pointer-ambiguous (ex-type #(current-version 7977)))))

(with-redefs [north.coord/resolved-envelope
              (fn [& _]
                {:value "3" :members 1 :values ["3"]
                 :ambiguous? false :version 1})]
  (check "current-version accepts a single-valued pointer" (= 3 (current-version 7977))))

;; --- C. happy path is unchanged ---------------------------------------------
(with-redefs [north.coord/show-envelope
              (fn [& _] {:rows [["axis" "tier"] ["rank" "0"]]
                          :version 1})]
  (check "facts returns the parsed rows on a healthy :show envelope"
         (= {"axis" ["tier"] "rank" ["0"]} (facts 7977 "@catalog:v1:axis_value:x"))))

;; --- D. end-to-end: a model missing context_window_tokens names the model ----
;; Stub the whole project-provider flow with one anthropic model that lacks the
;; required context window field; the projector must raise a named-model error
;; rather than NPE — the exact "missing-field row yields named-model error"
;; guarantee the SDK's packaged-JSON fallback then catches.
(with-redefs
  [north.coord/resolved-envelope
   (fn [& _]
     {:value "1" :members 1 :values ["1"] :ambiguous? false :version 1})
   north.coord/show-envelope
   (fn [_ subject]
     (case subject
       "@catalog:v1:provider:anthropic"
       {:rows [["kind" "provider_catalog"]] :version 1}
       ;; Deliberately MISSING context_window_tokens.
       "@catalog:v1:model:anthropic:claude-opus-4-8"
       {:rows [["deliberation_support" "high"]] :version 1}
       {:rows [] :version 1}))
   north.coord/query-rows
   (fn [_ query]
     (let [body (get-in query [:rules 0 :body])
           ;; the kind-scan body is [{:rel triple :args [{:var s} "kind" K]}]
           kind (let [args (:args (first body))]
                  (when (= 3 (count args)) (nth args 2)))]
       (cond
         (= kind "model") [["@catalog:v1:model:anthropic:claude-opus-4-8"]]
         (= kind "tier_row") []
         :else [])))]
  (let [t (ex-type #(project-provider 7977 "anthropic"))
        m (ex-message #(project-provider 7977 "anthropic"))]
    (check "project-provider on a field-less model throws :catalog-projection-missing-field"
           (= :catalog-projection-missing-field t))
    (check "the end-to-end error names the model claude-opus-4-8 and the field"
           (and m (re-find #"claude-opus-4-8" m) (re-find #"context_window_tokens" m)))))

;; --- E. policy pin reads every linked rule in one bounded scoped projection --
(let [rule (rule-map "decisionOwnership" "bounded" "decision-ownership:bounded"
                     "standard" "medium")
      rules [rule]
      digest (rules-digest rules)
      subject "@catalog:v2:rule:decision-ownership:bounded"
      policy "@catalog:v2:selection-policy:minimum-sufficient-v1"
      triples (fn [min-tier]
                [[subject "kind" "selection_rule"]
                 [subject "signal" (get rule "signal")]
                 [subject "signal_value" (get rule "signal_value")]
                 [subject "rule_code" (get rule "rule_code")]
                 [subject "min_tier" min-tier]
                 [subject "min_reasoning" (get rule "min_reasoning")]])
      healthy {:version 9
               :rows {subject (mapv #(subvec % 1) (triples "standard"))}}
      invoke
      (fn [scoped]
        (let [calls (atom [])]
          (with-redefs
            [enumerate-selection-rules (constantly rules)
             north.coord/resolved-envelope
             (fn [_ resolved-subject predicate]
               (swap! calls conj {:op :resolved
                                  :subject resolved-subject
                                  :predicate predicate})
               {:value "2" :members 1 :values ["2"]
                :ambiguous? false :version 8})
             north.coord/show-envelope
             (fn [_ shown-subject]
               (swap! calls conj {:op :show :subject shown-subject})
               (if (= policy shown-subject)
                 {:version 8 :rows [["policy_sha256" digest]
                                    ["rule" subject]]}
                 (throw (ex-info "unexpected exact subject projection"
                                 {:subject shown-subject}))))
             north.coord/show-many-in-domain
             (fn [_ domain subjects]
               (swap! calls conj {:op :show-many
                                  :domain domain
                                  :subjects subjects})
               (if (= ::timeout scoped)
                 (throw (ex-info "fixture timeout"
                                 {:type :coordination-operation-timeout}))
                 scoped))]
            [(try {:value (project-policy-pin 7977)}
                  (catch clojure.lang.ExceptionInfo error {:error error}))
             @calls])))
      [primary primary-calls] (invoke healthy)
      pin (:value primary)]
  (check "policy pin uses resolved + policy show + one scoped rule request"
         (= [:resolved :show :show-many] (mapv :op primary-calls)))
  (check "healthy scoped policy projection preserves three-way digest equality"
         (= #{digest} (set (map pin ["storedSha256" "projectionSha256" "validatorSha256"]))))
  (check "scoped request contains exactly the live linked rule subjects"
         (and (= :coordination (:domain (last primary-calls)))
              (= [subject] (:subjects (last primary-calls)))))

  (let [[tampered _]
        (invoke (assoc-in healthy [:rows subject]
                          (mapv #(subvec % 1) (triples "frontier"))))
        tampered-pin (:value tampered)]
    (check "a bare live rule write still changes only the projection digest"
           (and (= digest (get tampered-pin "storedSha256"))
                (= digest (get tampered-pin "validatorSha256"))
                (not= digest (get tampered-pin "projectionSha256")))))

  (let [[timed-out timeout-calls] (invoke ::timeout)]
    (check "typed scoped timeout fails closed without a second read path"
           (and (= :coordination-operation-timeout
                   (some-> timed-out :error ex-data :type))
                (= [:resolved :show :show-many]
                   (mapv :op timeout-calls)))))

  (let [[malformed malformed-calls]
        (invoke (assoc healthy :unexpected true))]
    (check "malformed scoped success fails closed"
           (and (= :catalog-projection-query-failed
                   (some-> malformed :error ex-data :type))
                (= [:resolved :show :show-many]
                   (mapv :op malformed-calls)))))

  (let [[missing _] (invoke (assoc healthy :rows {}))]
    (check "scoped projection missing a linked rule subject fails closed"
           (= :catalog-projection-query-failed
              (some-> missing :error ex-data :type)))))

;; --- F. durable catalog pin validates both version and canonical payload -----
(let [path (str (java.nio.file.Files/createTempFile
                 "north-catalog-pin-test-" ".json"
                 (make-array java.nio.file.attribute.FileAttribute 0)))
      bundle {"catalogVersion" 7
              "staffing" {"version" 2 "presets" [{"name" "integrator"}]}
              "providers" {"openai" {"provider" "openai"}}}
      digest (sha256-hex
              (json/generate-string
               (canon {"staffing" (get bundle "staffing")
                       "providers" (get bundle "providers")})))
      record {"version" 1 "catalogVersion" 7 "coordinatorVersion" 10
              "catalogDigestSha256" digest "bundle" bundle}]
  (try
    (spit path (json/generate-string record))
    (with-redefs [catalog-projection-cache-path (constantly path)]
      (check "catalog pin cache returns its digest with the current coordinator watermark"
             (= {"catalogVersion" 7 "coordinatorVersion" 99 "catalogDigestSha256" digest}
                (cached-catalog-pin 7 99)))
      (check "catalog version change invalidates the durable pin"
             (nil? (cached-catalog-pin 8 100)))
      (spit path (json/generate-string
                  (assoc-in record ["bundle" "staffing" "version"] 3)))
      (check "cached payload mutation without a matching digest is rejected"
             (nil? (cached-catalog-pin 7 101))))
    (finally (.delete (io/file path)))))

(let [rs @results
      passed (count (filter true? rs))]
  (println (format "\n%d/%d strictness checks passed" passed (count rs)))
  (System/exit (if (every? true? rs) 0 1)))
