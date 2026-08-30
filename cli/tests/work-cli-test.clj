(ns north.work-cli-test
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/work-cli.clj"))

(defn work-var [name]
  (or (ns-resolve 'north.work-cli name) (throw (ex-info "missing typed work CLI var" {:symbol name}))))

(def parse-command! (work-var 'parse-command!))

(def command-argument! (work-var 'command-argument!))

(def invoke-command! (work-var 'invoke-command!))

(def render-command-json! (work-var 'render-command-json!))

(def runtime-constructor (work-var '->CommandRuntime))

(def ^String usage-text (var-get (work-var 'usage-text)))

(def ^String ownership-transition-json "{\"protocol\":\"work-ownership-v1\",\"action\":\"accept\"}")

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj (->Check label (boolean value)))
  nil))

(defn denied-type [operation]
  (try
  (do
  (operation)
  nil)
  (catch Throwable error
    (:type (ex-data error)))))

(defn parsed-arguments! [argv]
  (:arguments (parse-command! argv)))

(def command-cases [["track" ["track" "Ship Bridge" "--tracked-by" "@actor:tom" "--json"] {:title "Ship Bridge" :tracked-by "@actor:tom"}] ["plan" ["plan" "@referent:r1" "--path" "cut over" "--endorsed-by" "@actor:tom" "--json"] {:referent "@referent:r1" :path "cut over" :endorsed-by "@actor:tom"}] ["start" ["start" "@referent:r1" "--revision" "@revision:v1" "--authorized-by" "@actor:tom" "--signature" "signed:r1:v1" "--json"] {:referent "@referent:r1" :revision "@revision:v1" :authorized-by "@actor:tom" :signature "signed:r1:v1"}] ["assign" ["assign" "@referent:r1" "--to" "@actor:worker" "--assigned-by" "@actor:tom" "--json"] {:referent "@referent:r1" :to "@actor:worker" :assigned-by "@actor:tom"}] ["request" ["request" "@referent:r1" "--from" "@actor:tom" "--to" "@actor:worker" "--body" "send evidence" "--json"] {:about "@referent:r1" :from "@actor:tom" :to "@actor:worker" :body "send evidence"}] ["request" ["request" "--from" "@actor:tom" "--to" "@actor:worker" "--body" "send evidence" "--json"] {:from "@actor:tom" :to "@actor:worker" :body "send evidence"}] ["ack" ["ack" "@request:q1" "--by" "@actor:worker" "--json"] {:request "@request:q1" :by "@actor:worker"}] ["result" ["result" "@request:q1" "--result" "@result:r1" "--reported-by" "@actor:worker" "--outcome" "done" "--summary" "shipped" "--json"] {:request "@request:q1" :result "@result:r1" :reported-by "@actor:worker" :outcome "done" :summary "shipped"}] ["ownership" ["ownership" "--transition" ownership-transition-json "--json"] {:transition ownership-transition-json}] ["settle" ["settle" "@assignment:a1" "--transition" "@transition:t1" "--by" "@actor:worker" "--outcome" "done" "--summary" "shipped" "--json"] {:assignment "@assignment:a1" :accepted-transition "@transition:t1" :by "@actor:worker" :outcome "done" :summary "shipped"}] ["show" ["show" "@referent:r1" "--json"] {:referent "@referent:r1"}] ["history" ["history" "@referent:r1" "--json"] {:referent "@referent:r1"}] ["inbox" ["inbox" "@actor:worker" "--json"] {:actor "@actor:worker"}] ["catalog" ["catalog" "--json"] {}]])

(check! "every frozen tracked-thing argv parses to exact named values" (every? true? (mapv (fn [[action argv expected]] (let [parsed (parse-command! argv)]
  (and (= action (:action parsed)) (= expected (:arguments parsed))))) command-cases)))

(check! "option order does not change the parsed semantic command" (= {:referent "@referent:r1" :to "@actor:worker" :assigned-by "@actor:tom"} (parsed-arguments! ["assign" "@referent:r1" "--assigned-by" "@actor:tom" "--json" "--to" "@actor:worker"])))

(check! "the stable protocol rejects missing or duplicated JSON selection" (and (= :missing-json-protocol (denied-type (fn [] (parse-command! ["show" "@referent:r1"])))) (= :duplicate-json-protocol (denied-type (fn [] (parse-command! ["show" "@referent:r1" "--json" "--json"]))))))

(check! "unknown, duplicated, missing, and misplaced command values fail closed" (and (= :unknown-work-command (denied-type (fn [] (parse-command! ["bogus" "x" "--json"])))) (= :unknown-work-command-option (denied-type (fn [] (parse-command! ["show" "@referent:r1" "--bogus" "x" "--json"])))) (= :duplicate-work-command-value (denied-type (fn [] (parse-command! ["ack" "@request:q1" "--by" "@actor:a" "--by" "@actor:b" "--json"])))) (= :missing-work-command-options (denied-type (fn [] (parse-command! ["track" "Ship" "--json"])))) (= :misplaced-work-command-value (denied-type (fn [] (parse-command! ["show" "--json" "@referent:r1"]))))))

(check! "blank, padded, and NUL-bearing values fail before invocation" (every? (fn [^String title] (= :invalid-work-command-value (denied-type (fn [] (parse-command! ["track" title "--tracked-by" "@actor:tom" "--json"]))))) ["" " Ship" "Ship " (str "Ship" "\u0000" "Bridge")]))

(def observed-pipeline (atom []))

(def observed-reads (atom []))

(def runtime (runtime-constructor (fn [port command] (do
  (swap! observed-pipeline conj [:observe port (:action command)])
  {:store-version (if (= "track" (:action command)) 91 90)})) (fn [command observation] (let [store-version (:store-version observation)]
  (swap! observed-pipeline conj [:plan (:action command) store-version])
  {:command command :actions [{:action (:action command)}] :expected-version store-version})) (fn [plan] (let [actions (:actions plan)]
  (swap! observed-pipeline conj [:actions actions])
  actions)) (fn [plan] (let [expected-version (:expected-version plan)]
  (swap! observed-pipeline conj [:version expected-version])
  expected-version)) (fn [port actions expected-version] (do
  (swap! observed-pipeline conj [:transact port actions expected-version])
  {:storeVersion (inc expected-version)})) (fn [command _plan committed] (let [store-version (:storeVersion committed)]
  (swap! observed-pipeline conj [:receipt (:action command) store-version])
  (case (:action command)
    "ack" {:protocol "north.semantic-receipt" :version 1 :action "ack" :storeVersion store-version :request "@request:q1" :ack "@ack:k1"}
    "track" {:protocol "north.semantic-receipt" :version 1 :action "track" :storeVersion store-version :referent "@referent:r1"}
    (throw (ex-info "unexpected mutation" {}))))) (fn [command _observation] (do
  (swap! observed-reads conj (:action command))
  (case (:action command)
    "show" {:protocol "north.semantic-view" :version 1 :referent "@referent:r1" :facts [] :derived []}
    "history" {:protocol "north.semantic-history" :version 1 :referent "@referent:r1" :occurrences []}
    "inbox" {:protocol "north.semantic-inbox" :version 1 :actor "@actor:worker" :requests []}
    (throw (ex-info "unexpected read" {})))))))

(defn fixed-runtime [mutation-receipt read-receipt]
  (runtime-constructor (fn [_port _command] {:store-version 0}) (fn [command _observation] {:command command :actions [] :expected-version 0}) (fn [_plan] []) (fn [_plan] 0) (fn [_port _actions _expected-version] {:storeVersion 1}) (fn [_command _plan _committed] mutation-receipt) (fn [_command _observation] read-receipt)))

(let [command (parse-command! ["ack" "@request:q1" "--by" "@actor:worker" "--json"])
   receipt (invoke-command! runtime 7977 command)]
  (check! "mutation invocation preserves the fenced semantic pipeline and receipt" (and (= 91 (:storeVersion receipt)) (= "@ack:k1" (:ack receipt)) (= [:observe :plan :actions :version :transact :receipt] (mapv first (deref observed-pipeline))))))

(check! "ACK receipts cannot smuggle an ownership transition or Assignment" (let [command (parse-command! ["ack" "@request:q1" "--by" "@actor:worker" "--json"])
   bad-runtime (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "ack" :storeVersion 91 :request "@request:q1" :ack "@ack:k1" :acceptedTransition "@transition:t1"} {})]
  (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! bad-runtime 7977 command))))))

(check! "Request receipt referent is present exactly when about is present" (let [about-command (parse-command! ["request" "@referent:r1" "--from" "@actor:tom" "--to" "@actor:worker" "--body" "send evidence" "--json"])
   no-about-command (parse-command! ["request" "--from" "@actor:tom" "--to" "@actor:worker" "--body" "send evidence" "--json"])
   with-about (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "request" :storeVersion 3 :request "@request:q1" :referent "@referent:r1"} {})
   without-about (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "request" :storeVersion 4 :request "@request:q2"} {})
   missing-referent (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "request" :storeVersion 3 :request "@request:q1"} {})
   extra-referent (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "request" :storeVersion 4 :request "@request:q2" :referent "@referent:r1"} {})]
  (and (= "@referent:r1" (:referent (invoke-command! with-about 7977 about-command))) (= "@request:q2" (:request (invoke-command! without-about 7977 no-about-command))) (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! missing-referent 7977 about-command)))) (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! extra-referent 7977 no-about-command)))))))

