(require '[clojure.java.io :as io])

(let [root (.getParentFile (.getParentFile
                            (io/file (System/getProperty "babashka.file"))))]
  (load-file (str root "/command-id.clj")))

(def checks (atom []))
(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)]))

(let [id north.command-id/content-id
      nested-a
      (array-map :outer (array-map :z #{3 1 2}
                                   :a [(array-map :right 2 :left 1)]))
      nested-b
      (array-map :outer (array-map :a [(array-map :left 1 :right 2)]
                                   :z #{2 3 1}))
      digest-a (id "tell" nested-a "worker" "retry")
      digest-b (id "tell" nested-b "worker" "retry")]
  (check "explicit identity is one full lowercase SHA-256"
         (boolean (re-matches #"^[0-9a-f]{64}$" digest-a)))
  (check "nested map and set insertion order is erased"
         (= digest-a digest-b))
  (check "nested sequence order remains authority-bearing"
         (not=
          (id "tell" {:payload [1 2 3]} "worker" "retry")
          (id "tell" {:payload [3 2 1]} "worker" "retry")))
  (check "set order is canonical"
         (=
          (id "tell" {:payload (into #{} [1 2 3])} "worker" "retry")
          (id "tell" {:payload (into #{} [3 2 1])} "worker" "retry")))
  (check "scalar types cannot alias"
         (not=
          (id "tell" {:payload 1} "worker" "retry")
          (id "tell" {:payload "1"} "worker" "retry")))
  (let [error
        (try
          (north.command-id/canonical-value (Object.))
          nil
          (catch Exception cause cause))]
    (check "unsupported values fail closed with a typed diagnostic"
           (= :unsupported-command-id-value
              (:type (ex-data error))))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\ncommand id: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
