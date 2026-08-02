#!/usr/bin/env bb
(require '[clojure.java.io :as io])
(load-file "cli/dashboard-state.clj")
(load-file "cli/dashboard-collectors.clj")
(load-file "cli/dashboard-render.clj")
(defn fail! [s] (binding [*out* *err*] (println "FAIL" s)) (System/exit 1))
(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-live" (make-array java.nio.file.attribute.FileAttribute 0)))]
  (try
    (with-redefs [north.dashboard.state/cache-dir (constantly (.getPath root))]
      (north.dashboard.state/record! :lanes {:status :ok :data {:lanes [{:id "abc" :title "fixture lane" :status "suspect" :last-output-age 70000}]}})
      (north.dashboard.state/record! :lanes {:status :error :detail "fixture failure"})
      (let [out (north.dashboard.render/render)]
        (when-not (.contains out "failed-refresh") (fail! "failed refresh did not preserve last good evidence"))
        (when-not (.contains out "fixture lane") (fail! "cached lane vanished after failed refresh")))
      (let [calls (atom 0)]
        (with-redefs [north.dashboard.state/record! (fn [& _] nil)]
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (north.dashboard.collectors/collect! :board #(swap! calls inc))
          (Thread/sleep 30)
          (when-not (= 1 @calls) (fail! "board collector overlapped")))))
    (println "dashboard-live: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
