#!/usr/bin/env bb
;; Provider-owned contexts can only rotate into a fresh managed lane.
(require '[babashka.classpath :as cp]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_TEST_CHECKOUT")
                (System/getenv "FRAM_HOME")
                "/home/tom/code/fram/main"))))
(def fram-out (str fram "/out"))
(cp/add-classpath (str root "/out:" fram-out))
(load-file (str root "/cli/coord.clj"))

(def checks (atom []))
(defn check! [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn eventually [predicate]
  (loop [remaining 800]
    (cond
      (try (boolean (predicate)) (catch Throwable _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))
(defn append! [port subject predicate value]
  (let [result (north.coord/append! port subject predicate value)]
    (when (:reject result)
      (throw (ex-info "context fixture write failed" result)))
    result))

(let [tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-context-replacement-"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file tmp "coordination.framlog"))
      port (free-port)
      space "north-coordination"
      server
      (proc/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                    "FRAM_SERVER_QUIET" "1"
                    "FRAM_SERVER_XMX" "1g"}}
       (str fram "/bin/fram-server") "serve" (str port) log space)]
  (try
    (check! "scratch canonical FRAMRPC server starts"
            (eventually
             #(let [status (north.coord/status port)]
                (and (= :ready (:state status))
                     (= space (:space-id status))))))
    (append! port "@agent:aaaaaaaaaaaa" "needs_rotation" "true")
    (let [result
          (proc/shell
           {:continue true
            :out :string
            :err :string
            :extra-env
            {"FRAM_HOME" fram
             "FRAM_BIN" (str fram "/bin")
             "FRAM_OUT" fram-out
             "NORTH_FRAMRPC_OUT" fram-out
             "FRAM_SPACE_ID" space
             "NORTH_PORT" (str port)}}
           "bb" "-cp" (str root "/out:" fram-out)
           (str root "/cli/dispatch-guard.clj")
           (str port) "aaaaaaaaaaaa")]
      (check! "rotation flag requests provider-neutral replacement"
              (and (= 2 (:exit result))
                   (str/includes? (:out result) "-> REPLACE:")
                   (str/includes? (:out result) "fresh managed lane")
                   (str/includes? (:out result) "provider-neutral")))
      (check! "replacement guidance advertises no fictional execution path"
              (not-any? #(str/includes? (:out result) %)
                        ["compact.sh" "MIGRATE_FROM" "-> COMPACT:"])))

    (let [retired
          (proc/shell
           {:continue true :out :string :err :string}
           "bb" "-cp" (str root "/out:" fram-out)
           (str root "/cli/presence-cli.clj") "59999"
           "compact" "aaaaaaaaaaaa")
          output (str (:out retired) (:err retired))]
      (check! "presence compact stays absent"
              (and (= 2 (:exit retired))
                   (str/includes? output "usage: presence-cli.clj")
                   (not (str/includes? output "|compact")))))

    (check! "runtime sources contain no compact helper reference"
            (not-any?
             #(str/includes? (slurp (io/file root %)) "compact.sh")
             ["cli/presence-cli.clj" "cli/dispatch-guard.clj"]))
    (finally
      (proc/destroy-tree server)
      (try @server (catch Exception _ nil))
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\ncontext replacement: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
