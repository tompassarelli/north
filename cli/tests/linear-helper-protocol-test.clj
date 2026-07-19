#!/usr/bin/env bb
;; Hostile coordinator fixtures for the Linear CAS helpers. Read envelopes and
;; mutation acknowledgements are authority boundaries: incoherent or extended
;; responses must fail before they can authorize a durable write.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file *file*)) "../..")))
(def reserve (str root "/sdk/src/integrations/linear/reserve-link.clj"))
(def schema (str root "/sdk/src/integrations/linear/reserve-schema-fact.clj"))

(def valid-resolved
  {:value nil :members 0 :ambiguous? false :values [] :version 1})

(defn serve-fixture [respond]
  (let [server (java.net.ServerSocket. 0)
        requests (atom [])
        running (atom true)
        worker
        (future
          (while @running
            (try
              (with-open [socket (.accept server)
                          reader (io/reader (.getInputStream socket))
                          writer (io/writer (.getOutputStream socket))]
                (let [envelope (edn/read-string (.readLine reader))
                      request (:request envelope)]
                  (swap! requests conj request)
                  (.write writer (str (pr-str (respond request)) "\n"))
                  (.flush writer)))
              (catch java.net.SocketException _
                (when @running
                  (throw (ex-info "fixture coordinator socket failed" {})))))))]
    {:port (.getLocalPort server)
     :requests requests
     :stop!
     (fn []
       (reset! running false)
       (.close server)
       (deref worker 5000 nil))}))

(defn invoke! [script port args]
  (let [dir (.toFile
             (java.nio.file.Files/createTempDirectory
              "north-linear-hostile-coordinator"
              (make-array java.nio.file.attribute.FileAttribute 0)))
        log (.getCanonicalPath (io/file dir "facts.log"))]
    (spit log "")
    (try
      (let [result
            (apply proc/shell
                   {:out :string :err :string :continue true
                    :extra-env {"FRAM_LOG" log}}
                   "bb" script (str port) args)]
        (when-not (zero? (:exit result))
          (throw (ex-info "Linear helper process failed" {:result result})))
        (json/parse-string (str/trim (:out result))))
      (finally
        (doseq [file (reverse (file-seq dir))]
          (io/delete-file file true))))))

(def reserve-args
  ["linear-sync:identity:linear%3Auuid%3A22222222-2222-8222-8222-222222222222%3A11111111-1111-8111-8111-111111111111"
   "hostile-holder"
   "1"
   "link:linear:uuid:22222222-2222-8222-8222-222222222222:11111111-1111-8111-8111-111111111111"
   "hostile-thread"
   "linear-hostile"
   "linear-uuid"])

(def schema-args
  ["exact" "linear_hostile_schema" "value_kind" "literal"])

(def mutation-ops
  #{:assert-at-version :assert-at-version-with-fence :assert-with-fence})

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(doseq [[label script args]
        [["binding" reserve reserve-args]
         ["schema" schema schema-args]]]
  (let [fixture
        (serve-fixture
         (fn [request]
           (case (:op request)
             :version {:version 1}
             :resolved
             {:value "other" :members 1 :ambiguous? false
              :values ["expected"] :version 1}
             :query (if (= label "schema")
                      {:ok [] :version 1 :engine "index"
                       :unexpected "authority-confusion"}
                      {:ok [] :version 1 :engine "index"})
             {:ok 2})))
        result (try
                 (invoke! script (:port fixture) args)
                 (finally ((:stop! fixture))))]
    (check! (str label " rejects an incoherent read envelope")
            (str/includes?
             (get result "reject" "")
             (if (= label "schema")
               "invalid query response"
               "invalid resolved response")))
    (check! (str label " performs no mutation after hostile read evidence")
            (not-any? mutation-ops (map :op @(:requests fixture)))))

  (let [fixture
        (serve-fixture
         (fn [request]
           (case (:op request)
             :version {:version 1}
             :resolved valid-resolved
             :query {:ok [] :version 1 :engine "index"}
             :assert-at-version {:ok 2 :unexpected "authority-confusion"}
             :assert-at-version-with-fence {:ok 2 :unexpected "authority-confusion"}
             :assert-with-fence {:ok 2 :unexpected "authority-confusion"}
             {:reject :unexpected :version 1})))
        result (try
                 (invoke! script (:port fixture) args)
                 (finally ((:stop! fixture))))]
    (check! (str label " rejects an extended success acknowledgement")
            (str/includes? (get result "reject" "") "invalid mutation response"))))

(let [failed (remove second @checks)]
  (doseq [[label ok?] @checks]
    (println (str (if ok? "PASS " "FAIL ") label)))
  (if (seq failed)
    (System/exit 1)
    (println "linear helper protocol: PASS")))
