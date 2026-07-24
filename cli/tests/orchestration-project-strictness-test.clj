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
;; Daemon-free: load the projector as a library (main-guarded) and stub send-op.
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

;; --- B. strict envelope: a timed-out query throws, never empties -------------
(with-redefs [send-op (fn [_ _] {:error "query evaluation stopped: query-time-limit"
                                 :code :query-time-limit})]
  (check "facts throws on a query-time-limit envelope (never a silent empty map)"
         (= :catalog-projection-query-failed
            (ex-type #(facts 7977 "@catalog:v1:staffing"))))
  (check "the thrown query failure carries the original coordinator :code"
         (= :query-time-limit
            (try (facts 7977 "@catalog:v1:staffing") nil
                 (catch clojure.lang.ExceptionInfo e (:code (ex-data e))))))
  (check "current-version surfaces the timeout, not a misleading missing-pointer"
         (= :catalog-projection-query-failed (ex-type #(current-version 7977)))))

;; --- C. happy path is unchanged ---------------------------------------------
(with-redefs [send-op (fn [_ _] {:ok [["axis" "tier"] ["rank" "0"]] :version 1 :engine "index"})]
  (check "facts returns the parsed rows on a healthy :ok envelope"
         (= {"axis" ["tier"] "rank" ["0"]} (facts 7977 "@catalog:v1:axis_value:x"))))

;; --- D. end-to-end: a model missing context_window_tokens names the model ----
;; Stub the whole project-provider flow with one anthropic model that lacks the
;; required context window field; the projector must raise a named-model error
;; rather than NPE — the exact "missing-field row yields named-model error"
;; guarantee the SDK's packaged-JSON fallback then catches.
(with-redefs
  [send-op
   (fn [_ op]
     (cond
       (= :resolved (:op op)) {:value "1"}
       (= :query (:op op))
       (let [q (:query op)
             body (get-in q [:rules 0 :body])
             ;; the kind-scan body is [{:rel triple :args [{:var s} "kind" K]}]
             kind (let [args (:args (first body))] (when (= 3 (count args)) (nth args 2)))
             subj (let [args (:args (first body))] (when (string? (first args)) (first args)))]
         (cond
           (= kind "model")    {:ok [["@catalog:v1:model:anthropic:claude-opus-4-8"]] :version 1 :engine "index"}
           (= kind "tier_row") {:ok [] :version 1 :engine "index"}
           (= subj "@catalog:v1:provider:anthropic") {:ok [["kind" "provider_catalog"]] :version 1 :engine "index"}
           ;; the model's own facts — deliberately MISSING context_window_tokens
           (= subj "@catalog:v1:model:anthropic:claude-opus-4-8")
           {:ok [["deliberation_support" "high"]] :version 1 :engine "index"}
           :else {:ok [] :version 1 :engine "index"}))
       :else {:ok [] :version 1 :engine "index"}))]
  (let [t (ex-type #(project-provider 7977 "anthropic"))
        m (ex-message #(project-provider 7977 "anthropic"))]
    (check "project-provider on a field-less model throws :catalog-projection-missing-field"
           (= :catalog-projection-missing-field t))
    (check "the end-to-end error names the model claude-opus-4-8 and the field"
           (and m (re-find #"claude-opus-4-8" m) (re-find #"context_window_tokens" m)))))

(let [rs @results
      passed (count (filter true? rs))]
  (println (format "\n%d/%d strictness checks passed" passed (count rs)))
  (System/exit (if (every? true? rs) 0 1)))
