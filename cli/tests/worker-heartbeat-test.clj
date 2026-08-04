#!/usr/bin/env bb
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def lib (str root "/cli/worker-heartbeat.clj"))
(def dashboard (str root "/cli/dashboard-cli.clj"))
(def home
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-worker-heartbeat-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def hb-file (io/file home "heartbeat"))
(def hb-path (.getCanonicalPath hb-file))
(def checks (atom []))

(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(defn run-bb [expression & {:keys [extra-env]}]
  (let [result
        (p/shell
         {:out :string :err :string :continue true
          :extra-env
          (merge {"NORTH_WORKER_HEARTBEAT" hb-path
                  "HOME" (.getCanonicalPath home)}
                 extra-env)}
         "bb" "-e" expression)]
    {:exit (:exit result)
     :out (str/trim (:out result))
     :err (str/trim (:err result))}))

(defn lib-expression [& body]
  (str "(load-file " (pr-str lib) ") " (apply str body)))

(defn stamp! [instant details]
  (spit hb-file (pr-str {:at (str instant) :details details})))

(try
  (let [result
        (run-bb
         (lib-expression
          "(north.worker-heartbeat/write-heartbeat! \"stale-lanes\" \"7977\" {:reaped 2}) "
          "(let [s (north.worker-heartbeat/heartbeat-status \"stale-lanes\" \"7977\" 60000)] "
          "  (println (pr-str [(:state s) (get-in s [:details :reaped]) (boolean (:ts s))])))"))]
    (check "heartbeat roundtrip preserves worker details"
           (and (zero? (:exit result))
                (= "[:fresh 2 true]" (:out result))
                (.isFile hb-file))
           result))

  (check "atomic write leaves no temporary sibling"
         (not (.exists (io/file (str hb-path ".tmp")))))

  (stamp! (.minusMillis (java.time.Instant/now) 120000) {})
  (let [result
        (run-bb
         (lib-expression
          "(println (name (:state (north.worker-heartbeat/heartbeat-status \"stale-lanes\" \"7977\" 60000))))"))]
    (check "worker-specific threshold classifies an old heartbeat as stale"
           (= "stale" (:out result))
           result))

  (.delete hb-file)
  (let [result
        (run-bb
         (lib-expression
          "(println (name (:state (north.worker-heartbeat/heartbeat-status \"stale-lanes\" \"7977\" 60000))))"))]
    (check "absent worker heartbeat classifies as missing"
           (= "missing" (:out result))
           result))

  (let [render-env {"NORTH_DASHBOARD_LIB" "1"
                    "NORTH_NO_COLOR" "1"
                    "NORTH_HOME" root}
        render
        (fn []
          (run-bb
           (str "(load-file " (pr-str dashboard) ") "
                "(println (maintenance-doctor-line PORT :stale-lanes))")
           :extra-env render-env))]
    (stamp! (java.time.Instant/now) {})
    (let [result (render)]
      (check "doctor renders a fresh task heartbeat"
             (and (str/includes? (:out result) "[ok]")
                  (str/includes? (:out result) "stale-lanes"))
             result))
    (stamp! (.minusMillis (java.time.Instant/now) (* 20 60 1000)) {})
    (let [result (render)]
      (check "doctor renders a stale task heartbeat loudly"
             (and (str/includes? (:out result) "[ERR]")
                  (str/includes? (:out result) "STALE"))
             result))
    (.delete hb-file)
    (let [result (render)]
      (check "doctor renders a missing task heartbeat loudly"
             (and (str/includes? (:out result) "[ERR]")
                  (str/includes? (:out result) "MISSING"))
             result)))

  (let [result
        (run-bb
         (lib-expression
          "(try (north.worker-heartbeat/heartbeat-file \"Bad Name\" \"7977\") "
          "     (catch Throwable _ (println \"rejected\")))"))]
    (check "heartbeat path rejects noncanonical worker names"
           (= "rejected" (:out result))
           result))

  (finally
    (doseq [file (reverse (file-seq home))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when (and (not ok) detail) (println (str "        " detail))))
  (println (format "\nworker heartbeat: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
