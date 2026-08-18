;; orchestration-import-pointer-test.clj — atomic catalog pointer publication
;; Daemon-free: load the importer as a library and stub its FRAMRPC facade.
(require '[clojure.java.io :as io])

(def cli-dir (.getParentFile (io/file (System/getProperty "babashka.file"))))
(load-file (str (io/file (.getParentFile cli-dir) "orchestration-import-cli.clj")))

(def results (atom []))
(defn check [label pass?]
  (swap! results conj (boolean pass?))
  (println (format "  %s %s" (if pass? "✓" "✗") label)))

(defn ex-type [f]
  (try (f) ::no-throw (catch clojure.lang.ExceptionInfo e (:type (ex-data e)))))

(defn stub-query [graph]
  (fn [_port query]
    (let [[subject predicate _] (get-in query [:rules 0 :body 0 :args])]
      (mapv vector (get @graph [subject predicate] [])))))

(defn stub-publish [graph calls]
  (fn [_port actions]
    (swap! calls conj (vec actions))
    (doseq [{:keys [op subject predicate values]} actions]
      (when-not (= :set op)
        (throw (ex-info (str "unstubbed publication " op) {})))
      (swap! graph assoc [subject predicate] (vec values)))
    {:ok 1 :changed? true :results []}))

(println "orchestration importer pointer publication — daemon-free stubs")

(check "parse-version accepts a bare N"  (= 3 (parse-version "3")))
(check "parse-version accepts vN"        (= 3 (parse-version "v3")))
(check "parse-version is nil for no arg" (nil? (parse-version nil)))
(check "parse-version throws on garbage"
       (= :catalog-bad-version (ex-type #(parse-version "v3x"))))

(let [graph (atom {["@catalog:current" "catalog_version"] ["7"]})]
  (with-redefs [north.coord/query-rows (stub-query graph)]
    (check "version-arg falls back to the current pointer"
           (= 7 (version-arg 0 nil)))
    (check "version-arg honours vN over the pointer"
           (= 3 (version-arg 0 "v3")))))

(let [graph (atom {["@catalog:current" "catalog_version"] ["5"]})
      calls (atom [])
      expected [{:op :set
                 :subject "@catalog_version"
                 :predicate "cardinality"
                 :values ["single"]
                 :cardinality :one}
                {:op :set
                 :subject "@catalog:current"
                 :predicate "catalog_version"
                 :values ["6"]
                 :cardinality :one}]]
  (with-redefs [north.coord/publish! (stub-publish graph calls)]
    (flip! 0 6)
    (check "the schema declaration and pointer replacement use one publication"
           (= [expected] @calls))
    (check "the publication leaves one current version"
           (= ["6"] (get @graph ["@catalog:current" "catalog_version"])))
    (check "the publication declares catalog_version cardinality"
           (= ["single"] (get @graph ["@catalog_version" "cardinality"])))))

(with-redefs [north.coord/publish! (fn [& _] {:reject {:type :conflict}})]
  (check "a rejected publication fails closed"
         (= :catalog-publication-rejected (ex-type #(flip! 0 7)))))

(let [actions (vec
               (for [n (range 659)]
                 {:op :set :subject (str "@catalog:v8:test:" n)
                  :predicate "value" :values [(str n)] :cardinality :one}))
      batches (staging-batches actions)]
  (check "catalog staging covers every measured write exactly once"
         (= actions (vec (mapcat identity batches))))
  (check "catalog staging stays within the Store mutation action bound"
         (every? #(<= (reduce + (map (comp (partial max 1) count :values) %))
                       store.rpc-limits/rpc-v2-max-batch-actions)
                 batches))
  (check "the measured 659-write catalog requires three staging transactions"
         (= 3 (count batches))))

(check "one oversized multi-value staging action fails closed"
       (= :catalog-staging-action-too-large
          (ex-type
           #(staging-batches
             [{:op :set :subject "@catalog:v8:oversized" :predicate "value"
               :values (mapv str (range (inc store.rpc-limits/rpc-v2-max-batch-actions)))
               :cardinality :many}]))))

(let [n (count @results) ok (count (filter true? @results))]
  (println (format "%s %d/%d" (if (= n ok) "PASS" "FAIL") ok n))
  (System/exit (if (= n ok) 0 1)))
