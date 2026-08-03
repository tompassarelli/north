#!/usr/bin/env -S bb -cp /home/tom/code/fram/main/out
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[coord-daemon-wire :as wire]
         '[fram.types :as t])

(import '[java.io File]
        '[java.nio.charset StandardCharsets]
        '[java.nio.file Files Path StandardCopyOption]
        '[java.security MessageDigest]
        '[java.util Arrays]
        '[java.util.concurrent TimeUnit])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH") "/home/tom/code/fram/main"))))
(def v03-out
  (.getCanonicalPath
   (io/file (or (System/getenv "NORTH_V03_FRAM_OUT")
                (str (System/getProperty "user.home")
                     "/.local/state/north/fram-runtime/active/current/out")))))

(load-file (str root "/cli/framrpc-client.clj"))
(require '[north.framrpc-client :as rpc])

(def baseline-expression
  "(require '[fram.rt :as rt] '[fram.fold :as fold] '[clojure.java.io :as io])
   (let [[source triples-path summary-path] *command-line-args*
         folded (fold/fold (rt/read-log source))
         facts (:facts folded)]
     (with-open [writer (io/writer triples-path)]
       (doseq [fact facts]
         (.write writer (pr-str [(:l fact) (:p fact) (:r fact)]))
         (.write writer \"\\n\")))
     (spit summary-path
           (pr-str {:version (:version folded) :live-count (count facts)})))")

(defn fail! [message data]
  (throw (ex-info message data)))

(defn sha256-bytes [^bytes bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
    (apply str (map #(format "%02x" (bit-and (int %) 255)) digest))))

(defn sha256-file [path]
  (sha256-bytes (Files/readAllBytes (.toPath (io/file path)))))

(defn process-result!
  [arguments directory environment stdout-path stderr-path]
  (let [builder (ProcessBuilder. ^java.util.List (mapv str arguments))
        process-environment (.environment builder)]
    (.directory builder (io/file directory))
    (doseq [[key value] environment]
      (.put process-environment (name key) (str value)))
    (.redirectOutput builder (io/file stdout-path))
    (.redirectError builder (io/file stderr-path))
    (let [process (.start builder)]
      {:exit (.waitFor process)
       :stdout stdout-path
       :stderr stderr-path})))

(defn output-text [{:keys [stdout stderr]}]
  (str (when (.exists (io/file stdout)) (slurp stdout))
       (when (.exists (io/file stderr)) (slurp stderr))))

(defn require-success! [label result]
  (when-not (zero? (:exit result))
    (fail! (str label " failed")
           {:type :rehearsal/process-failed
            :exit (:exit result)
            :output (output-text result)}))
  result)

(defn copy-input! [source target]
  (when-not (.isFile (io/file source))
    (fail! "legacy source log is not a readable file" {:source source}))
  (Files/copy (.toPath (io/file source)) (.toPath (io/file target))
              (into-array StandardCopyOption []))
  target)

(defn run-v03-fold! [scratch store]
  (let [name (:name store)
        triples (str scratch "/" name ".v03-live.edn")
        summary (str scratch "/" name ".v03-summary.edn")
        result (process-result!
                [(or (System/getenv "BABASHKA_BIN") "bb")
                 "-cp" v03-out "-e" baseline-expression "--"
                 (:copy store) triples summary]
                root {}
                (str scratch "/" name ".v03.stdout")
                (str scratch "/" name ".v03.stderr"))]
    (require-success! (str name " v0.3 fold") result)
    (assoc (edn/read-string (slurp summary))
           :triples-path triples)))

(defn legacy-rows [source]
  (with-open [reader (io/reader source)]
    (->> (line-seq reader)
         (remove str/blank?)
         (map-indexed
          (fn [index line]
            (let [row (edn/read-string line)]
              (when-not (and (map? row) (integer? (:tx row)))
                (fail! "legacy row lacks an integer transaction"
                       {:source source :line (inc index)}))
              [index row])))
         doall)))

(defn write-normalized-copy! [source target]
  (let [rows (legacy-rows source)
        txs (set (map (comp :tx second) rows))
        zero-count (count (filter #(zero? (:tx (second %))) rows))]
    (when (some neg? txs)
      (fail! "negative legacy transactions cannot be normalized"
             {:source source}))
    (when (and (pos? zero-count) (contains? txs 1))
      (fail! "transaction zero cannot be remapped without colliding with transaction one"
             {:source source :zero-count zero-count}))
    (let [normalized
          (map (fn [[index row]]
                 [index (if (zero? (:tx row)) (assoc row :tx 1) row)])
               rows)
          ordered (sort-by (fn [[index row]] [(:tx row) index]) normalized)]
      (with-open [writer (io/writer target)]
        (doseq [[_ row] ordered]
          (.write writer (pr-str row))
          (.write writer "\n")))
      {:path target :rows (count rows) :remapped-zero zero-count})))

(defn migration-result [scratch store source suffix]
  (let [name (:name store)
        target (str scratch "/" name suffix ".framlog")
        result (process-result!
                [(str fram "/bin/fram-migrate-triple-log")
                 source (:space-id store) target]
                fram {:CLJ_CONFIG (str scratch "/clj-config")}
                (str scratch "/" name suffix ".migration.stdout")
                (str scratch "/" name suffix ".migration.stderr"))]
    (assoc result :target target)))

(defn migrate! [scratch store]
  (let [direct (migration-result scratch store (:copy store) ".direct")]
    (if (zero? (:exit direct))
      {:target (:target direct) :direct? true :direct-exit 0}
      (let [normalized-path (str scratch "/" (:name store)
                                 ".tx-normalized.log")
            normalization (write-normalized-copy! (:copy store) normalized-path)
            normalized (migration-result scratch store normalized-path ".normalized")]
        (binding [*out* *err*]
          (println (str "DIVERGENCE: direct migration failed for "
                        (:space-id store) "; applying scratch-only stable tx sort"
                        " and tx=0 remap.\n" (str/trim (output-text direct)))))
        (require-success! (str (:name store) " normalized migration") normalized)
        {:target (:target normalized)
         :direct? false
         :direct-exit (:exit direct)
         :direct-error (str/trim (output-text direct))
         :normalization normalization}))))

(defn start-daemon! [scratch store]
  (let [name (:name store)
        builder
        (doto (ProcessBuilder.
               ^java.util.List
               [(str fram "/bin/fram-daemon") "serve" (str (:port store))
                (get-in store [:migration :target]) (:space-id store)])
          (.directory (io/file fram))
          (.redirectErrorStream true)
          (.redirectOutput (io/file scratch (str name ".daemon.log"))))
        environment (.environment builder)]
    (.put environment "CLJ_CONFIG" (str scratch "/clj-config"))
    (.put environment "FRAM_DAEMON_XMX" "1g")
    (.put environment "FRAM_DAEMON_QUIET" "1")
    (.put environment "FRAM_DAEMON_LOG"
          (str scratch "/" name ".requests.log"))
    (.start builder)))

(defn stop-daemon! [^Process process]
  (when process
    (.destroy process)
    (when-not (.waitFor process 5 TimeUnit/SECONDS)
      (.destroyForcibly process)
      (.waitFor process 5 TimeUnit/SECONDS))))

(defn connect-eventually! [store]
  (loop [attempt 0 last-error nil]
    (let [result
          (try
            {:client
             (rpc/connect "127.0.0.1" (:port store) (:space-id store)
                          {:connect-timeout-ms 200
                           :read-timeout-ms 60000
                           :max-attempts 1
                           :retry-delay-ms 0
                           :jitter-ms 0})}
            (catch Throwable error {:error error}))]
      (cond
        (:client result) (:client result)
        (= attempt 240)
        (fail! "Fram head daemon did not become FRAMRPC-ready"
               {:space-id (:space-id store)
                :port (:port store)
                :last-error (some-> (:error result) .getMessage)
                :last-data (some-> (:error result) ex-data)})
        :else
        (do (Thread/sleep 250)
            (recur (inc attempt) (:error result)))))))

(defn all-triples-query []
  (let [subject (wire/rpc-query-variable! "subject")
        predicate (wire/rpc-query-variable! "predicate")
        value (wire/rpc-query-variable! "value")]
    (wire/rpc-query-request!
     (wire/rpc-query-plan!
      (wire/rpc-query-find-relation! "dark-rehearsal-all")
      [(wire/rpc-query-stratum!
        [(wire/rpc-query-rule!
          (wire/rpc-query-head! "dark-rehearsal-all"
                                [subject predicate value])
          [(wire/rpc-query-relation! "triple"
                                     [subject predicate value] false)])])])
     wire/query-current)))

(defn read-triples [path]
  (with-open [reader (io/reader path)]
    (->> (line-seq reader) (remove str/blank?) (map edn/read-string) doall)))

(defn triple-vector [triple]
  [(t/triple-slot0 triple) (t/triple-slot1 triple) (t/triple-slot2 triple)])

(defn legacy-shaped? [triple]
  (every? string? triple))

(defn canonical-projection-bytes [triples]
  (.getBytes
   (str (str/join "\n" (sort (map pr-str triples)))
        (when (seq triples) "\n"))
   StandardCharsets/UTF_8))

(defn sampled-subjects [baseline]
  (->> baseline
       (map first)
       distinct
       (sort-by (fn [subject]
                  [(sha256-bytes (.getBytes (pr-str subject)
                                                 StandardCharsets/UTF_8))
                   (pr-str subject)]))
       (take 100)
       vec))

(defn sample-parity! [client baseline]
  (let [baseline-by-subject (group-by first baseline)
        subjects (sampled-subjects baseline)
        comparisons
        (mapv
         (fn [subject]
           (let [expected (get baseline-by-subject subject)
                 actual (->> (:rows (rpc/scan-all! client subject nil nil
                                                   {:page-size 200}))
                             (map triple-vector)
                             (filter legacy-shaped?))
                 expected-bytes (canonical-projection-bytes expected)
                 actual-bytes (canonical-projection-bytes actual)]
             {:subject subject
              :match? (Arrays/equals expected-bytes actual-bytes)
              :expected-sha256 (sha256-bytes expected-bytes)
              :actual-sha256 (sha256-bytes actual-bytes)}))
         subjects)
        mismatches (filterv (complement :match?) comparisons)]
    {:subjects (count subjects)
     :byte-exact? (and (= 100 (count subjects)) (empty? mismatches))
     :mismatches mismatches}))

(defn exercise-store! [store]
  (let [client (connect-eventually! store)]
    (try
      (let [version (rpc/version! client)
            status (rpc/status! client)
            query-result (rpc/query-all! client (all-triples-query)
                                         {:page-size 200})
            distinct-query-rows (vec (distinct (:rows query-result)))
            rpc-domain (filterv legacy-shaped? distinct-query-rows)
            baseline (read-triples (get-in store [:baseline :triples-path]))
            sample (sample-parity! client baseline)
            count-match? (= (count baseline) (count rpc-domain))
            status-match? (= (:live-count status) (count distinct-query-rows))]
        {:version version
         :status status
         :query {:row-count (count (:rows query-result))
                 :duplicate-rows (- (count (:rows query-result))
                                    (count distinct-query-rows))
                 :raw-live-count (count distinct-query-rows)
                 :legacy-live-count (count rpc-domain)
                 :pages (:pages query-result)
                 :served-version (:served-version query-result)}
         :parity {:v03-live-count (count baseline)
                  :framrpc-live-count (count rpc-domain)
                  :count-match? count-match?
                  :status-match? status-match?
                  :sample sample}})
      (finally (rpc/close! client)))))

(defn store-gaps [{:keys [name migration rpc]}]
  (let [parity (:parity rpc)
        status-count (get-in rpc [:status :live-count])
        query-count (get-in rpc [:query :raw-live-count])]
    (cond-> []
      (not (:direct? migration))
      (conj {:store name :gap :migration/nonmonotonic-transaction
             :direct-exit (:direct-exit migration)})

      (pos? (get-in migration [:normalization :remapped-zero] 0))
      (conj {:store name :gap :migration/transaction-zero
             :rows (get-in migration [:normalization :remapped-zero])})

      (not (:status-match? parity))
      (conj {:store name :gap :rpc/query-live-set-leak
             :status-live-count status-count
             :query-row-count query-count
             :excess (- query-count status-count)})

      (not (:count-match? parity))
      (conj {:store name :gap :rpc/query-domain-count-divergence
             :v03-live-count (:v03-live-count parity)
             :framrpc-query-count (:framrpc-live-count parity)
             :excess (- (:framrpc-live-count parity)
                        (:v03-live-count parity))})

      (not (get-in parity [:sample :byte-exact?]))
      (conj {:store name :gap :rpc/sample-byte-divergence
             :mismatches (get-in parity [:sample :mismatches])}))))

(defn prepare-scratch! [argument]
  (if argument
    (let [file (io/file argument)]
      (when (.exists file)
        (fail! "scratch target already exists; pass a new directory"
               {:scratch (.getCanonicalPath file)}))
      (Files/createDirectories (.toPath file)
                               (make-array java.nio.file.attribute.FileAttribute 0))
      (.getCanonicalPath file))
    (.getCanonicalPath
     (.toFile
      (Files/createTempDirectory
       (Path/of "/tmp" (make-array String 0)) "north-s3-rehearsal."
       (make-array java.nio.file.attribute.FileAttribute 0))))))

(defn -main [& arguments]
  (when (> (count arguments) 1)
    (binding [*out* *err*]
      (println "usage: scripts/framlog-dark-rehearsal.clj [NEW_SCRATCH_DIRECTORY]"))
    (System/exit 2))
  (when-not (.isFile (io/file fram "coord_daemon.clj"))
    (fail! "Fram head checkout is required" {:fram fram}))
  (when-not (.isDirectory (io/file v03-out))
    (fail! "deployed v0.3 Fram classpath is required" {:v03-out v03-out}))
  (let [scratch (prepare-scratch! (first arguments))
        state-home (str (System/getProperty "user.home") "/.local/state/north")
        stores
        [{:name "coordination" :space-id "north-coordination" :port 27977
          :source (or (System/getenv "NORTH_COORDINATION_LOG")
                      (str state-home "/coordination.log"))}
         {:name "telemetry" :space-id "north-telemetry" :port 27978
          :source (or (System/getenv "NORTH_TELEMETRY_LOG")
                      (str state-home "/telemetry.log"))}]
        daemons (atom [])]
    (println (str "scratch=" scratch))
    (try
      (Files/createDirectories (.toPath (io/file scratch "clj-config"))
                               (make-array java.nio.file.attribute.FileAttribute 0))
      (let [copied
            (mapv (fn [store]
                    (let [copy (str scratch "/" (:name store) ".log")]
                      (copy-input! (:source store) copy)
                      (assoc store :copy copy :source-sha256 (sha256-file copy))))
                  stores)
            folded
            (mapv #(assoc % :baseline (run-v03-fold! scratch %)) copied)
            migrated
            (mapv #(assoc % :migration (migrate! scratch %)) folded)
            started
            (mapv (fn [store]
                    (let [daemon (start-daemon! scratch store)]
                      (swap! daemons conj daemon)
                      (assoc store :daemon daemon)))
                  migrated)
            results
            (mapv (fn [store]
                    (let [rpc-result (exercise-store! store)]
                      {:name (:name store)
                       :space-id (:space-id store)
                       :port (:port store)
                       :source-sha256 (:source-sha256 store)
                       :baseline (dissoc (:baseline store) :triples-path)
                       :migration (dissoc (:migration store) :target)
                       :rpc rpc-result}))
                  started)
            pass?
            (every? #(and (get-in % [:rpc :parity :count-match?])
                          (get-in % [:rpc :parity :status-match?])
                          (get-in % [:rpc :parity :sample :byte-exact?]))
                    results)
            gaps (vec (mapcat store-gaps results))
            report {:thread "019fc335-58fa-734f-b473-38f875f5cbc5"
                    :scratch scratch
                    :status (if pass? :parity-pass :parity-fail)
                    :stores results
                    :s4-gaps gaps}]
        (spit (str scratch "/result.edn") (pr-str report))
        (doseq [gap gaps]
          (binding [*out* *err*]
            (println (str "DIVERGENCE: S4 gap " (pr-str gap)))))
        (prn report)
        (when-not pass? (System/exit 1)))
      (finally
        (doseq [daemon (reverse @daemons)] (stop-daemon! daemon))))))

(apply -main *command-line-args*)
