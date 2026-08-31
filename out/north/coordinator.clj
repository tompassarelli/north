(ns north.coordinator
  (:gen-class)
  (:require [clojure.java.io :as io]
            [clojure.string :as str]))

(def default-control-port 7977)

(def ^String local-control-host "127.0.0.1")

(def ^String store-server-relative-path "server.clj")

(defrecord CoordinatorConfig [store-home port log-path space-id role])

(defn coordinatorconfig-store-home [r] (:store-home r))

(defn coordinatorconfig-port [r] (:port r))

(defn coordinatorconfig-log-path [r] (:log-path r))

(defn coordinatorconfig-space-id [r] (:space-id r))

(defn coordinatorconfig-role [r] (:role r))

(def store-lifecycle-loaded? (atom false))

(defn- fail! [^String message code data]
  (throw (ex-info message (assoc data :type code))))

(defn- ^String require-nonblank! [^String label value]
  (if (and (some? value) (not (str/blank? value))) value (fail! (str label " is required") :north.coordinator/missing-configuration {:field label})))

(defn- parse-control-port! [^String value]
  (let [parsed (parse-long value)]
  (if (and (integer? parsed) (<= 1 parsed 65535)) (int parsed) (fail! "NORTH_PORT must be an integer from 1 through 65535" :north.coordinator/invalid-port {:value value}))))

(defn- ^String canonical-path! [^String label ^String value]
  (try
  (.getCanonicalPath (io/file value))
  (catch Throwable error
    (fail! (str label " must be a canonical filesystem path") :north.coordinator/invalid-path {:field label :value value :cause (.getMessage error)}))))

(defn ^CoordinatorConfig coordinator-config! [store-home north-port log-path space-id bind-host role-name]
  (let [^String home (canonical-path! "BEAGLE_STORE_HOME" (require-nonblank! "BEAGLE_STORE_HOME" store-home))
   port (parse-control-port! (if (some? north-port) north-port (str default-control-port)))
   ^String log (canonical-path! "BEAGLE_STORE_LOG" (require-nonblank! "BEAGLE_STORE_LOG" log-path))
   ^String space (require-nonblank! "BEAGLE_STORE_SPACE_ID" space-id)
   ^String bind (if (some? bind-host) bind-host local-control-host)
   ^String role (if (some? role-name) role-name "active")
   server (io/file home store-server-relative-path)]
  (if (not (= bind local-control-host)) (fail! "North's coordinator control boundary must bind to 127.0.0.1" :north.coordinator/nonlocal-bind {:bind bind}) nil)
  (if (not (= role "active")) (fail! "North's sole coordinator must own active writer authority" :north.coordinator/invalid-role {:role role}) nil)
  (if (not (.isFile server)) (fail! "BEAGLE_STORE_HOME does not contain the Store JVM lifecycle host" :north.coordinator/missing-store-lifecycle {:path (.getPath server)}) nil)
  (->CoordinatorConfig home port log space :active)))

(defn ^CoordinatorConfig environment-config! []
  (coordinator-config! (System/getenv "BEAGLE_STORE_HOME") (System/getenv "NORTH_PORT") (System/getenv "BEAGLE_STORE_LOG") (System/getenv "BEAGLE_STORE_SPACE_ID") (System/getenv "BEAGLE_STORE_BIND") (System/getenv "BEAGLE_STORE_SERVER_ROLE")))

(defn- store-lifecycle! [^CoordinatorConfig config ^String operation]
  (locking store-lifecycle-loaded? (if (not (deref store-lifecycle-loaded?)) (do
  (System/setProperty "user.dir" (coordinatorconfig-store-home config))
  (binding [*command-line-args* []]
  (clojure.core/load-file (str (coordinatorconfig-store-home config) "/" store-server-relative-path)))
  (reset! store-lifecycle-loaded? true)) nil))
  (let [callable (clojure.core/ns-resolve (symbol "server") (symbol operation))]
  (if (some? callable) callable (fail! "Beagle Store lifecycle operation is unavailable" :north.coordinator/missing-store-operation {:operation operation}))))

(defn serve-config! [^CoordinatorConfig config]
  (do
  (apply (store-lifecycle! config "serve!") [(coordinatorconfig-port config) (coordinatorconfig-log-path config) (coordinatorconfig-space-id config) (coordinatorconfig-role config)])
  nil))

(defn shutdown! [^CoordinatorConfig config]
  (do
  (if (deref store-lifecycle-loaded?) (apply (store-lifecycle! config "shutdown!") []) nil)
  nil))

(defn run! [arguments]
  (if (empty? arguments) (serve-config! (environment-config!)) (fail! "north-coordinator takes configuration only from its environment" :north.coordinator/unexpected-arguments {:arguments arguments})))

(defn -main [& $beagle$rest$host]
  (let [arguments (vec $beagle$rest$host)]
  (run! arguments)))
