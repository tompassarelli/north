(ns north.work-catalog-test
  (:require [clojure.java.io :as io]
            [north.projections :as proj]
            [north.referents :as referents]
            [store.types :as t]))

(def test-file (.getCanonicalPath (io/file *file*)))

(def tests-dir (.getParentFile (io/file test-file)))

(def cli-dir (.getParentFile tests-dir))

(load-file (str cli-dir "/coord.clj"))

(load-file (str cli-dir "/work-catalog.clj"))

(def live-facts-view-var (or (ns-resolve (symbol "north.coord") (symbol "live-facts-view!")) (throw (ex-info "north.coord/live-facts-view! is unavailable" {:type :missing-test-var}))))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def checks (atom []))

(defn check! [^String label passed]
  (do
  (swap! checks conj (->Check label (boolean passed)))
  nil))

(defn denied-type [operation]
  (try
  (do
  (operation)
  nil)
  (catch Throwable error
    (:type (ex-data error)))))

(def ^String tracker "@agent:tracker")

(def ^String worker "@agent:worker")

(def ^String goal "@tracked:goal")

(def ^String plan "@tracked:plan")

(def ^String plain "@tracked:plain")

(def ^String revision "@occurrence:revision")

(def ^String authorization "@occurrence:authorization")

(def ^String assignment "@occurrence:assignment")

(def ^String request "@occurrence:request")

(def catalog-facts (vec (concat (referents/tracked-thing-facts! tracker "Tracker" tracker "2026-08-30T10:00:00Z") (referents/agent-role-facts! tracker) (referents/tracked-thing-facts! worker "Worker" tracker "2026-08-30T10:00:01Z") (referents/agent-role-facts! worker) (referents/tracked-thing-facts! goal "Goal" tracker "2026-08-30T10:00:02Z") (referents/desired-outcome-facts! goal "Ship the catalog") (referents/tracked-thing-facts! plan "Plan" tracker "2026-08-30T10:00:03Z") (referents/plan-revision-facts! plan revision "Implement the catalog" tracker "2026-08-30T10:01:00Z") (referents/project-start-authorization-facts! authorization plan revision tracker "sig:accepted" "2026-08-30T10:02:00Z") (referents/assignment-facts! assignment plan tracker worker "2026-08-30T10:03:00Z") (referents/tracked-thing-facts! plain "Plain" tracker "2026-08-30T10:00:04Z") (referents/occurrence-facts! request "request" tracker "2026-08-30T10:04:00Z") (referents/occurrence-about-facts! request plan))))

(def envelope (north.work-catalog/catalog-envelope "catalog-test" 42 catalog-facts))

(def rows (:trackedThings envelope))

(def rows-by-id (into {} (map (fn [row] [(:id row) row]) rows)))

(def plan-row (get rows-by-id plan))

(def goal-row (get rows-by-id goal))

(def plain-row (get rows-by-id plain))

(check! "catalog envelope uses the exact protocol keys" (= #{:protocol :version :storeSpace :storeVersion :trackedThings} (set (keys envelope))))

(check! "catalog envelope binds one Store space and committed version" (and (= "north.semantic-catalog" (:protocol envelope)) (= 1 (:version envelope)) (= "catalog-test" (:storeSpace envelope)) (= 42 (:storeVersion envelope))))

(check! "tracked rows use the exact public key set" (every? (fn [row] (= #{:id :title :desiredOutcome :agent :plan :project :task :assignee :assigneeTitle :status} (set (keys row)))) rows))

(check! "every tracked identity appears once in deterministic order" (= (vec (sort [tracker worker goal plan plain])) (mapv (fn [row] (:id row)) rows)))

(check! "occurrence identities never appear as catalog rows" (empty? (filterv (fn [^String id] (contains? #{revision authorization assignment request} id)) (mapv (fn [row] (:id row)) rows))))

(check! "Goal derives only from a nonblank desired outcome" (and (= "Ship the catalog" (:desiredOutcome goal-row)) (= false (:plan goal-row)) (= nil (:desiredOutcome plain-row))))

(check! "Plan, Project, Task, and Assignment stay on one tracked identity" (and (= true (:plan plan-row)) (= true (:project plan-row)) (= true (:task plan-row)) (= worker (:assignee plan-row)) (= "Worker" (:assigneeTitle plan-row)) (= nil (:status plan-row))))

(check! "Agents are tracked identities playing the agent role" (and (= true (:agent (get rows-by-id tracker))) (= true (:agent (get rows-by-id worker))) (= false (:agent plan-row))))

(check! "blank Store space is rejected" (= :north/invalid-semantic-catalog (denied-type (fn [] (north.work-catalog/catalog-envelope "" 42 catalog-facts)))))

(check! "negative Store version is rejected" (= :north/invalid-semantic-catalog (denied-type (fn [] (north.work-catalog/catalog-envelope "catalog-test" -1 catalog-facts)))))

(def missing-title-facts (filterv (fn [fact] (not (and (= plain (t/triple-t1 fact)) (= "title" (t/triple-t2 fact))))) catalog-facts))

(check! "a tracked thing without one display title is rejected" (= :north/invalid-semantic-catalog (denied-type (fn [] (north.work-catalog/catalog-envelope "catalog-test" 42 missing-title-facts)))))

(def ^String second-assignment "@occurrence:assignment-2")

(def ambiguous-assignment-facts (vec (concat catalog-facts (referents/assignment-facts! second-assignment plan tracker tracker "2026-08-30T10:05:00Z"))))

(check! "multiple complete Assignments are rejected instead of selected" (= :north/invalid-semantic-catalog (denied-type (fn [] (north.work-catalog/catalog-envelope "catalog-test" 43 ambiguous-assignment-facts)))))

(defn fact-row [fact]
  [(t/triple-t1 fact) (t/triple-t2 fact) (t/triple-t3 fact)])

(let [seen-port (atom 0)]
  (with-redefs-fn {live-facts-view-var (fn [port] (do
  (reset! seen-port port)
  {:domains {:coordination {:available true :version 77 :facts (mapv fact-row catalog-facts)}}}))} (fn [] (let [effectful (north.work-catalog/catalog-envelope! 43123)]
  (check! "effectful entry projects the exact coordination snapshot" (and (= 43123 (deref seen-port)) (= "north-coordination" (:storeSpace effectful)) (= 77 (:storeVersion effectful)) (= (mapv (fn [row] (:id row)) rows) (mapv (fn [row] (:id row)) (:trackedThings effectful)))))))))

(check! "effectful entry rejects an unavailable Store snapshot" (= :north/semantic-catalog-store-unavailable (with-redefs-fn {live-facts-view-var (fn [_port] {:domains {:coordination {:available false :error "fixture unavailable"}}})} (fn [] (denied-type (fn [] (north.work-catalog/catalog-envelope! 43123)))))))

(let [results (deref checks)
   passed (count (filter (fn [^Check result] (check-passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (check-passed result) "PASS" "FAIL") (check-label result))))
  (println (format "\nWork catalog B5a: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