(check! "ownership receipts expose only the frozen transition and owner keys" (let [command (parse-command! ["ownership" "--transition" ownership-transition-json "--json"])
   receipt {:protocol "north.semantic-receipt" :version 1 :action "ownership" :storeVersion 5 :transition "@transition:t1" :owner "@actor:worker"}
   valid-runtime (fixed-runtime receipt {})
   extra-runtime (fixed-runtime (assoc receipt :assignment "@assignment:a1") {})]
  (and (= receipt (invoke-command! valid-runtime 7977 command)) (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! extra-runtime 7977 command)))))))

(check! "Result receipts admit only the snapshot-derived optional referent" (let [command (parse-command! ["result" "@request:q1" "--result" "@result:r1" "--reported-by" "@actor:worker" "--outcome" "done" "--summary" "shipped" "--json"])
   unscoped {:protocol "north.semantic-receipt" :version 1 :action "result" :storeVersion 6 :request "@request:q1" :result "@result:r1" :outcome "done"}
   scoped (assoc unscoped :referent "@referent:r1")
   extra (assoc unscoped :assignment "@assignment:a1")]
  (and (= unscoped (invoke-command! (fixed-runtime unscoped {}) 7977 command)) (= scoped (invoke-command! (fixed-runtime scoped {}) 7977 command)) (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! (fixed-runtime extra {}) 7977 command)))))))

