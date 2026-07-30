#!/usr/bin/env bb
;; FRAM_LOG selects WHICH CORPUS. FRAM_TELEMETRY_LOG only says where telemetry
;; goes and must never veto that selection: every direct-bb client (the presence
;; hooks) fences on this answer, and a wrong one is rejected :log-mismatch.
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def root (-> test-script io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(def coord (str root "/cli/coord.clj"))

(def tmp (str (java.nio.file.Files/createTempDirectory
               "north-corpus-selection"
               (into-array java.nio.file.attribute.FileAttribute []))))
(def state (str tmp "/.local/state/north"))
(.mkdirs (io/file state))
(spit (str state "/facts.log") "")

(defn selected
  "env(1) requires every -u before any NAME=VALUE, so build the two groups apart."
  [{:keys [unset set]}]
  (let [argv (concat ["env"]
                     (mapcat (fn [n] ["-u" n]) unset)
                     (map (fn [[k v]] (str k "=" v)) set)
                     ["bb" "-e" (str "(load-file \"" coord "\") "
                                     "(print (north.coord/expected-log))")])
        result @(p/process (vec argv) {:out :string :err :string})]
    (str/trim (str (:out result)))))

(def checks (atom []))
(defn check [label ok]
  (swap! checks conj [label ok])
  (println (if ok (str "PASS " label) (str "FAIL " label))))

(let [facts (str state "/facts.log")
      split (str state "/coordination.log")]
  (check "with no split log present, the default corpus is facts.log"
         (= facts (selected {:unset ["FRAM_LOG" "FRAM_TELEMETRY_LOG"]
                             :set {"HOME" tmp}})))

  (spit split "")

  (check "a present split log is selected over facts.log"
         (= split (selected {:unset ["FRAM_LOG" "FRAM_TELEMETRY_LOG"]
                             :set {"HOME" tmp}})))

  (check "FRAM_TELEMETRY_LOG does not veto split-log corpus selection"
         (= split (selected {:unset ["FRAM_LOG"]
                             :set {"HOME" tmp
                                   "FRAM_TELEMETRY_LOG" (str state "/telemetry.log")}})))

  (check "a partitioned deployment still selects the split log"
         (= split (selected {:unset ["FRAM_LOG"]
                             :set {"HOME" tmp
                                   "NORTH_TELEMETRY_PARTITION" "1"
                                   "NORTH_TELEMETRY_PORT" "7978"
                                   "FRAM_TELEMETRY_LOG" (str state "/telemetry.log")}})))

  (check "an explicitly pinned FRAM_LOG still wins"
         (= facts (selected {:unset []
                             :set {"HOME" tmp
                                   "FRAM_LOG" facts
                                   "FRAM_TELEMETRY_LOG" (str state "/telemetry.log")}}))))

(let [failed (remove second @checks)]
  (println (str "coord corpus selection: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
