#!/usr/bin/env bb
(require '[babashka.process]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def worker-host (str root "/cli/coordination-projection-worker-host.clj"))
(def checks (atom []))

(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(System/setProperty "babashka.file" worker-host)
(System/setProperty "north.coordination-projection-worker-host.lib" "1")
(load-file worker-host)
(alias 'worker 'north.coordination-projection-worker-host)

(let [call (atom nil)]
  (with-redefs [babashka.process/shell
                (fn [options & command]
                  (reset! call {:options options :command command})
                  {:exit 0 :out "" :err ""})
                worker/north-bin "/fixture/north"
                north.coord/expected-log (constantly "/fixture/coordination.framlog")]
    (worker/heal!))
  (check "auto-heal selects only the coordination database"
         (= {"FRAM_LOG" "/fixture/coordination.framlog"
             "NORTH_TELEMETRY_PARTITION" "0"
             "FRAM_TELEMETRY_LOG" ""}
            (get-in @call [:options :extra-env]))
         @call)
  (check "auto-heal still invokes the canonical heal verb"
         (= ["/fixture/north" "heal"] (:command @call))
         @call))

(with-redefs [worker/triple-values identity]
  (let [durable ["@thread" "title" "Bridge"]
        outcome ["@thread" "outcome" "landed"]
        projection
        (worker/tracked-projection
         [durable durable outcome
          ["@lease:worker" "holder" "agent"]
          ["@session:worker" "status" "active"]])]
    (check "FRAMRPC scan retains duplicate-sensitive durable facts"
           (= {"@thread" {durable 2 outcome 1}} projection)
           projection)
    (check "FRAMRPC scan omits ephemeral subjects"
           (= #{"@thread"} (set (keys projection)))
           projection)))

(let [scans (atom 0)
      previous {:version 7 :projection {"@thread" {}}}]
  (reset! worker/dirty false)
  (with-redefs [north.framrpc-client/version!
                (fn [_] {:served-version 7})
                worker/observation!
                (fn [_]
                  (swap! scans inc)
                  {:version 7 :projection {}})]
    (let [observed (worker/poll-once! :client previous)]
      (check "unchanged FRAMRPC version avoids a corpus scan"
             (and (= previous observed) (zero? @scans))
             {:observed observed :scans @scans}))))

(let [previous {:version 8 :projection {}}
      durable ["@thread" "title" "Bridge"]]
  (reset! worker/dirty false)
  (with-redefs [north.framrpc-client/version!
                (fn [_] {:served-version 9})
                worker/observation!
                (fn [_]
                  {:version 9 :projection {"@thread" {durable 1}}})]
    (let [observed (worker/poll-once! :client previous)]
      (check "durable FRAMRPC projection change enters the existing debounce"
             (and (= 9 (:version observed))
                  (true? @worker/dirty))
             {:observed observed :dirty @worker/dirty}))))

(let [source (slurp worker-host)]
  (check "worker names the canonical Fram connection contract"
         (and (str/includes? source "FRAM_SERVER_CONNECT")
              (str/includes? source "FRAM_SERVER_PORT")
              (str/includes? source "FRAM_OUT"))
         nil))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\ncoordination projection worker: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
