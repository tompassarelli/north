#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def reactor
  (or (System/getenv "NORTH_TEST_REACTOR")
      (str root "/cli/north-reactor.clj")))
(def checks (atom []))

(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(System/setProperty "babashka.file" reactor)
(load-string
 (-> (slurp reactor)
     (str/replace-first #"^#![^\n]*\n" "")
     (str/replace
      #"\n\(if sweep-verb\?\n  \(System/exit \(sweep-once-exit-code\)\)\n  \(-main\)\)\s*$"
      "\n")))

(let [call (atom nil)]
  (with-redefs [proc/shell
                (fn [options & command]
                  (reset! call {:options options :command command})
                  {:exit 0 :out "" :err ""})
                north-bin "/fixture/north"
                north.coord/expected-log (constantly "/fixture/coordination.log")]
    (heal!))
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
  (println (format "\nreactor heal projection: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
