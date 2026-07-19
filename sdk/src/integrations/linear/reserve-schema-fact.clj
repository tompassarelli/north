;; Install one Linear schema fact against the exact coordinator version that
;; was inspected. Concurrent compatible installers converge; a conflicting
;; writer is re-read and rejected rather than overwritten.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def here (.getParentFile (io/file (System/getProperty "babashka.file"))))
(load-file (.getCanonicalPath (io/file here "../../../.." "cli" "coord.clj")))

(defn fail! [message]
  (throw (ex-info message {:type :linear-schema-conflict})))

(defn required-text [label value]
  (when (or (not (string? value)) (str/blank? value) (not= value (str/trim value)))
    (fail! (str label " must be canonical nonblank text")))
  value)

(defn positive-long [label raw]
  (let [value (try (Long/parseLong (str raw))
                   (catch Exception _ (fail! (str label " must be a positive integer"))))]
    (when-not (pos? value) (fail! (str label " must be a positive integer")))
    value))

(defn nonnegative-long? [value]
  (and (integer? value) (<= 0 value) (<= value Long/MAX_VALUE)))

(defn exact-keys? [value expected]
  (and (map? value) (= (set (keys value)) expected)))

(defn values-of [port subject predicate]
  ;; `cardinality` and `value_kind` live in Fram's schema-as-facts client view,
  ;; not in the reified domain group served by :resolved. Query the public fact
  ;; relation so the CAS validates the same schema facts that `north show` sees.
  (let [response
        (north.coord/send-op
         port
         {:op :query
          :query
          {:find "value"
           :rules
           [{:head {:rel "value" :args [{:var "value"}]}
             :body [{:rel "triple"
                     :args [subject predicate {:var "value"}]}]}]}})
        rows (:ok response)]
    (when-not (and (exact-keys? response #{:ok :version :engine})
                   (nonnegative-long? (:version response))
                   (#{"index" "scan"} (:engine response))
                   (vector? rows)
                   (every? (fn [row]
                             (and (vector? row)
                                  (= 1 (count row))
                                  (string? (first row))))
                           rows)
                   (= (count rows) (count (set rows))))
      (fail! "Linear schema reservation received an invalid query response"))
    (set (map first rows))))

(defn exact-success? [result]
  (and (exact-keys? result #{:ok})
       (nonnegative-long? (:ok result))))

(defn exact-reject? [result]
  (and (exact-keys? result #{:reject :version})
       (keyword? (:reject result))
       (nonnegative-long? (:version result))))

(let [[port-token mode subject-token predicate-token value-token & tail]
      *command-line-args*]
  (try
    (let [port (positive-long "port" port-token)
          _ (when (> port 65535) (fail! "port must be at most 65535"))
          mode (required-text "mode" mode)
          _ (when-not (#{"exact" "migrate"} mode)
              (fail! "schema reservation mode must be exact or migrate"))
          _ (when-not (= (count tail) (if (= mode "migrate") 1 0))
              (fail! "schema reservation arguments do not match its mode"))
          subject (str "@" (str/replace
                             (required-text "subject" subject-token)
                             #"^@" ""))
          predicate (required-text "predicate" predicate-token)
          value (required-text "value" value-token)
          allowed-previous (when (= mode "migrate")
                             (required-text "allowed previous value" (first tail)))
          validate!
          (fn []
            (let [values (values-of port subject predicate)
                  allowed (cond-> #{value}
                            allowed-previous (conj allowed-previous))]
              (when (or (> (count values) 1)
                        (some #(not (contains? allowed %)) values))
                (fail! (str "Linear graph schema conflicts on @"
                            (subs subject 1) " " predicate)))))]
      (let [result
            (north.coord/assert-after-read!
             port subject predicate value validate!)]
        (if (exact-success? result)
          (println (json/generate-string {"ok" (:ok result)}))
          (if (exact-reject? result)
            (println
             (json/generate-string
              {"reject" (if (= :conflict (:reject result))
                          "Linear graph schema raced with another writer"
                          "Linear graph schema assertion was rejected")}))
            (fail! "Linear schema reservation received an invalid mutation response")))))
    (catch Exception error
      (println (json/generate-string {"reject" (.getMessage error)})))))
