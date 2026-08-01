#!/usr/bin/env bb
(ns north.lane-run
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.time Duration Instant]
           [java.nio.charset StandardCharsets]
           [java.security MessageDigest]
           [java.util UUID]))

(load-file (str (.getParent (io/file *file*)) "/coord.clj"))

(def usage
  (str "usage:\n"
       "  north-lane-run start --thread ID --arm random|graph|text|forced-graph|forced-text|na"
       " --provider codex|claude --account ID --model MODEL --task TEXT"
       " [--size-files N --size-lines N] [--est-tokens N --est-wall-min N] [--by AGENT]\n"
       "    RANDOM MODE: statistically random, reproducible — implemented as a deterministic hash.\n"
       "  north-lane-run estimate RUN --tokens N --wall-min N [--why TEXT] [--by AGENT]\n"
       "  north-lane-run finish RUN --outcome landed|returned|failed|partial"
       " --retries N [--tokens-in N --tokens-out N]\n"
       "  north-lane-run extract codex|claude [FILE|-]"))

(def arms #{"random" "graph" "text" "forced-graph" "forced-text" "na"})
(def providers #{"codex" "claude"})
(def outcomes #{"landed" "returned" "failed" "partial"})
(def ^:dynamic *now* #(Instant/now))
(def ^:dynamic *new-token* #(str "run:" (UUID/randomUUID)))
(def ^:dynamic *new-estimate-token* #(str "run:estimate:" (UUID/randomUUID)))
(def ^:dynamic *env* #(System/getenv %))

(defn fail! [message]
  (throw (ex-info message {:type :usage})))

(defn nonblank! [label value]
  (when (str/blank? value) (fail! (str label " requires a nonblank value")))
  value)

(defn nonnegative! [label value]
  (let [number (when (and value (re-matches #"[0-9]+" value)) (parse-long value))]
    (when-not number (fail! (str label " requires a non-negative integer")))
    number))

(defn positive! [label value]
  (let [number (nonnegative! label value)]
    (when (zero? number) (fail! (str label " requires a positive integer")))
    number))

(defn parse-flags [args allowed]
  (loop [remaining args result {}]
    (if (empty? remaining)
      result
      (let [[flag value & more] remaining]
        (when-not (contains? allowed flag) (fail! (str "unknown option " (pr-str flag))))
        (when (nil? value) (fail! (str flag " requires a value")))
        (when (contains? result flag) (fail! (str "duplicate option " flag)))
        (recur more (assoc result flag value))))))

(defn canonical-thread [raw]
  (let [bare (some-> raw (str/replace #"^@" ""))]
    (when-not (and bare (re-matches #"[A-Za-z0-9][A-Za-z0-9._:-]*" bare))
      (fail! "--thread requires an exact North thread id"))
    (str "@" bare)))

(defn canonical-run [raw]
  (let [canonical (if (str/starts-with? (or raw "") "@") raw (str "@" raw))]
    (when-not (re-matches #"@run:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
                          canonical)
      (fail! "command requires a run token printed by start"))
    canonical))

(defn canonical-agent [raw]
  (let [bare (some-> raw str/trim (str/replace #"^@?agent:" ""))]
    (when-not (and (seq bare) (re-matches #"[A-Za-z0-9][A-Za-z0-9._:-]*" bare))
      (fail! "--by requires an exact agent identity"))
    (str "@agent:" bare)))

(defn detected-agent []
  (canonical-agent
   (or (*env* "AGENT_ID")
       (*env* "NORTH_AGENT_ID")
       (when-let [id (*env* "CODEX_THREAD_ID")] (str "codex:" id))
       (when-let [id (*env* "CLAUDE_CODE_SESSION_ID")] (str "claude:" id))
       (*env* "NORTH_AUTHOR")
       (when-let [id (*env* "USER")] (str "operator:" id))
       (fail! "agent identity is unavailable; pass --by AGENT"))))

(defn estimator [options]
  (if-let [override (get options "--by")]
    (canonical-agent override)
    (detected-agent)))

(defn random-arm [run]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256")
                        (.getBytes ^String run StandardCharsets/UTF_8))]
    (if (zero? (bit-and 1 (aget digest 0))) "graph" "text")))

(defn facts-of [port subject]
  (let [response (north.coord/show-envelope port subject)]
    (reduce (fn [facts [predicate object]]
              (update facts predicate (fnil conj #{}) object))
            {} (:rows response))))

(defn write-batch! [port subject facts]
  (let [response
        (north.coord/send-op
         port {:op :assert-batch :te subject
               :facts (mapv (fn [[predicate object]] {:p predicate :r object}) facts)})
        acknowledged?
        (and (= #{:ok :written :idempotent :batch} (set (keys response)))
             (integer? (:ok response))
             (vector? (:written response))
             (every? string? (:written response))
             (vector? (:idempotent response))
             (every? string? (:idempotent response))
             (true? (:batch response)))]
    (when-not acknowledged?
      (throw (ex-info "coordinator rejected lane telemetry batch"
                      {:subject subject :response response})))
    response))

(defn estimates-of [port run]
  (let [response
        (binding [north.coord/*operation-domain* :telemetry]
          (north.coord/send-op
           port {:op :query
                 :query {:find "lane_estimate_fact"
                         :rules [{:head {:rel "lane_estimate_fact"
                                         :args [{:var "e"} {:var "p"} {:var "r"}]}
                                  :body [{:rel "triple"
                                          :args [{:var "e"} "estimate_of" run]}
                                         {:rel "triple"
                                          :args [{:var "e"} {:var "p"} {:var "r"}]}]}]}}))]
    (when-not (vector? (:ok response))
      (throw (ex-info "coordinator returned no estimate rows" {:response response})))
    (->> (:ok response)
         (reduce (fn [entities [entity predicate object]]
                   (assoc-in entities [entity predicate] object)) {})
         vals
         (filter #(= "estimate" (get % "kind")))
         vec)))

(defn estimate-order [estimate]
  [(get estimate "estimate_at" "") (get estimate "estimate_by" "")])

(defn ratio-divergent? [left right]
  (> (/ (double (max left right)) (double (min left right))) 2.0))

(defn divergent? [dispatch tokens wall-min]
  (and dispatch
       (or (ratio-divergent? (positive! "estimate_tokens" (get dispatch "estimate_tokens")) tokens)
           (ratio-divergent? (positive! "estimate_wall_min" (get dispatch "estimate_wall_min")) wall-min))))

(defn append-estimate! [port run by tokens wall-min why at]
  (let [entity (*new-estimate-token*)]
    (write-batch!
     port (str "@" entity)
     (cond-> [["estimate_of" run]
              ["estimate_by" by]
              ["estimate_tokens" (str tokens)]
              ["estimate_wall_min" (str wall-min)]]
       why (conj ["estimate_why" why])
       true (conj ["estimate_at" (str at)] ["kind" "estimate"])))
    entity))

(defn require-one [options flag]
  (nonblank! flag (get options flag)))

(defn start! [port args]
  (let [options (parse-flags args
                             #{"--thread" "--arm" "--provider" "--account"
                               "--model" "--task" "--size-files" "--size-lines"
                               "--est-tokens" "--est-wall-min" "--by"})
        thread (canonical-thread (require-one options "--thread"))
        arm (require-one options "--arm")
        provider (require-one options "--provider")
        account (require-one options "--account")
        model (require-one options "--model")
        task (require-one options "--task")
        size-files (some-> (get options "--size-files")
                           (->> (nonnegative! "--size-files")))
        size-lines (some-> (get options "--size-lines")
                           (->> (nonnegative! "--size-lines")))
        est-tokens? (contains? options "--est-tokens")
        est-wall? (contains? options "--est-wall-min")
        _ (when-not (= est-tokens? est-wall?)
            (fail! "--est-tokens and --est-wall-min must be supplied together"))
        est-tokens (when est-tokens? (positive! "--est-tokens" (get options "--est-tokens")))
        est-wall (when est-wall? (positive! "--est-wall-min" (get options "--est-wall-min")))
        dispatcher (estimator options)
        run (*new-token*)
        assigned-arm (if (= "random" arm) (random-arm run) arm)
        started (str (*now*))]
    (when-not (contains? arms arm) (fail! (str "unsupported arm " (pr-str arm))))
    (when-not (contains? providers provider)
      (fail! (str "unsupported provider " (pr-str provider))))
    (when-not (seq (get (facts-of port thread) "title"))
      (fail! (str "thread does not exist or has no title: " thread)))
    (write-batch!
     port (str "@" run)
     (cond-> [["thread" thread]
              ["run_start" started]
              ["run_arm" assigned-arm]
              ["run_dispatcher" dispatcher]
              ["run_provider" provider]
              ["run_account" account]
              ["run_model" model]
              ["run_task" task]]
       (some? size-files) (conj ["run_size_files" (str size-files)])
       (some? size-lines) (conj ["run_size_lines" (str size-lines)])
       (= "random" arm) (conj ["run_assignment_mode" "random"])
       true (conj ["kind" "run"])))
    (if est-tokens?
      (append-estimate! port (str "@" run) dispatcher est-tokens est-wall nil started)
      (binding [*out* *err*]
        (println "WARNING: dispatch has no estimate; estimating is part of deciding to spend")))
    run))

(defn singleton [facts predicate]
  (let [values (get facts predicate)]
    (when-not (= 1 (count values))
      (fail! (str "run requires exactly one " predicate " fact")))
    (first values)))

(defn estimate! [port token args]
  (let [options (parse-flags args #{"--tokens" "--wall-min" "--why" "--by"})
        tokens (positive! "--tokens" (require-one options "--tokens"))
        wall-min (positive! "--wall-min" (require-one options "--wall-min"))
        why (some-> (get options "--why") (->> (nonblank! "--why")))
        by (estimator options)
        run (canonical-run token)
        run-facts (facts-of port run)
        _ (when-not (= #{"run"} (get run-facts "kind"))
            (fail! "run token is unknown or is not lane telemetry"))
        estimates (estimates-of port run)
        dispatcher (or (first (get run-facts "run_dispatcher"))
                       (get (first (sort-by estimate-order estimates)) "estimate_by"))
        dispatch-estimate (some->> estimates
                                   (filter #(= dispatcher (get % "estimate_by")))
                                   (sort-by estimate-order)
                                   first)
        divergence (and (not= by dispatcher)
                        (divergent? dispatch-estimate tokens wall-min))
        entity (append-estimate! port run by tokens wall-min why (*now*))]
    {:estimate entity :divergent? divergence :dispatcher dispatcher}))

(defn finish! [port token args]
  (let [options (parse-flags args
                             #{"--outcome" "--retries" "--tokens-in" "--tokens-out"})
        outcome (require-one options "--outcome")
        retries (nonnegative! "--retries" (require-one options "--retries"))
        token-in? (contains? options "--tokens-in")
        token-out? (contains? options "--tokens-out")
        _ (when-not (= token-in? token-out?)
            (fail! "--tokens-in and --tokens-out must be supplied together"))
        tokens-in (when token-in? (nonnegative! "--tokens-in" (get options "--tokens-in")))
        tokens-out (when token-out? (nonnegative! "--tokens-out" (get options "--tokens-out")))
        run (canonical-run token)
        facts (facts-of port run)
        _ (when-not (= #{"run"} (get facts "kind"))
            (fail! "run token is unknown or is not lane telemetry"))
        _ (when (seq (get facts "run_end")) (fail! "run is already finished"))
        started (try (Instant/parse (singleton facts "run_start"))
                     (catch Exception _ (fail! "run_start is not an ISO-8601 instant")))
        ended (*now*)
        wall (.toMillis (Duration/between started ended))]
    (when-not (contains? outcomes outcome)
      (fail! (str "unsupported outcome " (pr-str outcome))))
    (when (neg? wall) (fail! "run_end precedes run_start"))
    (write-batch!
     port run
     (cond-> [["run_end" (str ended)]
              ["run_wall_ms" (str wall)]
              ["run_outcome" outcome]
              ["run_retries" (str retries)]
              ["run_token_status" (if token-in? "exact" "cannot-determine")]]
       token-in? (conj ["run_tokens_in" (str tokens-in)]
                       ["run_tokens_out" (str tokens-out)])))
    {:run (subs run 1) :wall-ms wall
     :token-status (if token-in? "exact" "cannot-determine")}))

(defn usage-pair [usage]
  (when (map? usage)
    (let [in (get usage "input_tokens") out (get usage "output_tokens")]
      (when (and (integer? in) (not (neg? in))
                 (integer? out) (not (neg? out)))
        {:tokens-in in :tokens-out out}))))

(defn parse-codex-usage [text]
  (let [events (keep (fn [line]
                       (try (json/parse-string line) (catch Exception _ nil)))
                     (str/split-lines text))
        terminals (filter #(= "turn.completed" (get % "type")) events)]
    (when (= 1 (count terminals))
      (usage-pair (get (first terminals) "usage")))))

(defn parse-claude-usage [text]
  (try
    (usage-pair (get (json/parse-string text) "usage"))
    (catch Exception _ nil)))

(defn extract-usage [provider text]
  (case provider
    "codex" (parse-codex-usage text)
    "claude" (parse-claude-usage text)
    (fail! (str "unsupported provider " (pr-str provider)))))

(defn read-input [path]
  (if (or (nil? path) (= path "-")) (slurp *in*) (slurp path)))

(defn command! [args]
  (let [[command & more] args
        port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977"))]
    (case command
      "start" (do (println (start! port more)) 0)
      "estimate" (let [[token & options] more
                       {:keys [estimate divergent? dispatcher]} (estimate! port token options)]
                   (println estimate)
                   (when divergent?
                     (binding [*out* *err*]
                       (println (str "SCOPE-OVERRUN SIGNAL: worker estimate diverges by more than 2x from dispatcher "
                                     dispatcher))))
                   0)
      "finish" (let [[token & options] more
                     {:keys [run wall-ms token-status]} (finish! port token options)]
                 (println (format "%s wall-ms=%d tokens=%s" run wall-ms token-status))
                 0)
      "extract" (let [[provider path & extra] more]
                  (when (or (nil? provider) (seq extra)) (fail! usage))
                  (if-let [{:keys [tokens-in tokens-out]}
                           (extract-usage provider (read-input path))]
                    (println (format "tokens-in=%d tokens-out=%d" tokens-in tokens-out))
                    (println "cannot-determine"))
                  0)
      (fail! usage))))

(defn -main [& args]
  (try
    (System/exit (command! args))
    (catch Exception error
      (binding [*out* *err*]
        (println (str "north-lane-run: " (.getMessage error)))
        (when (= :usage (:type (ex-data error))) (println usage)))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
