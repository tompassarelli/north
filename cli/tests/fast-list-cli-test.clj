#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])
(load-file "cli/dashboard-state.clj")
(load-file "cli/fast-list-cli.clj")

(def checks (atom []))
(defn check! [label ok?] (swap! checks conj [label (boolean ok?)]))
(def root (.toFile (java.nio.file.Files/createTempDirectory "north-fast-list-" (make-array java.nio.file.attribute.FileAttribute 0))))

(try
  (with-redefs [north.dashboard.state/cache-dir (constantly (.getPath root))]
    (let [renders (atom 0) refreshes (atom 0)]
      (north.dashboard.state/record! :board {:status :ok :data {:text "cached board\n"}})
      (with-redefs [north.fast-list-cli/render! (fn [_ _] (swap! renders inc) "fresh board\n")
                    north.fast-list-cli/launch! (fn [_ _] (swap! refreshes inc))]
        (let [out (with-out-str (north.fast-list-cli/-main "board"))]
          (check! "cached hit returns rendered snapshot with an age label"
                  (and (str/includes? out "cached board") (str/includes? out "as of 0s ago") (= 0 @renders) (= 1 @refreshes))))
        (let [out (with-out-str (north.fast-list-cli/-main "board" "--fresh"))]
          (check! "--fresh bypasses the cached rendering" (and (= "fresh board\n" out) (= 1 @renders) (= 1 @refreshes))))
        (north.dashboard.state/write-panel! :next {:schema north.dashboard.state/schema})
        (let [out (with-out-str (north.fast-list-cli/-main "next"))]
          (check! "first run blocks for and saves an authoritative render" (and (= "fresh board\n" out) (= 2 @renders) (str/starts-with? (get-in (north.dashboard.state/read-panel :next) [:last-good :data :text]) "fresh"))))))
    (check! "refresh process uses flock for cross-invocation single-flight"
            (str/includes? (slurp "cli/fast-list-cli.clj") "flock")))
  (finally (doseq [file (reverse (file-seq root))] (io/delete-file file true))))

(doseq [[label ok?] @checks] (println (if ok? "PASS" "FAIL") label))
(System/exit (if (every? second @checks) 0 1))
