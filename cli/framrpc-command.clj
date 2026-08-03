#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[fram.types :as t])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))

(load-file (str root "/cli/framrpc-client.clj"))
(require '[north.framrpc-client :as rpc])

(defn fail! [message data]
  (throw (ex-info message data)))

(defn positive-int! [label value]
  (let [parsed (parse-long (or value ""))]
    (when-not (and parsed (pos? parsed))
      (fail! (str label " must be a positive integer") {label value}))
    parsed))

(defn legacy-triple [triple]
  ;; Migration metadata uses typed Terms; North v0.3 exposed only string triples.
  (let [row [(t/triple-slot0 triple)
             (t/triple-slot1 triple)
             (t/triple-slot2 triple)]]
    (when (every? string? row) row)))

(defn scan-result [client subject]
  ;; Paged scan is suppression-aware; direct query exposes superseded occurrences.
  (let [result (rpc/scan-all! client subject nil nil {:page-size 200})]
    {:served-version (:served-version result)
     :pages (:pages result)
     :raw-live-count (count (:rows result))
     :rows (into [] (keep legacy-triple) (:rows result))}))

(defn write-result [client operation subject predicate value expected]
  (try
    (let [options {:expected-version expected}
          proposition (t/triple subject predicate value)
          result (case operation
                   "assert" (rpc/assert! client proposition options)
                   "retract" (rpc/retract! client proposition options)
                   (fail! "write operation must be assert or retract"
                          {:operation operation}))]
      {:served-version (:served-version result)
       :changed? (boolean (get-in result [:results 0 :changed?]))})
    (catch clojure.lang.ExceptionInfo error
      (if (= :rpc/conflict (:type (ex-data error)))
        {:error :conflict}
        (throw error)))))

(defn -main [& arguments]
  (let [[command host port-text space-id & more] arguments]
    (when-not (and command host port-text space-id)
      (fail! "usage: framrpc-command COMMAND HOST PORT SPACE_ID [ARGS...]" {}))
    (let [port (positive-int! :port port-text)
          read-timeout (positive-int!
                        :read-timeout-ms
                        (or (System/getenv "NORTH_FRAMRPC_READ_TIMEOUT_MS")
                            "60000"))
          client (rpc/connect host port space-id
                              {:connect-timeout-ms 1000
                               :read-timeout-ms read-timeout
                               :max-attempts 3
                               :retry-delay-ms 10
                               :jitter-ms 25})]
      (try
        (prn
         (case command
           "scan-all" (scan-result client nil)
           "scan-subject"
           (let [[subject] more]
             (when-not subject (fail! "scan-subject requires SUBJECT" {}))
             (scan-result client subject))
           "version" (:served-version (rpc/version! client))
           "status" (rpc/status! client)
           "write"
           (let [[operation subject predicate value expected-text] more]
             (when-not (and operation subject predicate value expected-text)
               (fail! "write requires OP SUBJECT PREDICATE VALUE EXPECTED_VERSION" {}))
             (write-result client operation subject predicate value
                           (positive-int! :expected-version expected-text)))
           (fail! "unknown framrpc command" {:command command})))
        (finally (rpc/close! client))))))

(try
  (apply -main *command-line-args*)
  (catch Throwable error
    (binding [*out* *err*]
      (prn {:error (or (:type (ex-data error)) :framrpc-command-failed)
            :message (.getMessage error)
            :data (ex-data error)}))
    (System/exit 1)))
