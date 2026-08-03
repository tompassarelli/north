#!/usr/bin/env bb
(require '[clojure.java.io :as io] '[babashka.process :as p] '[clojure.string :as str])
(load-file "cli/dashboard-state.clj")
(load-file "cli/dashboard-collectors.clj")
(load-file "cli/dashboard-render.clj")
(defn fail! [s] (binding [*out* *err*] (println "FAIL" s)) (System/exit 1))
(defn check [p s] (when-not p (fail! s)))
(defn write-journal! [file records]
  (.mkdirs (.getParentFile file))
  (with-open [out (java.io.DataOutputStream. (java.io.FileOutputStream. file))]
    (doseq [record records
            :let [body (.getBytes (cheshire.core/generate-string record) "UTF-8")]]
      (.writeInt out (alength body))
      (.write out body))))
(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-live" (make-array java.nio.file.attribute.FileAttribute 0)))
      state-root (io/file root "state") agents (io/file state-root "agents") threads (io/file state-root "threads")
      pid (str/trim (:out (p/sh "bash" "-c" "echo $PPID")))]
  (try
    (.mkdirs agents) (.mkdirs threads)
    (spit (io/file agents "lane-test1.log") "[spawn] starting provider=openai tier=standard (route=standard/medium)\n")
    (spit (io/file agents "lane-test1.log.lane.pid") (str pid))
    (spit (io/file agents "lane-test2.log") "dead\n")
    (spit (io/file agents "lane-test3.log") "[spawn] starting provider=anthropic tier=senior\n")
    (spit (io/file agents "lane-test3.log.lane.exit") "0")
    (spit (io/file agents "lane-test4.log") "AGENT_THREAD=thread-fixture\n[spawn] complete (process=ran, delivery=ok)\n[harvest] harvested 1 commit(s)\n")
    (spit (io/file agents "lane-test4.meta.json") "{\"thread\":\"thread-fixture\",\"role\":\"executor\",\"tier\":\"standard\",\"effort\":\"high\",\"provider\":\"openai\",\"model\":\"gpt-5.6-sol\",\"startedAt\":\"2026-08-03T00:00:00.000Z\"}\n")
    (spit (io/file agents "lane-test5.log") "[spawn] complete (process=provider_error, delivery=no)\n")
    (spit (io/file agents "lane-fresh.log") "[spawn] starting provider=openai tier=standard\n")
    (spit (io/file agents "lane-long.log") (str "[spawn] complete (process=ran, delivery=ok)\n" (apply str (repeat 4096 "report "))))
    (spit (io/file agents "lane-stale.log") "[spawn] starting provider=openai tier=standard\n")
    (.setLastModified (io/file agents "lane-stale.log") (- (System/currentTimeMillis) 180000))
    (spit (io/file agents "lane-mutation.log") "worktree provisioned\n[spawn] starting provider=anthropic tier=senior (route=senior/high)\n")
    (spit (io/file agents "lane-mutation.log.lane.pid") (str pid))
    (spit (io/file agents "lane-journal-fixture.log") "[spawn] starting provider=openai tier=senior\n")
    (spit (io/file threads "thread-fixture-slug.md") "# Joined thread title\n")
    (let [at (.toString (java.time.Instant/now))]
      (write-journal!
        (io/file state-root "bridge" "journal" "journal-fixture" "events.log")
        [{:version 1 :executionId "journal-fixture" :seq 1 :at at
          :kind "execution.accepted" :data {:prompt "Journal fixture title" :cwd (.getPath root)}}
         {:version 1 :executionId "journal-fixture" :seq 2 :at at
          :kind "provider.starting" :data {:adapter "mock-provider"}}
         {:version 1 :executionId "journal-fixture" :seq 3 :at at
          :kind "provider.result" :data {:result "delivered"}}
         {:version 1 :executionId "journal-fixture" :seq 4 :at at
          :kind "execution.completed" :data {}}]))
    (with-redefs [north.dashboard.collectors/state-dir (.getPath state-root)
                  north.dashboard.state/cache-dir (constantly (.getPath root))]
      (reset! north.dashboard.collectors/log-sizes {})
      (let [by-id (into {} (map (juxt :id identity) (:lanes (north.dashboard.collectors/lanes))))]
        (check (= "advancing" (get-in by-id ["test1" :status])) "first live observation was not working")
        (check (= "advancing" (get-in by-id ["test2" :status])) "first recent observation was not working")
        (check (= "finished" (get-in by-id ["test3" :status])) "exit lane was not finished")
        (check (= "finished" (get-in by-id ["test4" :status])) "managed completion was not finished")
        (check (= "failed" (get-in by-id ["test5" :status])) "managed provider failure was not failed")
        (check (= "advancing" (get-in by-id ["fresh" :status])) "fresh no-pid lane was not working")
        (check (= "finished" (get-in by-id ["long" :status])) "completion beyond tail window was not found")
        (check (= "vanished" (get-in by-id ["stale" :status])) "stale no-terminal lane was not vanished")
        (check (= "unknown" (get-in by-id ["stale" :work])) "vanished lane work was not unknown")
        (check (= "delivered" (get-in by-id ["test4" :work])) "harvested completion was not delivered")
        (check (= "none" (get-in by-id ["test3" :work])) "empty completed lane was not none")
        (check (= "Joined thread title" (get-in by-id ["test4" :title])) "joined thread title was not extracted")
        (check (= "executor" (get-in by-id ["test4" :role])) "meta role was not extracted")
        (check (= "high" (get-in by-id ["test4" :effort])) "meta effort was not extracted")
        (check (= "gpt-5.6-sol" (get-in by-id ["test4" :model])) "meta model was not extracted")
        (check (and (= "standard" (get-in by-id ["test1" :role])) (= "medium" (get-in by-id ["test1" :effort])) (= "openai" (get-in by-id ["test1" :provider]))) "legacy spawn metadata missing")
        (check (and (= "senior" (get-in by-id ["mutation" :role])) (= "high" (get-in by-id ["mutation" :effort]))) "mutation spawn metadata was not found on line two")
        (check (= "finished" (get-in by-id ["journal-fixture" :status])) "journal terminal state was not authoritative")
        (check (= "delivered" (get-in by-id ["journal-fixture" :work])) "journal result was not delivered work")
        (check (= "mock-provider" (get-in by-id ["journal-fixture" :provider])) "journal provider was not projected"))
      (spit (io/file agents "lane-test2.log") "dead but growing\n")
      (check (= "advancing" (get-in (into {} (map (juxt :id identity) (:lanes (north.dashboard.collectors/lanes)))) ["test2" :status])) "second grown observation was not advancing")
      (let [started (.toString (java.time.Instant/ofEpochMilli (- (System/currentTimeMillis) 720000)))
            many (vec (concat [{:id "work-lane" :thread "019fc335-5c17-77d5-a8e2-38001f8c97f9" :startedAt started :title "Working fixture title" :role "integrator" :effort "high" :provider "openai" :model "gpt-5.6-sol" :status "advancing" :work "unknown" :last-output-age 0}
                               {:id "legacy-lane" :role "senior" :effort "high" :provider "anthropic" :status "advancing" :last-output-age 0}
                               {:id "done-empty" :title "Done without a result" :status "finished" :work "none" :last-output-age 0}
                               {:id "delivered-lane" :title "Delivered fixture title" :status "finished" :work "delivered" :last-output-age 0}]
                              (for [n (range 11)] {:id (str n) :title (apply str (repeat 50 "x")) :status "vanished" :work "unknown" :last-output-age n})))
            board "THREADS — 12 open threads · 7 active · 3 ready · 1 blocked\n\nACTIVE\n native-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 019fc335-5c17-77d5-a8e2-38001f8c97f9 Active fixture title\n\nREADY — top 3\n unblocks 2  019fc335-5c17-77d5-a8e2-38001f8c97f9 Low leverage ready\n unblocks 34  019fc336-5c17-77d5-a8e2-38001f8c97f9 High leverage ready\n unblocks 8  019fc337-5c17-77d5-a8e2-38001f8c97f9 Middle leverage ready"
            providers {:providers [{:targets [{:id "openai-main" :routing "eligible" :usage {:windows [{:usedPercent 5 :resetsAt "2026-08-04T10:00:00Z"}]}}
                                             {:id "openai-backup" :routing "exhausted" :usage {:windows [{:usedPercent 35 :resetsAt "2026-08-04T10:00:00Z"}]}}]}]}]
        (north.dashboard.state/record! :lanes {:status :ok :data {:lanes many}})
        (north.dashboard.state/record! :health {:status :ok :data {:services {"north-coord.service" {:active true :socket true :memory {"memory.current" "2254857830" "memory.max" "19327352832"}}}}})
        (north.dashboard.state/record! :board {:status :ok :data {:text board}})
        (north.dashboard.state/record! :providers {:status :ok :data providers})
        (let [out (north.dashboard.render/render) lines (str/split-lines out)]
          (check (.contains out "integrator/high · GPT 5.6 Sol") "resolved model name was not rendered")
          (check (not (.contains out "GPT 5.6 Sol · openai")) "provider suffix remained with resolved model")
          (check (.contains out "senior/high · anthropic") "legacy provider fallback was not rendered")
          (check (some #(.contains % "(+3 older)") lines) "fleet row cap summary missing")
          (check (every? #(<= (count %) 100) lines) "line width was not truncated")
          (check (.contains out "Working fixture title") "meta fleet title did not render")
          (check (.contains out "(untitled)") "untitled fleet row did not render")
          (check (re-find #"(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun) [0-9]{2}:[0-9]{2}|\b[0-9]{2}:[0-9]{2}\b" out) "startedAt timestamp did not render")
          (let [fleet (take-while #(not (.startsWith % "HEALTH")) (drop-while #(not (.startsWith % "FLEET")) lines))
                rows (filter #(.contains % "running") fleet)]
            (check (= 1 (count (set (map #(.indexOf % "running") rows)))) "fleet agent column drifted")
            (check (not-any? #(re-find #"[0-9a-f]{32,}" %) rows) "fleet task column rendered a lane hash"))
          (let [fleet (take-while #(not (.startsWith % "HEALTH")) (drop-while #(not (.startsWith % "FLEET")) lines))
                rows (filter #(re-find #"(?:running|done|vanished)" %) fleet)]
            (check (= 1 (count (set (map #(count (north.dashboard.render/strip-ansi %)) rows))))
                   "fleet rows with different colors had unequal visible widths"))
          (check (re-find #"agent +work" out) "fleet did not render separate agent and work headers")
          (check (re-find #"Done without a result +done +none" out) "done/none fixture did not prove independent axes")
          (check (re-find #"Delivered fixture title +done +delivered" out) "done/delivered fixture did not render")
          (check (re-find #"vanished +unknown" out) "vanished/unknown fixture did not render")
          (let [ids-out (with-redefs [north.dashboard.render/width (constantly 110)]
                          (north.dashboard.render/render true))]
            (check (re-find #"(?:work-lan|legacy-lan|failed-lan)" ids-out) "--ids did not append short lane ids"))
          (check (.contains out "QUEUE") "board header was not renamed")
          (check (re-find #"● 1[12]m Active fixture title" out) (str "meta-bound live lane did not render active stint: " out))
          (check (and (.contains out "○     High leverage ready") (re-find #"High leverage ready +019fc336 +34" out)) "unbound ready row did not align leverage")
          (check (< (.indexOf out "Active fixture title") (.indexOf out "High leverage ready") (.indexOf out "Middle leverage ready") (.indexOf out "Low leverage ready")) "queue ordering was not active then leverage descending")
          (check (and (.contains out "7 active · 3 ready · 1 blocked") (not (.contains out "native-")) (not (re-find #"[0-9a-f]{40,}" out))) "queue was not summarized safely")
          (check (= ["  1 active · 1 ready · 0 blocked"
                     "  ● 12m Active title                                              019fc335          "
                     "  ○     Ready title                                               019fc336         7"]
                    (with-redefs [north.dashboard.state/now (constantly 720000)]
                      (vec (north.dashboard.render/queue-lines
                            "THREADS — 1 active · 1 ready · 0 blocked\n\nACTIVE\n native 019fc335-5c17-77d5-a8e2-38001f8c97f9 Active title\n\nREADY\n unblocks 7  019fc336-5c17-77d5-a8e2-38001f8c97f9 Ready title"
                            [{:thread "019fc335-5c17-77d5-a8e2-38001f8c97f9" :status "advancing" :started-at 0}]))))
                 "QUEUE golden changed")
          (let [account-lines (filter #(or (.contains % "openai-main") (.contains % "openai-backup")) lines)]
            (check (and (= 2 (count account-lines))
                        (= 1 (count (set (map #(.indexOf % "%") account-lines)))))
                   "accounts used percentages did not align"))
          (check (= ["  one                                    eligible    5% 10m     "
                     "  two                                    exhausted  35% 20h     "]
                    (with-redefs [north.dashboard.state/now (constantly 0)]
                      (vec (north.dashboard.render/account-lines
                            {:providers [{:targets [{:id "one" :routing "eligible" :usage {:windows [{:usedPercent 5 :resetsAt "1970-01-01T00:10:00Z"}]}}
                                                    {:id "two" :routing "exhausted" :usage {:windows [{:usedPercent 35 :resetsAt "1970-01-01T20:00:00Z"}]}}]}]}))))
                 "ACCOUNTS golden changed")
          (check (and (.contains out "agent: running/quiet") (.contains out "work: delivered = result or commit")) "footer legend missing")
          (check (not (re-find #"stale" (first (str/split out #"HEALTH")))) "stale leaked into fleet rows")
          (check (re-find #"· data [0-9]+s old" out) "header did not render snapshot age")
          (check (not (re-find #"(?i)suspect|advancing|live quiet|lost" out)) "internal status vocabulary leaked")
          (check (not (.contains out "\u001b")) "NO_COLOR render contained ANSI escape bytes")
          (with-redefs [north.dashboard.render/color? (constantly false)]
            (check (not (re-find #"\u001b" (north.dashboard.render/render))) "NO_COLOR purity failed"))
          (with-redefs [north.dashboard.state/now (constantly 0)
                        north.dashboard.render/color? (constantly true)]
            (let [colored (north.dashboard.render/render)]
              (check (.contains colored "\u001b[32mrunning  \u001b[0m") "running row was not green")
              (check (.contains colored "\u001b[31mnone") "none work was not red")
              (with-redefs [north.dashboard.render/color? (constantly false)]
                (let [plain (north.dashboard.render/render)]
                  (check (= (north.dashboard.render/strip-ansi colored) plain)
                       "colored and NO_COLOR renders differ after ANSI stripping")))))))
      (let [calls (atom 0)]
        (with-redefs [north.dashboard.state/record! (fn [& _] nil)]
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (Thread/sleep 100)
          (check (= 1 @calls) "board collector overlapped")))
      (spit (io/file agents "lane-poison.log") "[spawn] starting provider=openai tier=standard\n")
      (spit (io/file agents "lane-poison.meta.json") "{\"bad\": NaN}\n")
      (with-redefs [north.dashboard.collectors/state-dir (.getPath state-root)
                    north.dashboard.state/record! (fn [& _] (throw (Error. "poisoned snapshot")))]
        (reset! north.dashboard.collectors/running {})
        (reset! north.dashboard.state/fallback-panels {})
        (north.dashboard.collectors/collect! :lanes north.dashboard.collectors/lanes)
        (Thread/sleep 100)
        (check (not (contains? @north.dashboard.collectors/running :lanes)) "record failure left fleet marked running")
        (let [out (with-redefs [north.dashboard.render/width (constantly 500)]
                    (north.dashboard.render/render))]
          (check (.contains out "panel error:") "snapshot failure looked like collecting")
          (check (.contains out "poisoned snapshot") "snapshot failure detail was missing")))
      (with-redefs [north.dashboard.collectors/now (constantly 61001)]
        (reset! north.dashboard.collectors/running {:lanes 0})
        (reset! north.dashboard.state/fallback-panels {})
        (north.dashboard.collectors/clear-stuck!)
        (check (not (contains? @north.dashboard.collectors/running :lanes)) "watchdog did not clear stuck collector")
        (check (= "error" (get-in (north.dashboard.state/read-panel :lanes) [:last-attempt :status])) "watchdog did not record an error")))
    (println "dashboard-live: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
