(ns north.provider-native-session-projection
  (:require [clojure.java.io :as io]))

(def script-file (.getCanonicalPath (io/file *file*)))

(def cli-dir (.getParent (io/file script-file)))

(load-file (str cli-dir "/coord.clj"))

(defn- ^String env [^String name]
  (let [value (System/getenv name)]
  (if (string? value) value "")))

(defn- coord-put! [port ^String subject ^String predicate ^String value]
  (let [put! (clojure.core/ns-resolve (symbol "north.coord") (symbol "put!"))]
  (if (some? put!) (put! port subject predicate value) (throw (ex-info "north.coord/put! is unavailable" {})))))

(defn- common-facts []
  [["kind" "session"] ["repo" (env "NORTH_NATIVE_REPO")] ["provider" (env "NORTH_NATIVE_PROVIDER")] ["model" (env "NORTH_NATIVE_MODEL")] ["effort" (env "NORTH_NATIVE_EFFORT")] ["display_handle" (env "NORTH_NATIVE_DISPLAY")] ["display_name" (env "NORTH_NATIVE_DISPLAY")]])

(defn- optional-role [facts]
  (let [^String role (env "NORTH_NATIVE_ROLE")]
  (if (seq role) (conj facts ["role" role]) facts)))

(defn- spawn-facts []
  (let [^String session-key (env "NORTH_NATIVE_PROVIDER_SESSION_KEY")
   ^String parent-key (env "NORTH_NATIVE_PARENT_ACTOR_KEY")
   facts (conj (common-facts) ["execution_source" "provider-native"] ["execution_transport" "provider-hook"] ["provider_session_persistence" "unknown"] ["thread_provenance" "unknown"] ["turn_provenance" "unknown"] ["native_actor_kind" (env "NORTH_NATIVE_ACTOR_KIND")] ["native_depth" (env "NORTH_NATIVE_DEPTH")] ["dispatch_mode_at_start" (env "NORTH_NATIVE_DISPATCH_MODE_AT_START")])
   joined (if (seq session-key) (conj facts ["provider_join_key_version" "north-provider-join:v1"] ["provider_join_coverage" "partial"] ["provider_session_key" session-key]) facts)]
  (optional-role (if (seq parent-key) (conj joined ["native_parent_actor_key" parent-key]) joined))))

(defn- repair-facts []
  (optional-role (common-facts)))

(defn- alias-facts [^String subject]
  (let [^String role-alias (env "NORTH_NATIVE_ROLE_ALIAS")]
  (if (seq role-alias) [["title" role-alias] ["exclusivity" "exclusive"] ["target" (subs subject (count "@agent:"))]] [])))

(defn- publish! [facts]
  (let [port-value (parse-long (env "NORTH_NATIVE_PORT"))
   ^String subject (env "NORTH_NATIVE_SUBJECT")]
  (if (not (int? port-value)) (do
  (throw (ex-info "NORTH_NATIVE_PORT is malformed" {}))))
  (let [port port-value
   results (into (mapv (fn [fact] (coord-put! port subject (nth fact 0) (nth fact 1))) facts) (mapv (fn [fact] (coord-put! port (str "@role:" (env "NORTH_NATIVE_ROLE_ALIAS")) (nth fact 0) (nth fact 1))) (alias-facts subject)))]
  (if (some :reject results) (do
  (System/exit 1)))))
  nil)

(defn- run-cli! [args]
  (case (first args)
    "spawn" (publish! (spawn-facts))
    "repair" (publish! (repair-facts))
    (throw (ex-info "mode must be spawn or repair" {}))))

(defn- ^Boolean direct-invocation? []
  (= script-file (.getCanonicalPath (io/file (System/getProperty "babashka.file")))))

(if (direct-invocation?) (do
  (try
  (run-cli! (vec *command-line-args*))
  (catch Exception _
    (System/exit 2)))))
