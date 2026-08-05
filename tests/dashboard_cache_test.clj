#!/usr/bin/env bb
(load-file "cli/dashboard-cli.clj")
(require '[clojure.java.io :as io])

(defn fail! [message]
  (binding [*out* *err*] (println "FAIL" message))
  (System/exit 1))

(def cache-root (.getPath (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-cache" (make-array java.nio.file.attribute.FileAttribute 0)))))
(alter-var-root #'CACHE-DIR (constantly cache-root))
(cache-put! "probe.edn" {:ok true})
(def probe-file (io/file CACHE-DIR (str CACHE-SCOPE "-probe.edn")))

(spit probe-file (pr-str {:ts (+ (System/currentTimeMillis) 60000) :val {:poison true}}))
(when (cache-get "probe.edn" 300000) (fail! "future cache timestamp was accepted"))

(let [concern-file (io/file CACHE-DIR (str CACHE-SCOPE "-concerns.edn"))]
  (.delete concern-file)
  (let [result (with-redefs [run (fn [& _] {:out "" :err "boom" :exit 1 :ok false})]
                 (concern-rows))]
    (when-not (:err result) (fail! "failed concern probe was treated as success"))
    (when (.exists concern-file) (fail! "failed concern probe was cached"))))

(let [perms (java.nio.file.Files/getPosixFilePermissions
              (.toPath probe-file) (make-array java.nio.file.LinkOption 0))
      names (set (map str perms))]
  (when-not (= names #{"OWNER_READ" "OWNER_WRITE"})
    (fail! (str "cache permissions are not 0600: " names))))

(let [root (.toFile (java.nio.file.Files/createTempDirectory "north-dashboard-envelope" (make-array java.nio.file.attribute.FileAttribute 0)))]
  (try
    (with-redefs [north.dashboard.state/cache-dir (constantly (.getPath root))]
      (north.dashboard.state/record! :board {:status :ok :data {:open 3}})
      (let [before (north.dashboard.state/read-panel :board)]
        (north.dashboard.state/record! :board {:status :timeout :detail "deadline"})
        (let [panel (north.dashboard.state/read-panel :board)
              dir-perms (set (map str (java.nio.file.Files/getPosixFilePermissions (.toPath root) (make-array java.nio.file.LinkOption 0))))
              file-perms (set (map str (java.nio.file.Files/getPosixFilePermissions (.toPath (north.dashboard.state/panel-file :board)) (make-array java.nio.file.LinkOption 0))))]
          (when-not (= "north.dashboard/panel-v1" (:schema panel)) (fail! "panel schema missing"))
          (when-not (= {:open 3} (get-in panel [:last-good :data])) (fail! "failed attempt destroyed last-good"))
          (when-not (= (dissoc before :last-attempt) (dissoc panel :last-attempt)) (fail! "failed attempt changed more than last-attempt"))
          (when-not (= "timeout" (get-in panel [:last-attempt :status])) (fail! "attempt status missing"))
          (when-not (= #{"OWNER_READ" "OWNER_WRITE" "OWNER_EXECUTE"} dir-perms) (fail! (str "cache directory is not 0700: " dir-perms)))
          (when-not (= #{"OWNER_READ" "OWNER_WRITE"} file-perms) (fail! (str "panel file is not 0600: " file-perms)))
          (when (seq (filter #(.contains (.getName %) ".tmp") (file-seq root))) (fail! "atomic replacement left a temp file")))))
    (println "dashboard-cache: passed")
    (finally (doseq [f (reverse (file-seq root))] (io/delete-file f true)))))
