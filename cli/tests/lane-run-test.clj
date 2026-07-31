#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/lane-run.clj"))

(def checks (atom 0))
(def failures (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass?
    (println "PASS" label)
    (do (swap! failures inc) (println "FAIL" label))))

(def store (atom {"@thread-1" {"title" #{"Fixture thread"}}}))
(def times (atom [(java.time.Instant/parse "2026-07-27T10:00:00Z")
                  (java.time.Instant/parse "2026-07-27T10:00:01.500Z")]))
(def token "run:11111111-1111-4111-8111-111111111111")
(def initial-estimate "run:estimate:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")

(defn next-time []
  (let [value (first @times)]
    (swap! times subvec 1)
    value))

(defn stub-write! [_port subject facts]
  (swap! store update subject
         (fn [existing]
           (reduce (fn [state [predicate object]]
                     (update state predicate (fnil conj #{}) object))
                   (or existing {}) facts)))
  {:ok (count facts) :written (mapv first facts) :idempotent [] :batch true})

(with-redefs [north.lane-run/facts-of (fn [_port subject] (get @store subject {}))
              north.lane-run/estimates-of
              (fn [_port run]
                (->> @store
                     vals
                     (filter #(= #{run} (get % "estimate_of")))
                     (map #(into {} (map (fn [[p values]] [p (first values)]) %)))))
              north.lane-run/write-batch! stub-write!]
  (binding [north.lane-run/*now* next-time
            north.lane-run/*new-token* (constantly token)
            north.lane-run/*new-estimate-token* (constantly initial-estimate)
            north.lane-run/*env* #(get {"AGENT_ID" "dispatcher"} %)]
    (let [started
          (north.lane-run/start!
           7977 ["--thread" "thread-1" "--arm" "random"
                 "--provider" "codex" "--account" "primary"
                 "--model" "gpt-fixture" "--task" "exercise recorder"
                 "--size-files" "3" "--size-lines" "120"
                 "--est-tokens" "200" "--est-wall-min" "5"])
          result
          (north.lane-run/finish!
           7977 started ["--outcome" "landed" "--retries" "0"
                         "--tokens-in" "120" "--tokens-out" "30"])
          facts (get @store (str "@" token))
          estimate-facts (get @store (str "@" initial-estimate))]
      (check! "start returns the finish token" (= token started))
      (check! "start publishes the thread and arm"
              (and (= #{"@thread-1"} (get facts "thread"))
                   (= #{(north.lane-run/random-arm token)} (get facts "run_arm"))
                   (= #{"random"} (get facts "run_assignment_mode"))))
      (check! "start publishes provider metadata and task size"
              (and (= #{"codex"} (get facts "run_provider"))
                   (= #{"primary"} (get facts "run_account"))
                   (= #{"gpt-fixture"} (get facts "run_model"))
                   (= #{"exercise recorder"} (get facts "run_task"))
                   (= #{"3"} (get facts "run_size_files"))
                   (= #{"120"} (get facts "run_size_lines"))))
      (check! "start sugar appends an attributed estimate entity"
              (and (= #{"@agent:dispatcher"} (get facts "run_dispatcher"))
                   (= #{(str "@" token)} (get estimate-facts "estimate_of"))
                   (= #{"@agent:dispatcher"} (get estimate-facts "estimate_by"))
                   (= #{"200"} (get estimate-facts "estimate_tokens"))
                   (= #{"5"} (get estimate-facts "estimate_wall_min"))
                   (= #{"2026-07-27T10:00:00Z"} (get estimate-facts "estimate_at"))
                   (= #{"estimate"} (get estimate-facts "kind"))))
      (check! "finish records exact wall time and outcome"
              (and (= 1500 (:wall-ms result))
                   (= #{"1500"} (get facts "run_wall_ms"))
                   (= #{"landed"} (get facts "run_outcome"))
                   (= #{"0"} (get facts "run_retries"))))
      (check! "finish records exact token usage"
              (and (= #{"exact"} (get facts "run_token_status"))
                   (= #{"120"} (get facts "run_tokens_in"))
                   (= #{"30"} (get facts "run_tokens_out")))))))

(let [run (str "@" token)
      revisions ["run:estimate:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
                 "run:estimate:cccccccc-cccc-4ccc-8ccc-cccccccccccc"]
      next-estimate (atom revisions)]
  (with-redefs [north.lane-run/facts-of (fn [_port subject] (get @store subject {}))
                north.lane-run/estimates-of
                (fn [_port target]
                  (->> @store
                       vals
                       (filter #(= #{target} (get % "estimate_of")))
                       (map #(into {} (map (fn [[p values]] [p (first values)]) %)))))
                north.lane-run/write-batch! stub-write!]
    (binding [north.lane-run/*now* #(java.time.Instant/parse "2026-07-27T10:00:02Z")
              north.lane-run/*new-estimate-token*
              #(let [value (first @next-estimate)] (swap! next-estimate subvec 1) value)]
      (let [first-revision (north.lane-run/estimate!
                            7977 token ["--tokens" "700" "--wall-min" "20"
                                        "--why" "scope larger" "--by" "worker"])
            second-revision (north.lane-run/estimate!
                             7977 token ["--tokens" "250" "--wall-min" "6"
                                         "--why" "intake refined" "--by" "worker"])
            entities (map #(get @store (str "@" %)) revisions)]
        (check! "worker estimate over 2x signals scope overrun"
                (true? (:divergent? first-revision)))
        (check! "revision appends a second entity without overwriting"
                (and (= revisions [(:estimate first-revision) (:estimate second-revision)])
                     (= [#{"700"} #{"250"}] (mapv #(get % "estimate_tokens") entities))
                     (= [#{"scope larger"} #{"intake refined"}]
                        (mapv #(get % "estimate_why") entities))
                     (= 3 (count (filter #(= #{run} (get % "estimate_of")) (vals @store))))))))))

(with-redefs [north.lane-run/facts-of (fn [_port subject] (get @store subject {}))
              north.lane-run/write-batch! stub-write!]
  (binding [north.lane-run/*now* #(java.time.Instant/parse "2026-07-27T12:00:00Z")
            north.lane-run/*new-token*
            (constantly "run:33333333-3333-4333-8333-333333333333")]
    (let [writer (java.io.StringWriter.)
          _ (binding [*err* writer]
            (north.lane-run/start!
             7977 ["--thread" "thread-1" "--arm" "graph"
                   "--provider" "codex" "--account" "primary"
                   "--model" "gpt-fixture" "--task" "unestimated dispatch"
                   "--by" "dispatcher"]))
          warning (str writer)]
      (check! "dispatch without estimate warns loudly"
              (= "WARNING: dispatch has no estimate; estimating is part of deciding to spend\n"
                 warning)))))

(let [second-run "@run:22222222-2222-4222-8222-222222222222"]
  (swap! store assoc second-run
         {"kind" #{"run"} "run_start" #{"2026-07-27T11:00:00Z"}})
  (with-redefs [north.lane-run/facts-of (fn [_port subject] (get @store subject {}))
                north.lane-run/write-batch! stub-write!]
    (binding [north.lane-run/*now* #(java.time.Instant/parse "2026-07-27T11:00:02Z")]
      (north.lane-run/finish!
       7977 (subs second-run 1) ["--outcome" "returned" "--retries" "0"])))
  (let [facts (get @store second-run)]
    (check! "missing usage records cannot-determine with wall time only"
            (and (= #{"cannot-determine"} (get facts "run_token_status"))
                 (= #{"2000"} (get facts "run_wall_ms"))
                 (nil? (get facts "run_tokens_in"))
                 (nil? (get facts "run_tokens_out"))))))

(let [codex (str "{\"type\":\"thread.started\",\"thread_id\":\"fixture\"}\n"
                 "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":41,"
                 "\"cached_input_tokens\":20,\"output_tokens\":7,\"reasoning_output_tokens\":2}}\n")
      claude "{\"type\":\"result\",\"usage\":{\"input_tokens\":13,\"cache_read_input_tokens\":99,\"output_tokens\":5}}"]
  (check! "Codex JSONL terminal usage is exact"
          (= {:tokens-in 41 :tokens-out 7}
             (north.lane-run/extract-usage "codex" codex)))
  (check! "Claude JSON result usage is exact"
          (= {:tokens-in 13 :tokens-out 5}
             (north.lane-run/extract-usage "claude" claude)))
  (check! "ordinary Codex output is explicitly indeterminate"
          (nil? (north.lane-run/extract-usage "codex" "tokens used: unknown")))
  (check! "repeated Codex terminals are explicitly indeterminate"
          (nil? (north.lane-run/extract-usage
                 "codex" (str codex "{\"type\":\"turn.completed\",\"usage\":{"
                              "\"input_tokens\":1,\"output_tokens\":1}}\n")))))

(println (format "lane-run: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
