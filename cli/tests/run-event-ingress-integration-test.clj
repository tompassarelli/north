#!/usr/bin/env bb
;; Store-backed proof that wire-event ingress can resume committed event suffixes.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(def store
  (.getCanonicalPath
   (io/file (or (System/getenv "BEAGLE_STORE_PATH")
                "/home/tom/code/beagle/main/store"))))
(when-not (.isFile (io/file store "bin/beagle-store-server"))
  (throw (ex-info "current Beagle Store engine is required" {:store store})))
(def writer (str root "/cli/run-event-internal.clj"))
(def generator
  (str "import { WireEventWriter, wireEventId } from "
       (pr-str (str root "/sdk/src/wire/index.ts")) ";"
       "import { wireEventFacts } from " (pr-str (str root "/sdk/src/run-ledger.ts")) ";"
       "let tick=0;const runId='run:event-ingress-suffix';"
       "const writer=new WireEventWriter({runId,eventId:(sequence)=>wireEventId(`event:ingress:${sequence}`),"
       "now:()=>new Date(Date.UTC(2026,0,1,0,0,tick++)).toISOString()});"
       "writer.append({kind:'run.started',lifecycle:'running',owner:'event-ingress'});"
       "writer.append({kind:'run.progress',lifecycle:'running',progress:{currentAction:'first suffix',compactions:0}});"
       "writer.append({kind:'run.progress',lifecycle:'running',progress:{currentAction:'terminal suffix',compactions:0}});"
       "writer.terminate({lifecycle:'completed',reason:{code:'completed'}});"
       "const identity={thread:'@thread-event-ingress',agent:'event-ingress',coordinator:'root'};"
       "const events=writer.events();const projections=events.map((event)=>wireEventFacts(identity,event));"
       "const after={...events[1],sequence:4,id:wireEventId('event:ingress:4'),at:'2026-01-01T00:00:04.000Z'};"
       "console.log(JSON.stringify({prefix:projections.slice(0,1),middle:projections.slice(1,2),terminal:projections.slice(2),after:wireEventFacts(identity,after)}));"))

(load-file (str root "/cli/coord.clj"))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn eventually [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 100) false
          :else (do (Thread/sleep 25) (recur (inc attempt))))))

(defn run-writer [port log payload]
  (proc/shell {:out :string :err :string :continue true :in (json/generate-string payload)
               :extra-env {"BEAGLE_STORE_LOG" log}}
              "bb" "-cp" (str store "/out") writer (str port)))

(def checks (atom []))
(defn check! [label result] (swap! checks conj [label (boolean result)]))

(let [port (free-port)
      directory (.toFile (java.nio.file.Files/createTempDirectory
                          "north-event-ingress" (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file directory "coordination.storelog"))
      daemon (proc/process {:dir store :out :string :err :string
                            :extra-env {"BEAGLE_STORE_LOG" log
                                        "BEAGLE_STORE_SERVER_QUIET" "1"
                                        "BEAGLE_STORE_SERVER_XMX" "1g"}}
                           (str store "/bin/beagle-store-server") "serve" (str port) log "north-coordination")]
  (alter-var-root #'north.coord/expected-log (constantly (fn [] log)))
  (try
    (check! "sealed Beagle Store starts"
            (eventually #(try (= :ready (:state (north.coord/status port)))
                              (catch Exception _ false))))
    (let [generated (proc/shell {:out :string :err :string :continue true} "bun" "-e" generator)
          payloads (when (zero? (:exit generated)) (json/parse-string (:out generated)))
          prefix (get payloads "prefix")
          middle (get payloads "middle")
          terminal (get payloads "terminal")
          after [(get payloads "after")]
          prefix-result (run-writer port log prefix)
          before-middle (north.coord/version port)
          middle-result (run-writer port log middle)
          before-terminal (north.coord/version port)
          terminal-result (run-writer port log terminal)
          committed (north.coord/version port)
          retry-result (run-writer port log terminal)
          conflict (update-in terminal [0 "facts"]
                              (fn [facts]
                                (mapv (fn [[predicate value]]
                                        [predicate (if (= predicate "thread") "@thread-conflict" value)])
                                      facts)))
          conflict-result (run-writer port log conflict)
          after-terminal-result (run-writer port log after)]
      (check! "fixture emits canonical projections" (and payloads (zero? (:exit generated))))
      (check! "first nonterminal event is admitted" (zero? (:exit prefix-result)))
      (check! "committed-predecessor nonterminal suffix is admitted"
              (and (zero? (:exit middle-result)) (= (inc before-middle) before-terminal)))
      (check! "committed-predecessor terminal suffix is admitted" (zero? (:exit terminal-result)))
      (check! "exact suffix retry is idempotent"
              (and (zero? (:exit retry-result)) (= committed (north.coord/version port))))
      (check! "conflicting suffix is rejected without mutation"
              (and (not (zero? (:exit conflict-result))) (= committed (north.coord/version port))))
      (check! "append after committed terminal is rejected without mutation"
              (and (not (zero? (:exit after-terminal-result))) (= committed (north.coord/version port)))))
    (finally
      (proc/destroy-tree daemon)
      (try @daemon (catch Exception _ nil))
      (doseq [[label passed?] @checks]
        (println (format "  [%s] %s" (if passed? "PASS" "FAIL") label)))
      (let [failed (remove second @checks)]
        (println (format "\n%d/%d passed" (- (count @checks) (count failed)) (count @checks)))
        (when (seq failed) (System/exit 1))))))
