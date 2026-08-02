#!/usr/bin/env bb
(require '[clojure.java.io :as io])
(load-file "cli/dashboard-state.clj")

(defn fail! [message]
  (binding [*out* *err*] (println "FAIL" message))
  (System/exit 1))

(let [root (.toFile (java.nio.file.Files/createTempDirectory
                      "north-dashboard-cache"
                      (make-array java.nio.file.attribute.FileAttribute 0)))]
  (try
    (with-redefs [north.dashboard.state/cache-dir (constantly (.getPath root))]
      (north.dashboard.state/record! :board {:status :ok :data {:open 3}})
      (north.dashboard.state/record! :board {:status :timeout :detail "deadline"})
      (let [panel (north.dashboard.state/read-panel :board)
            perms (java.nio.file.Files/getPosixFilePermissions
                   (.toPath (north.dashboard.state/panel-file :board))
                   (make-array java.nio.file.LinkOption 0))]
        (when-not (= "north.dashboard/panel-v1" (:schema panel))
          (fail! "panel schema missing"))
        (when-not (= {:open 3} (get-in panel [:last-good :data]))
          (fail! "failed attempt destroyed last-good"))
        (when-not (= "timeout" (get-in panel [:last-attempt :status]))
          (fail! "attempt status missing"))
        (when-not (= #{"OWNER_READ" "OWNER_WRITE"} (set (map str perms)))
          (fail! "panel file is not 0600"))))
    (println "dashboard-cache: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
