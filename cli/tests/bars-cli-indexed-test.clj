#!/usr/bin/env bb
;; bb -cp <store-out> cli/tests/bars-cli-indexed-test.clj
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[store.types :as t])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/bars-cli.clj"))
;; bars-cli.clj loads store-rpc-client.clj lazily (only when a connect actually
;; runs); the test harness needs it loaded up front so with-redefs can stub it.
(load-file (str root "/cli/store-rpc-client.clj"))
(require '[north.store-rpc-client :as rpc])

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(def subject "@019fa000-0000-7000-8000-000000000001")
(def first-bar "Probe A. Expected: pass.")
(def second-bar "Probe B. Expected: pass.")

(defn triples [rows]
  (mapv (fn [[predicate value]] (t/triple subject predicate value)) rows))

(def base-rows
  [["title" "Indexed outcome test"]
   ["done_when" first-bar]
   ["done_when" second-bar]
   ["bar_evidence" (str first-bar " → observed")]
   ["bar_evidence_unreserved" (str "unreserved · " second-bar " → observation")]])

(defn stub-client [_host port space _options]
  {:host "127.0.0.1" :port port :space-id space :closed (atom false)})

(let [calls (atom [])]
  (with-redefs [rpc/connect stub-client
                rpc/close! (fn [_] nil)
                rpc/scan-all! (fn [client subject _ _]
                                (swap! calls conj [(:port client) subject])
                                {:rows (triples base-rows)})]
    (let [echoed (with-out-str (north.bars-cli/cmd-echo (subs subject 1)))
          listed (with-out-str (north.bars-cli/cmd-list (subs subject 1)))]
      (check "outcome echo reads the exact subject through the native scan"
             (= [[7977 subject] [7977 subject]] @calls))
      (check "outcome echo preserves the historical evidence marks"
             (and (str/includes? echoed (str "✓ " first-bar))
                  (str/includes? echoed (str "○ " second-bar))
                  (not (str/includes? echoed (str "~ " second-bar)))))
      (check "full bars list still distinguishes unreserved observations"
             (str/includes? listed (str "~ [")))
      (check "outcome echo retains the evidence command hint"
             (str/includes? echoed
                            (str "evidence: north tell " (subs subject 1)
                                 " bar_evidence")))))
  (with-redefs [rpc/connect stub-client
                rpc/close! (fn [_] nil)
                rpc/scan-all! (fn [_ subject _ _]
                                {:rows (triples [["title" (str "No bars on " subject)]])})]
    (check "bar-less outcome echo is silent"
           (str/blank?
            (with-out-str (north.bars-cli/cmd-echo (subs subject 1)))))))

;; A non-string Term is not a groomable bar fact.
(with-redefs [rpc/connect stub-client
              rpc/close! (fn [_] nil)
              rpc/scan-all! (fn [_ _ _ _]
                              {:rows (conj (triples base-rows)
                                           (t/triple subject :done_when :not-a-string))})]
  (let [listed (with-out-str (north.bars-cli/cmd-list (subs subject 1)))]
    (check "native rows that are not string predicate/value are ignored"
           (and (str/includes? listed first-bar)
                (str/includes? listed "2 bar(s)")))))

;; The prune coupling: retirement goes through the projection layer, never the
;; raw wire retract that would leave an accumulated duplicate occurrence live.
(let [rows (atom base-rows)
      retracted (atom [])]
  (with-redefs [rpc/connect stub-client
                rpc/close! (fn [_] nil)
                rpc/scan-all! (fn [_ _ _ _] {:rows (triples @rows)})
                rpc/retract! (fn [& _]
                               (throw (ex-info "raw wire retract is not the prune path" {})))
                rpc/retract-projected!
                (fn [_ proposition]
                  (swap! retracted conj [(t/triple-t2 proposition)
                                         (t/triple-t3 proposition)])
                  (swap! rows (fn [current]
                                (vec (remove #(= % [(t/triple-t2 proposition)
                                                    (t/triple-t3 proposition)])
                                             current))))
                  {:changed? true})]
    (let [pruned (with-out-str
                   (north.bars-cli/cmd-prune (subs subject 1)
                                             {:bars [] :dry-run false}))]
      (check "prune retires the evidenced bar through the projection layer"
             (= [["done_when" first-bar]] @retracted))
      (check "prune reports the retirement and re-reads the exact projection"
             (and (str/includes? pruned (str "retired ✓ " first-bar))
                  (str/includes? pruned "retired 1 bar(s)")
                  (str/includes? pruned "1 bar(s) · limit 32")))))
  (reset! rows base-rows)
  (reset! retracted [])
  (with-redefs [rpc/connect stub-client
                rpc/close! (fn [_] nil)
                rpc/scan-all! (fn [_ _ _ _] {:rows (triples @rows)})
                rpc/retract-projected! (fn [& _] (swap! retracted conj :called))]
    (let [dry (with-out-str
                (north.bars-cli/cmd-prune (subs subject 1)
                                          {:bars [] :dry-run true}))]
      (check "dry-run prune writes nothing"
             (and (empty? @retracted)
                  (str/includes? dry (str "would retire ✓ " first-bar)))))))

(let [entrypoint (slurp (str root "/bin/north"))]
  (check "real outcome entrypoint uses the indexed bars echo"
         (and (str/includes? entrypoint "\"${NORTH_BARS[@]}\" echo \"$2\"")
              (not (str/includes? entrypoint
                                  "\"${NORTH_MAIN[@]}\" done-bars \"$2\""))))
  (check "the bars entrypoint carries the Beagle Store classpath the native wire needs"
         (str/includes? entrypoint
                        "NORTH_BARS=(\"$BB\" -cp \"$NORTH_RUNTIME_CLASSPATH\" \"$NORTH/cli/bars-cli.clj\")")))

(doseq [[label ok?] @checks]
  (println (str (if ok? "PASS " "FAIL ") label)))

(when-not (every? second @checks)
  (System/exit 1))
