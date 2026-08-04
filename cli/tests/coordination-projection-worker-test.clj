#!/usr/bin/env bb
(require '[babashka.process]
         '[clojure.java.io :as io])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def worker-host (str root "/cli/coordination-projection-worker-host.clj"))
(def checks (atom []))

(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(System/setProperty "babashka.file" worker-host)
(System/setProperty "north.coordination-projection-worker-host.lib" "1")
(load-file worker-host)

(let [call (atom nil)]
  (with-redefs [babashka.process/shell
                (fn [options & command]
                  (reset! call {:options options :command command})
                  {:exit 0 :out "" :err ""})
                north.coordination-projection-worker-host/north-bin
                "/fixture/north"
                north.coord/expected-log (constantly "/fixture/coordination.log")]
    (north.coordination-projection-worker-host/heal!))
  (check "auto-heal selects only the coordination corpus"
         (= {"FRAM_LOG" "/fixture/coordination.log"
             "NORTH_TELEMETRY_PARTITION" "0"
             "FRAM_TELEMETRY_LOG" ""}
            (get-in @call [:options :extra-env]))
         @call)
  (check "auto-heal still invokes the canonical heal verb"
         (= ["/fixture/north" "heal"] (:command @call))
         @call))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\ncoordination projection worker: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
