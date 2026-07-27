#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))

(when-not (= "1" (System/getenv "NORTH_LISTEN_LIB"))
  (let [result @(proc/process
                 ["env" "NORTH_LISTEN_LIB=1" "bb" test-script]
                 {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file test-script)) "../..")))
(let [source (str root "/cli/north-listen.clj")]
  (System/setProperty "babashka.file" source)
  (try
    (load-file source)
    (finally
      (System/setProperty "babashka.file" test-script))))

(def checks (atom []))
(defn check [label value]
  (swap! checks conj [label (boolean value)])
  (println (if value (str "PASS " label) (str "FAIL " label))))

(let [passes (atom [{:reason :unavailable :message "connection refused"}
                    {:reason :closed :message "restart EOF"}
                    {:reason :stop :message "re-armed"}])
      sleeps (atom [])
      notices (atom [])
      result
      (run-with-reconnect!
       #(let [value (first @passes)]
          (swap! passes subvec 1)
          value)
       #(swap! sleeps conj %)
       #(swap! notices conj [%1 %2]))]
  (check "restart sequence reaches the re-armed pass instead of exiting"
         (= {:reason :stop :message "re-armed"} result))
  (check "restart failures back off exponentially"
         (= [250 500] @sleeps))
  (check "each interruption is surfaced before retry"
         (= ["connection refused" "restart EOF"]
            (mapv (comp :message first) @notices))))

(let [result
      (proc/shell
       {:continue true :out :string :err :string}
       "timeout" "--signal=TERM" "--kill-after=0.1s" "0.15s"
       "env" "-u" "NORTH_LISTEN_LIB"
       "NORTH_LISTEN_INITIAL_BACKOFF_MS=10"
       "NORTH_LISTEN_MAX_BACKOFF_MS=20"
       "bb" (str root "/cli/north-listen.clj") "59999" "restart-probe")
      diagnostics (str (:out result) "\n" (:err result))]
  (check "connection refusal is transient and listener remains running"
         (= 124 (:exit result)))
  (check "connection refusal retries loudly with bounded backoff"
         (and (str/includes? diagnostics "reconnecting in 10ms")
              (str/includes? diagnostics "reconnecting in 20ms"))))

(let [failed (remove second @checks)]
  (println (str "north listen reconnect: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