(check! "mutation receipt requires the exact protocol and nonnegative Store version" (let [command (parse-command! ["track" "Ship" "--tracked-by" "@actor:tom" "--json"])
   wrong-protocol (fixed-runtime {:protocol "north.semantic-view" :version 1 :action "track" :storeVersion 1 :referent "@referent:r1"} {})
   wrong-version (fixed-runtime {:protocol "north.semantic-receipt" :version 1 :action "track" :storeVersion -1 :referent "@referent:r1"} {})]
  (and (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! wrong-protocol 7977 command)))) (= :invalid-semantic-receipt (denied-type (fn [] (invoke-command! wrong-version 7977 command)))))))

(check! "all three semantic reads validate their exact committed shapes" (let [show (invoke-command! runtime 7977 (parse-command! ["show" "@referent:r1" "--json"]))
   history (invoke-command! runtime 7977 (parse-command! ["history" "@referent:r1" "--json"]))
   inbox (invoke-command! runtime 7977 (parse-command! ["inbox" "@actor:worker" "--json"]))]
  (and (= "north.semantic-view" (:protocol show)) (= "north.semantic-history" (:protocol history)) (= "north.semantic-inbox" (:protocol inbox)) (= 3 (count (deref observed-reads))))))

(check! "read views reject extra transient state" (let [command (parse-command! ["show" "@referent:r1" "--json"])
   bad-runtime (fixed-runtime {} {:protocol "north.semantic-view" :version 1 :referent "@referent:r1" :facts [] :derived [] :memory-cache true})]
  (= :invalid-semantic-read (denied-type (fn [] (invoke-command! bad-runtime 7977 command))))))

(check! "catalog validates the exact deterministic backend envelope" (let [command (parse-command! ["catalog" "--json"])
   envelope {:protocol "north.semantic-catalog" :version 1 :storeSpace "north-coordination" :storeVersion 7 :trackedThings [{:id "@referent:a" :title "A" :desiredOutcome nil :agent false :plan true :project false :task false :assignee nil :assigneeTitle nil :status nil} {:id "@referent:b" :title "B" :desiredOutcome "Ship" :agent true :plan false :project false :task false :assignee "@actor:b" :assigneeTitle "B" :status "active"}]}
   valid-runtime (fixed-runtime {} envelope)
   reversed-runtime (fixed-runtime {} (update envelope :trackedThings (fn [rows] (vec (reverse rows)))))]
  (and (= envelope (invoke-command! valid-runtime 7977 command)) (= :invalid-semantic-catalog (denied-type (fn [] (invoke-command! reversed-runtime 7977 command)))))))

(check! "rendered mutation JSON exposes the Bridge receipt keys exactly" (let [rendered (render-command-json! runtime 7977 ["track" "Ship" "--tracked-by" "@actor:tom" "--json"])
   parsed (json/parse-string rendered true)]
  (and (str/ends-with? rendered "\n") (= #{:protocol :version :action :storeVersion :referent} (set (keys parsed))) (= 92 (:storeVersion parsed)))))

(check! "usage exposes only the frozen three-view product vocabulary" (and (str/includes? usage-text "views: Agents | Goals | All") (str/includes? usage-text "tracked things") (str/includes? usage-text "request [TRACKED-THING]") (str/includes? usage-text "result REQUEST --result") (str/includes? usage-text "ownership --transition") (str/includes? usage-text "catalog --json") (not (str/includes? usage-text "referent-based control plane"))))

(let [results (deref checks)
   passed (count (filter (fn [^Check result] (check-passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (check-passed result) "PASS" "FAIL") (check-label result))))
  (println (format "\nTracked-thing semantic CLI: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
