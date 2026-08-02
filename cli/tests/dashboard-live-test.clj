#!/usr/bin/env bb
(require '[clojure.java.io :as io] '[babashka.process :as p] '[clojure.string :as str])
(load-file "cli/dashboard-state.clj")
(load-file "cli/dashboard-collectors.clj")
(load-file "cli/dashboard-render.clj")
(defn fail! [s] (binding [*out* *err*] (println "FAIL" s)) (System/exit 1))
(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-live" (make-array java.nio.file.attribute.FileAttribute 0)))
      state-root (io/file root "state") agents (io/file state-root "agents") threads (io/file state-root "threads")
      pid (str/trim (:out (p/sh "bash" "-c" "echo $PPID")))]
  (try
    (.mkdirs agents) (.mkdirs threads)
    (spit (io/file agents "lane-test1.log") "[spawn] starting provider=openai tier=standard (route=standard/medium)\n")
    (spit (io/file agents "lane-test1.lane.pid") (str pid))
    (spit (io/file agents "lane-test2.log") "[spawn] starting provider=anthropic tier=senior (route=senior/high)\n")
    (spit (io/file agents "lane-test2.lane.exit") "0")
    (spit (io/file threads "test1.md") "# Fixture lane one\n")
    (with-redefs [north.dashboard.collectors/state-dir (.getPath state-root)
                  north.dashboard.state/cache-dir (constantly (.getPath root))]
      (let [lanes (:lanes (north.dashboard.collectors/lanes))
            by-id (into {} (map (juxt :id identity) lanes))]
        (when-not (= #{"test1" "test2"} (set (keys by-id))) (fail! (str "lane discovery failed: " (keys by-id))))
        (when-not (= "advancing" (get-in by-id ["test1" :status])) (fail! "live pid lane status was not advancing"))
        (when-not (= "finished" (get-in by-id ["test2" :status])) (fail! "exit lane status was not finished"))
        (when-not (= "Fixture lane one" (get-in by-id ["test1" :title])) (fail! "thread title was not extracted"))
        (when-not (and (number? (get-in by-id ["test1" :elapsed])) (= "standard" (get-in by-id ["test1" :role])) (= "openai" (get-in by-id ["test1" :provider])))
          (fail! "spawn metadata or elapsed was not extracted")))
      (north.dashboard.state/record! :lanes {:status :ok :data {:lanes [{:id "abc" :title "fixture lane" :status "suspect" :elapsed 70000 :role "standard" :provider "openai" :last-output-age 70000}]}})
      (north.dashboard.state/record! :lanes {:status :error :detail "fixture failure"})
      (let [out (north.dashboard.render/render)]
        (when-not (.contains out "failed-refresh") (fail! "failed refresh did not preserve last good evidence"))
        (when-not (.contains out "fixture lane") (fail! "cached lane vanished after failed refresh"))
        (when (.contains out "?") (fail! "rendered an unavailable placeholder")))
      (let [calls (atom 0)]
        (with-redefs [north.dashboard.state/record! (fn [& _] nil)]
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (Thread/sleep 30)
          (when-not (= 1 @calls) (fail! "board collector overlapped")))))
    (println "dashboard-live: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
