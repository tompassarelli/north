#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def pred-cli (str root "/cli/pred-cli.clj"))
(def store-root
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_HOME")
      "/home/tom/code/beagle/main/store"))
(def store-out
  (or (some #(when (and % (.isDirectory (io/file %))) %)
            [(System/getenv "BEAGLE_STORE_OUT")
             (some-> (System/getenv "BEAGLE_STORE_TEST_CHECKOUT") (str "/out"))])
      (do
        (binding [*out* *err*]
          (println "predicate test requires BEAGLE_STORE_OUT or BEAGLE_STORE_TEST_CHECKOUT"))
        (System/exit 2))))
(def checks (atom []))
(def forbidden-online-token
  #"(?i)(^|[^A-Za-z0-9_])presence([^A-Za-z0-9_]|$)")

(defn check [label passed?]
  (swap! checks conj [label (boolean passed?)]))

(let [{:keys [exit out err]}
      (if store-out
        (shell/sh "bb" "-cp" store-out pred-cli "7977" "lint-offline" "--strict" :dir root)
        {:exit 1 :out "" :err "Beagle Store classpath is unavailable"})]
  (check "offline predicate lint executes the production registry" (zero? exit))
  (check "every fixed v2 projection predicate is registered"
         (and (str/includes? out "clean against bootstrap inventory")
              (not (str/includes? out "absent from bootstrap inventory"))
              (str/blank? err)
              (not (re-find forbidden-online-token (str out err))))))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn await-port [port]
  (loop [remaining 200]
    (cond
      (try
        (with-open [socket (java.net.Socket.)]
          (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
          true)
        (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(let [tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-predicate-output-"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file tmp "coordination.storelog"))
      port (free-port)
      daemon
      (proc/process
       {:dir store-root :out :string :err :string
        :extra-env {"BEAGLE_STORE_SERVER_QUIET" "1"
                    "BEAGLE_STORE_SERVER_XMX" "1g"}}
       (str store-root "/bin/beagle-store-server") "serve" (str port)
       log "north-coordination")
      run-predicate
      (fn [& args]
        (apply proc/shell
               {:out :string :err :string :continue true
                :extra-env {"BEAGLE_STORE_LOG" log}}
               "bb" "-cp" store-out pred-cli (str port) args))]
  (try
    (check "scratch predicate output coordinator starts" (await-port port))
    (let [defined (run-predicate
                   "define" "session_id" "single" "literal"
                   "session id of a liveness lease registration")
          listed (run-predicate "ls")
          shown (run-predicate "show" "session_id")
          output (str (:out defined) (:err defined)
                      (:out listed) (:err listed)
                      (:out shown) (:err shown))]
      (check "predicate ls and show render the liveness lease vocabulary"
             (and (every? zero? (map :exit [defined listed shown]))
                  (str/includes? (:out listed) "session id of a liveness lease registration")
                  (str/includes? (:out shown) "session id of a liveness lease registration")))
      (when-not (every? zero? (map :exit [defined listed shown]))
        (binding [*out* *err*]
          (println output)))
      (check "predicate ls and show contain no retired online-language token"
             (not (re-find forbidden-online-token output))))
    (finally
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(doseq [[label passed?] @checks]
  (println (format "  [%s] %s" (if passed? "PASS" "FAIL") label)))
(let [passed (count (filter second @checks))]
  (println (format "\npredicate registry behavior: %d / %d PASS" passed (count @checks)))
  (System/exit (if (= passed (count @checks)) 0 1)))
