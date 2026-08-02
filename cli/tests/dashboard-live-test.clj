#!/usr/bin/env bb
(require '[clojure.java.io :as io] '[babashka.process :as p] '[clojure.string :as str])
(load-file "cli/dashboard-state.clj")
(load-file "cli/dashboard-collectors.clj")
(load-file "cli/dashboard-render.clj")
(defn fail! [s] (binding [*out* *err*] (println "FAIL" s)) (System/exit 1))
(defn check [p s] (when-not p (fail! s)))
(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-live" (make-array java.nio.file.attribute.FileAttribute 0)))
      state-root (io/file root "state") agents (io/file state-root "agents") threads (io/file state-root "threads")
      pid (str/trim (:out (p/sh "bash" "-c" "echo $PPID")))]
  (try
    (.mkdirs agents) (.mkdirs threads)
    (spit (io/file agents "lane-test1.log") "[spawn] starting provider=openai tier=standard (route=standard/medium)\n")
    (spit (io/file agents "lane-test1.lane.pid") (str pid))
    (spit (io/file agents "lane-test2.log") "dead\n")
    (spit (io/file agents "lane-test3.log") "[spawn] starting provider=anthropic tier=senior\n")
    (spit (io/file agents "lane-test3.lane.exit") "0")
    (spit (io/file agents "lane-test4.log") "AGENT_THREAD=thread-fixture\n[spawn] complete (process=ran, delivery=ok)\n")
    (spit (io/file agents "lane-test5.log") "[spawn] complete (process=provider_error, delivery=no)\n")
    (spit (io/file threads "thread-fixture-slug.md") "# Joined thread title\n")
    (with-redefs [north.dashboard.collectors/state-dir (.getPath state-root)
                  north.dashboard.state/cache-dir (constantly (.getPath root))]
      (reset! north.dashboard.collectors/log-sizes {})
      (let [by-id (into {} (map (juxt :id identity) (:lanes (north.dashboard.collectors/lanes))))]
        (check (= "live quiet" (get-in by-id ["test1" :status])) "first live observation was not live quiet")
        (check (= "suspect" (get-in by-id ["test2" :status])) "first stale observation was not suspect")
        (check (= "finished" (get-in by-id ["test3" :status])) "exit lane was not finished")
        (check (= "finished" (get-in by-id ["test4" :status])) "managed completion was not finished")
        (check (= "failed" (get-in by-id ["test5" :status])) "managed provider failure was not failed")
        (check (= "Joined thread title" (get-in by-id ["test4" :title])) "joined thread title was not extracted")
        (check (and (= "standard" (get-in by-id ["test1" :role])) (= "openai" (get-in by-id ["test1" :provider]))) "spawn metadata missing"))
      (spit (io/file agents "lane-test2.log") "dead but growing\n")
      (check (= "advancing" (get-in (into {} (map (juxt :id identity) (:lanes (north.dashboard.collectors/lanes)))) ["test2" :status])) "second grown observation was not advancing")
      (let [many (vec (for [n (range 14)] {:id (str n) :title (apply str (repeat 50 "x")) :status "suspect" :last-output-age n}))
            board "THREADS — 12 open threads · 7 active · 2 ready\n\nACTIVE\n  one very active thread\n  two very active thread\n  three very active thread\n  four very active thread\n  five very active thread\n  six hidden thread\n\nREADY\n raw details that must not appear"
            providers {:providers [{:targets [{:id "openai-main" :routing "eligible" :usage {:windows [{:usedPercent 42 :resetsAt "2026-08-04T10:00:00Z"}]}}]}]}]
        (north.dashboard.state/record! :lanes {:status :ok :data {:lanes many}})
        (north.dashboard.state/record! :health {:status :ok :data {:services {"north-coord.service" {:active true :socket true :memory {"memory.current" "2254857830" "memory.max" "19327352832"}}}}})
        (north.dashboard.state/record! :board {:status :ok :data {:text board}})
        (north.dashboard.state/record! :providers {:status :ok :data providers})
        (let [out (north.dashboard.render/render) lines (str/split-lines out)]
          (check (some #(.contains % "(+2 older)") lines) "fleet row cap summary missing")
          (check (every? #(<= (count %) 100) lines) "line width was not truncated")
          (check (and (.contains out "THREADS — 12 open") (.contains out "five very active") (not (.contains out "six hidden")) (not (.contains out "raw details"))) "board was not summarized")
          (check (.contains out "openai-main  eligible  42%  reset 2026-08-04T10:00:00Z") "providers were not rendered as one line")))
      (let [calls (atom 0)]
        (with-redefs [north.dashboard.state/record! (fn [& _] nil)]
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (Thread/sleep 30)
          (check (= 1 @calls) "board collector overlapped"))))
    (println "dashboard-live: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
