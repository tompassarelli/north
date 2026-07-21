#!/usr/bin/env bb
;; Exact-fold + real-coordinator proof for the executable schema cutover.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[fram.fold :as fold]
         '[fram.rt :as rt])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(def fram (.getCanonicalPath
           (io/file (or (System/getenv "FRAM_PATH") (str root "/../fram")))))
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw (ex-info "Fram checkout not found; set FRAM_PATH or keep it beside North" {:fram fram})))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn port-open? [port]
  (try (with-open [_ (java.net.Socket. "127.0.0.1" (int port))] true)
       (catch Exception _ false)))

(defn eventually [f]
  (loop [remaining 1200]
    (cond
      (try (f) (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(defn op [tx operation subject predicate object]
  (pr-str {:tx tx :op operation :l subject :p predicate :r object :frame "schema-test"}))

(defn live-facts [log telemetry]
  (:facts (fold/fold (concat (rt/read-log log) (rt/read-log telemetry)))))

(defn values [facts subject predicate]
  (set (map :r (filter #(and (= subject (:l %)) (= predicate (:p %))) facts))))

(defn run-schema [port log telemetry receipt-dir & args]
  (apply proc/shell
         {:dir root
          :out :string
          :err :string
          :continue true
          :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry
                      "FRAM_SINGLE_VALUED" ""}}
         "bb" "-cp" (str fram "/out")
         (str root "/cli/schema-migrate.clj") (str port)
         (concat args ["--log" log "--telemetry" telemetry
                       "--receipt-dir" receipt-dir])))

(defn run-pred [port log telemetry & args]
  (apply proc/shell
         {:dir root :out :string :err :string :continue true
          :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry}}
         "bb" (str root "/cli/pred-cli.clj") (str port) args))

(defn start-daemon [port log telemetry]
  (proc/process {:dir fram :out :string :err :string
                 :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry}}
                "bb" "-cp" "out" "coord_daemon.clj"
                "serve-flat" (str port) log))

(defn stop-daemon! [daemon]
  (when daemon
    (proc/destroy-tree daemon)
    (try @daemon (catch Exception _ nil))))

(defn delete-tree! [file]
  (when (.isDirectory file)
    (doseq [child (.listFiles file)] (delete-tree! child)))
  (.delete file))

(let [port (free-port)
      temp (.toFile (java.nio.file.Files/createTempDirectory
                     "north-schema-migrate-"
                     (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file temp "coordination.log"))
      telemetry (.getCanonicalPath (io/file temp "telemetry.log"))
      receipts (.getCanonicalPath (io/file temp "receipts"))
      _ (spit log (str/join "\n"
                            [(op 1 "assert" "@thread-a" "title" "one")
                             (op 2 "assert" "@thread-a" "custom_ref" "@thread-b")
                             (op 3 "assert" "@thread-b" "title" "two")
                             ;; A valid executable declaration is authority even
                             ;; when bootstrap intent disagrees.
                             (op 4 "assert" "@title" "cardinality" "multi")
                             (op 5 "assert" "@topic-schema" "title" "schema")
                             (op 6 "assert" "@concern-schema" "kind" "concern")
                             (op 7 "assert" "@msg:schema" "body" "hello")
                             (op 8 "assert" "@run-schema" "kind" "run")
                             (op 9 "assert" "@client-clock" "kind" "client_session")
                             (op 10 "assert" "@denial:schema" "kind" "guard_denial")
                             (op 11 "assert" "@agent:schema" "display_name" "Agent Schema")
                             (op 12 "assert" "@person-schema" "display_name" "Person Schema")
                             ;; Explicit namespace/name extensions are preserved.
                             (op 13 "assert" "@vendor-subject" "entity_kind" "vendor/widget")
                             (op 14 "assert" "@vendor-subject" "note" "extension")
                             ;; No structural signal: remain visible as `other`.
                             (op 15 "assert" "@ambiguous" "opaque" "value")
                             ;; The live malformed predicate must be surfaced and
                             ;; repaired as an exact triple, never registered.
                             (op 16 "assert" "@thread-a" "\"\"" "")])
              :append true)
      _ (spit telemetry "")
      daemon (atom (start-daemon port log telemetry))
      checks (atom [])
      check! (fn [label ok detail]
               (swap! checks conj {:label label :ok (boolean ok) :detail detail}))]
  (try
    (check! "real Fram coordinator starts" (eventually #(port-open? port)) nil)

    (let [listing (run-pred port log telemetry "ls")
          show-missing (run-pred port log telemetry "show" "owner")
          lint-before (run-pred port log telemetry "lint" "--strict")]
      (check! "connected predicate listing is generated only from graph authority"
              (and (zero? (:exit listing))
                   (str/includes? (:out listing) "1 on")
                   (str/includes? (:out listing) "title")
                   (not (str/includes? (:out listing) "owner")))
              (:out listing))
      (check! "connected show never fills absent graph metadata from bootstrap"
              (and (zero? (:exit show-missing))
                   (not (str/includes? (:out show-missing) "organizational owner")))
              (:out show-missing))
      (check! "connected strict lint fails when graph declarations are absent"
              (= 1 (:exit lint-before))
              (str "exit=" (:exit lint-before) " out=" (:out lint-before))))

    (let [before (run-schema port log telemetry receipts "audit" "--strict")]
      (check! "strict audit fails before migration"
              (= 1 (:exit before)) (str "exit=" (:exit before) " out=" (:out before)))
      (check! "audit names the malformed predicate"
              (str/includes? (:out before) "corrupt predicate") (:out before)))

    (let [refused (run-schema port log telemetry receipts "migrate" "--execute")]
      (check! "online migration refuses to register around corrupt bytes"
              (and (not (zero? (:exit refused)))
                   (str/includes? (:err refused) "repair-corrupt"))
              (str "exit=" (:exit refused) " out=" (:out refused) " err=" (:err refused))))

    (stop-daemon! @daemon)
    (reset! daemon nil)
    (check! "coordinator is offline before physical repair"
            (eventually #(not (port-open? port))) nil)
    (let [repair (run-schema port log telemetry receipts "repair-corrupt" "--execute" "--offline-confirm")]
      (check! "offline repair appends exact retract tombstone under lock + fsync"
              (and (zero? (:exit repair)) (str/includes? (:out repair) "exact retract tombstone"))
              (str "exit=" (:exit repair) " out=" (:out repair) " err=" (:err repair))))

    (reset! daemon (start-daemon port log telemetry))
    (check! "coordinator restarts on the repaired corpus" (eventually #(port-open? port)) nil)
    (let [migrate (run-schema port log telemetry receipts "migrate" "--execute")]
      (check! "migration exits 0 after corruption is repaired"
              (zero? (:exit migrate)) (str "exit=" (:exit migrate) " out=" (:out migrate) " err=" (:err migrate)))
      (check! "repair and migration each emit a persisted receipt"
              (and (str/includes? (:out migrate) "receipt ")
                   (.isDirectory (io/file receipts))
                   (= 2 (count (.listFiles (io/file receipts)))))
              (:out migrate)))

    (let [after (run-schema port log telemetry receipts "audit" "--strict")]
      (check! "strict audit passes after migration"
              (zero? (:exit after)) (str "exit=" (:exit after) " out=" (:out after)))
      (check! "audit reports executable authority + governed entity kinds"
              (str/includes? (:out after) "executable predicate entities are authoritative")
              (:out after)))

    (let [lint-after (run-pred port log telemetry "lint" "--strict")]
      (check! "connected strict lint passes from migrated graph authority"
              (zero? (:exit lint-after))
              (str "exit=" (:exit lint-after) " out=" (:out lint-after))))

    (let [facts (live-facts log telemetry)]
      (check! "valid graph declaration wins over bootstrap intent"
              (= #{"multi"} (values facts "@title" "cardinality")) nil)
      (check! "observed unknown ref predicate gains complete executable metadata"
              (and (= #{"multi"} (values facts "@custom_ref" "cardinality"))
                   (= #{"ref"} (values facts "@custom_ref" "value_kind"))
                   (= #{"predicate"} (values facts "@custom_ref" "entity_kind"))
                   (= 1 (count (values facts "@custom_ref" "doc")))) nil)
      (check! "meta-schema is self-describing"
              (and (= #{"single"} (values facts "@cardinality" "cardinality"))
                   (= #{"literal"} (values facts "@cardinality" "value_kind"))) nil)
      (check! "core entity-kind definitions are explicit and open-taxonomy shaped"
              (and (= #{"north/entity_kind_definition"}
                      (values facts "@entity-kind:thread" "entity_kind"))
                   (= #{"thread"}
                      (values facts "@entity-kind:thread" "entity_kind_name"))) nil)
      (let [expected {"@thread-a" "thread"
                      "@topic-schema" "topic"
                      "@concern-schema" "concern"
                      "@msg:schema" "message"
                      "@run-schema" "run"
                      "@client-clock" "client_session"
                      "@denial:schema" "guard_denial"
                      "@agent:schema" "agent"
                      "@person-schema" "person"
                      "@custom_ref" "predicate"}]
        (check! "migration assigns every unambiguous core entity kind"
                (every? (fn [[subject kind]]
                          (= #{kind} (values facts subject "entity_kind")))
                        expected)
                (pr-str (into {} (map (fn [[subject _]]
                                        [subject (values facts subject "entity_kind")])
                                      expected)))))
      (check! "explicit namespaced entity-kind extension is preserved"
              (= #{"vendor/widget"} (values facts "@vendor-subject" "entity_kind")) nil)
      (check! "ambiguous subject remains untyped instead of being guessed"
              (empty? (values facts "@ambiguous" "entity_kind")) nil)
      (check! "malformed predicate fact is absent after exact repair"
              (empty? (filter #(= "\"\"" (:p %)) facts))
              (pr-str (map #(select-keys % [:tx :op :l :p :r])
                           (filter #(= "\"\"" (:p %))
                                   (concat (rt/read-log log) (rt/read-log telemetry)))))))

    (let [second (run-schema port log telemetry receipts "migrate" "--execute")]
      (check! "second migration is a zero-delta idempotent run"
              (and (zero? (:exit second)) (str/includes? (:out second) "0 action(s)"))
              (str "exit=" (:exit second) " out=" (:out second))))

    (let [defined (run-pred port log telemetry "define" "extension/example" "single" "literal"
                            "extension predicate")]
      (check! "pred define writes the exact executable entity"
              (zero? (:exit defined)) (str "exit=" (:exit defined) " out=" (:out defined))))
    (let [facts (live-facts log telemetry)]
      (check! "pred define stores executable metadata on @name"
              (and (= #{"single"} (values facts "@extension/example" "cardinality"))
                   (= #{"literal"} (values facts "@extension/example" "value_kind"))
                   (= #{"predicate"} (values facts "@extension/example" "entity_kind"))) nil)
      (check! "pred define never creates a historical @pred:* authority"
              (empty? (filter #(= "@pred:extension/example" (:l %)) facts)) nil))

    (let [alias (run-pred port log telemetry "alias" "old" "new")]
      (check! "unsound executable predicate alias is rejected"
              (= 2 (:exit alias)) (str "exit=" (:exit alias) " err=" (:err alias))))

    (finally
      (stop-daemon! @daemon)
      (delete-tree! temp)))

  (let [results @checks failures (remove :ok results)
        passed (- (count results) (count failures))]
    (doseq [{:keys [label ok detail]} results]
      (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
      (when (and (not ok) detail) (println (str "         " detail))))
    (println (format "\nschema migration integration: %d / %d PASS" passed (count results)))
    (System/exit (if (empty? failures) 0 1))))
