(ns north.store-rpc-request-id-test
  (:require [clojure.java.io :as io]))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/store-rpc-client.clj"))

(def checks (atom []))

(defn check! [^String label ^Boolean value]
  (println (str (if value "  [PASS] " "  [FAIL] ") label))
  (swap! checks conj [label value])
  nil)

(defn private-function [name]
  (let [resolved (ns-resolve 'north.store-rpc-client name)]
  (if (not resolved) (do
  (throw (ex-info "Store RPC private test seam is unavailable" {:name name}))))
  resolved))

(defn thrown-type [operation]
  (try
  (operation)
  nil
  (catch clojure.lang.ExceptionInfo error
    (:type (ex-data error)))))

(let [request-id-for-process (private-function 'request-id-for-process)
   next-request-id (private-function 'next-request-id)
   first-id (request-id-for-process 1 1)
   second-process-id (request-id-for-process 2 1)
   maximum-id (request-id-for-process 2147483647 4294967295)
   live-first (next-request-id)
   live-second (next-request-id)]
  (check! "process identity occupies a disjoint 32-bit request namespace" (= [4294967297 8589934593] [first-id second-process-id]))
  (check! "the largest admitted process and sequence fit signed i64 exactly" (= Long/MAX_VALUE maximum-id))
  (check! "one process emits consecutive request ids without reuse" (= 1 (- live-second live-first)))
  (check! "process id zero fails closed" (= :rpc/request-id-process-invalid (thrown-type (fn [] (request-id-for-process 0 1)))))
  (check! "sequence exhaustion fails closed instead of wrapping" (= :rpc/request-id-sequence-exhausted (thrown-type (fn [] (request-id-for-process 1 4294967296))))))

(let [failures (remove second (deref checks))]
  (println)
  (if (seq failures) (do
  (println "Store RPC request identity:" (- (count (deref checks)) (count failures)) "/" (count (deref checks)) "PASS")
  (System/exit 1)) (println "Store RPC request identity:" (count (deref checks)) "/" (count (deref checks)) "PASS")))
