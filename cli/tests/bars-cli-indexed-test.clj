#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/bars-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(let [subject "@019fa000-0000-7000-8000-000000000001"
      first-bar "Probe A. Expected: pass."
      second-bar "Probe B. Expected: pass."
      calls (atom [])
      rows [["title" "Indexed outcome test"]
            ["done_when" first-bar]
            ["done_when" second-bar]
            ["bar_evidence" (str first-bar " → observed")]
            ["bar_evidence_unreserved" (str "unreserved · " second-bar
                                             " → observation")]]]
  (with-redefs [north.coord/show-envelope
                (fn [port entity]
                  (swap! calls conj [port entity])
                  {:version 42 :rows rows})]
    (let [echoed (with-out-str (north.bars-cli/cmd-echo (subs subject 1)))
          listed (with-out-str (north.bars-cli/cmd-list (subs subject 1)))]
      (check "outcome echo reads the exact subject through show-envelope"
             (= [[7977 subject] [7977 subject]] @calls))
      (check "outcome echo preserves the historical evidence marks"
             (and (str/includes? echoed (str "✓ " first-bar))
                  (str/includes? echoed (str "○ " second-bar))
                  (not (str/includes? echoed (str "~ " second-bar)))))
      (check "full bars list still distinguishes unreserved observations"
             (str/includes? listed (str "~ [" )))
      (check "outcome echo retains the evidence command hint"
             (str/includes? echoed
                            (str "evidence: north tell " (subs subject 1)
                                 " bar_evidence")))))
  (with-redefs [north.coord/show-envelope
                (fn [_ entity] {:version 43
                                :rows [["title" (str "No bars on " entity)]]})]
    (check "bar-less outcome echo is silent"
           (str/blank?
            (with-out-str (north.bars-cli/cmd-echo (subs subject 1)))))))

(let [entrypoint (slurp (str root "/bin/north"))]
  (check "real outcome entrypoint uses the indexed bars echo"
         (and (str/includes? entrypoint
                             "\"$NORTH/cli/bars-cli.clj\" echo \"$2\"")
              (not (str/includes? entrypoint
                                  "\"${NORTH_MAIN[@]}\" done-bars \"$2\"")))))

(doseq [[label ok?] @checks]
  (println (str (if ok? "PASS " "FAIL ") label)))

(when-not (every? second @checks)
  (System/exit 1))
