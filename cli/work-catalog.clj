(ns north.work-catalog
  (:require [clojure.string :as str]
            [north.coord :as coord]
            [north.projections :as proj]
            [north.referents :as referents]
            [store.types :as t]))

(def ^String protocol "north.semantic-catalog")

(def protocol-version 1)

(def ^String default-store-space "north-coordination")

(defrecord CatalogTrackedThing [id title desiredOutcome agent work plan project task assignee assigneeTitle status])

(defn catalogtrackedthing-id [r] (:id r))

(defn catalogtrackedthing-title [r] (:title r))

(defn catalogtrackedthing-desiredOutcome [r] (:desiredOutcome r))

(defn catalogtrackedthing-agent [r] (:agent r))

(defn catalogtrackedthing-work [r] (:work r))

(defn catalogtrackedthing-plan [r] (:plan r))

(defn catalogtrackedthing-project [r] (:project r))

(defn catalogtrackedthing-task [r] (:task r))

(defn catalogtrackedthing-assignee [r] (:assignee r))

(defn catalogtrackedthing-assigneeTitle [r] (:assigneeTitle r))

(defn catalogtrackedthing-status [r] (:status r))

(defrecord CatalogEnvelope [protocol version storeSpace storeVersion trackedThings])

(defn catalogenvelope-protocol [r] (:protocol r))

(defn catalogenvelope-version [r] (:version r))

(defn catalogenvelope-storeSpace [r] (:storeSpace r))

(defn catalogenvelope-storeVersion [r] (:storeVersion r))

(defn catalogenvelope-trackedThings [r] (:trackedThings r))

(defrecord AssignmentDisplay [assignee assigneeTitle])

(defn assignmentdisplay-assignee [r] (:assignee r))

(defn assignmentdisplay-assigneeTitle [r] (:assigneeTitle r))

(defn- invalid-catalog [^String message data]
  (throw (ex-info message (assoc data :type :north/invalid-semantic-catalog))))

(defn- ^String nonblank [^String field ^String value]
  (if (str/blank? value) (invalid-catalog (str field " must be nonblank") {:field field}) value))

(defn- exact-string-values [idx ^String subject ^String predicate]
  (let [raw (proj/values-at idx subject predicate)]
  (if (every? string? raw) (mapv (fn [value] value) raw) (invalid-catalog "catalog predicate contains a non-string value" {:subject subject :predicate predicate}))))

(defn- ^String required-value [idx ^String subject ^String predicate ^String field]
  (let [values (exact-string-values idx subject predicate)]
  (if (and (= 1 (count values)) (not (str/blank? (first values)))) (first values) (invalid-catalog (str field " must have exactly one nonblank value") {:field field :subject subject :predicate predicate}))))

(defn- optional-value [idx ^String subject ^String predicate ^String field]
  (let [values (exact-string-values idx subject predicate)]
  (cond
  (empty? values) nil
  (and (= 1 (count values)) (not (str/blank? (first values)))) (first values)
  :else (invalid-catalog (str field " must be absent or have exactly one nonblank value") {:field field :subject subject :predicate predicate}))))

(defn- assignment-display [idx ^String tracked-thing]
  (let [assignments (->> (referents/occurrence-ids idx tracked-thing referents/assignment-kind) (filterv (fn [^String assignment] (referents/assignment-occurrence? idx assignment tracked-thing))) sort vec)]
  (cond
  (empty? assignments) nil
  (= 1 (count assignments)) (let [^String assignment (first assignments)
   ^String assignee (required-value idx assignment "assignee" "assignment assignee")]
  (->AssignmentDisplay assignee (required-value idx assignee "title" "assignee title")))
  :else (invalid-catalog "tracked thing has more than one complete Assignment" {:tracked-thing tracked-thing :assignments assignments}))))

(defn- ^CatalogTrackedThing catalog-row [idx ^String tracked-thing]
  (let [^String tracked-thing-id (nonblank "tracked thing ID" tracked-thing)
   assignment (assignment-display idx tracked-thing-id)]
  (->CatalogTrackedThing tracked-thing-id (required-value idx tracked-thing-id "title" "tracked thing title") (optional-value idx tracked-thing-id "desired_outcome" "desired outcome") (referents/agent? idx tracked-thing-id) (referents/work? idx tracked-thing-id) (referents/plan? idx tracked-thing-id) (referents/project? idx tracked-thing-id) (referents/task? idx tracked-thing-id) (if (some? assignment) (assignmentdisplay-assignee assignment) nil) (if (some? assignment) (assignmentdisplay-assigneeTitle assignment) nil) nil)))

(defn ^CatalogEnvelope catalog-envelope [^String store-space store-version facts]
  (do
  (nonblank "Store space" store-space)
  (if (neg? store-version) (do
  (invalid-catalog "Store version must be nonnegative" {:store-version store-version})))
  (let [idx (proj/index-triples facts)
   tracked-things (vec (sort (referents/all-tracked-thing-ids idx)))]
  (->CatalogEnvelope protocol protocol-version store-space store-version (mapv (fn [^String tracked-thing] (catalog-row idx tracked-thing)) tracked-things)))))

(defn- fact-row! [row]
  (if (and (vector? row) (= 3 (count row)) (every? string? row)) (t/triple (nth row 0) (nth row 1) (nth row 2)) (invalid-catalog "Store snapshot contains a malformed fact row" {:row row})))

(defn- ^String store-space! []
  (nonblank "Store space" (or (System/getenv "BEAGLE_STORE_SPACE_ID") default-store-space)))

(defn ^CatalogEnvelope catalog-envelope! [port]
  (let [view (coord/live-facts-view! port)
   domains (:domains view)
   snapshot (if (map? domains) (do
  (:coordination domains)))
   available (if (map? snapshot) (do
  (:available snapshot)))
   store-version (if (map? snapshot) (do
  (:version snapshot)))
   rows (if (map? snapshot) (do
  (:facts snapshot)))]
  (if (not (= true available)) (do
  (throw (ex-info "coordination Store snapshot is unavailable" {:type :north/semantic-catalog-store-unavailable}))))
  (if (not (and (integer? store-version) (not (neg? store-version)) (vector? rows))) (do
  (throw (ex-info "coordination Store snapshot is malformed" {:type :north/malformed-semantic-catalog-snapshot}))))
  (catalog-envelope (store-space!) store-version (mapv (fn [row] (fact-row! row)) rows))))
