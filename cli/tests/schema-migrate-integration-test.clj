#!/usr/bin/env bb
;; Exact-fold + real-coordinator proof for the executable schema cutover.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[fram.fold :as fold]
         '[fram.rt :as rt])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(load-file (str root "/cli/schema-migrate.clj"))
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

(defn write-reviewed-manifest!
  [path log telemetry predicate-semantics other-entries]
  (let [corpus (read-corpus (resolve-corpus-paths! log telemetry))
        manifest {:format REPAIR-MANIFEST-FORMAT
                  :source (source-seal corpus)
                  :review {:by "schema-migrate-integration-test"
                           :at "2026-07-22T00:00:00Z"
                           :basis "Explicit fixture semantics and classifications."}
                  :predicate_semantics predicate-semantics
                  :cardinality_repairs []
                  :fact_repairs []
                  :other_allowlist
                  {:name "schema-migrate-integration-reviewed-other/v1"
                   :entries other-entries}}]
    (spit path (str (pr-str manifest) "\n"))
    path))

(defn run-pred [port log telemetry & args]
  (apply proc/shell
         {:dir root :out :string :err :string :continue true
          :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry}}
         "bb" (str root "/cli/pred-cli.clj") (str port) args))

(defn start-daemon [port log telemetry]
  (proc/process {:dir fram :out :string :err :string
                 :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry
                             "FRAM_REQUIRE_LOG_FENCE" "1"
                             "FRAM_SINGLE_VALUED" ""}}
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
      log-alias (.getAbsolutePath (io/file temp "coordination-alias.log"))
      receipts (.getCanonicalPath (io/file temp "receipts"))
      manifest-path (.getCanonicalPath (io/file temp "reviewed-manifest.edn"))
      invalid-log (.getCanonicalPath (io/file temp "acyclic-invalid.log"))
      invalid-telemetry (.getCanonicalPath (io/file temp "acyclic-invalid-telemetry.log"))
      invalid-receipts (.getCanonicalPath (io/file temp "acyclic-invalid-receipts"))
      corrupt-log (.getCanonicalPath (io/file temp "corrupt.log"))
      corrupt-telemetry (.getCanonicalPath (io/file temp "corrupt-telemetry.log"))
      corrupt-receipts (.getCanonicalPath (io/file temp "corrupt-receipts"))
      corrupt-manifest (.getCanonicalPath (io/file temp "corrupt-manifest.edn"))
      unterminated-log (.getCanonicalPath (io/file temp "unterminated.log"))
      unterminated-telemetry (.getCanonicalPath (io/file temp "unterminated-telemetry.log"))
      unterminated-receipts (.getCanonicalPath (io/file temp "unterminated-receipts"))
      unterminated-manifest (.getCanonicalPath (io/file temp "unterminated-manifest.edn"))
      _ (spit log (str (str/join "\n"
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
                                  ;; Valid explicit policy, including false, is
                                  ;; authoritative and must survive migration.
                                  (op 16 "assert" "@part_of" "acyclic" "false")
                                  (op 17 "assert" "@depends_on" "acyclic" "true")])
                       "\n")
              :append true)
      _ (spit telemetry "")
      _ (java.nio.file.Files/createSymbolicLink
         (.toPath (io/file log-alias))
         (.toPath (io/file log))
         (make-array java.nio.file.attribute.FileAttribute 0))
      _ (spit invalid-log
              (str/join "\n"
                        [(op 1 "assert" "@part_of" "acyclic" "true")
                         (op 2 "assert" "@part_of" "acyclic" "false")
                         (op 3 "assert" "@custom_edge" "acyclic" "maybe")]))
      _ (spit invalid-telemetry "")
      _ (spit corrupt-log (str (op 1 "assert" "@thread-a" "\"\"" "") "\n"))
      _ (spit corrupt-telemetry "")
      _ (spit unterminated-log (op 1 "assert" "@thread-a" "title" "one"))
      _ (spit unterminated-telemetry "")
      _ (write-reviewed-manifest!
         manifest-path log telemetry
         {"custom_ref" {:cardinality "multi"
                        :value_kind "ref"
                        :doc "Reviewed integration-test reference edge."
                        :rationale "The fixture explicitly targets @thread-b."}
          "opaque" {:cardinality "multi"
                    :value_kind "literal"
                    :doc "Reviewed opaque integration-test literal."
                    :rationale "The fixture value is literal test data."}}
         {"@ambiguous" {:entity_kind "vendor/opaque"
                         :rationale "Fixture-owned extension with no North structural signal."}})
      _ (write-reviewed-manifest!
         unterminated-manifest unterminated-log unterminated-telemetry {} {})
      _ (write-reviewed-manifest!
         corrupt-manifest corrupt-log corrupt-telemetry {}
         {"@thread-a" {:entity_kind "north/quarantined_legacy_artifact"
                       :rationale "Keeps the corrupt fixture classification explicit during preflight."}})
      daemon (atom (start-daemon port log telemetry))
      checks (atom [])
      check! (fn [label ok detail]
               (swap! checks conj {:label label :ok (boolean ok) :detail detail}))]
  (try
    (let [synthetic-facts [{:l "@part_of" :p "acyclic" :r "true"}
                           {:l "@part_of" :p "acyclic" :r "false"}
                           {:l "@custom_edge" :p "acyclic" :r "maybe"}]
          synthetic-schema (desired-schema synthetic-facts)
          defects (malformed-schema synthetic-facts synthetic-schema)
          by-key (into {} (map (fn [defect]
                                 [[(:predicate defect) (:field defect)] (:values defect)]))
                       defects)]
      (check! "strict schema audit detects synthetic multiple and malformed acyclic state"
              (and (= ["false" "true"] (get by-key ["part_of" "acyclic"]))
                   (= ["maybe"] (get by-key ["custom_edge" "acyclic"])))
              (pr-str by-key)))

    (let [log-before (slurp log)
          telemetry-before (slurp telemetry)
          empty-telemetry-plan (run-schema port log telemetry receipts "plan")
          missing-coordination (run-schema port
                                           (.getAbsolutePath (io/file temp "missing-coordination.log"))
                                           telemetry receipts "migrate" "--execute")
          missing-telemetry (run-schema port log
                                        (.getAbsolutePath (io/file temp "missing-telemetry.log"))
                                        receipts "migrate" "--execute")
          blank-telemetry (run-schema port log "" receipts "plan")
          directory-telemetry (run-schema port log (.getCanonicalPath temp) receipts "audit" "--strict")
          duplicate-runs (mapv (fn [args]
                                 (apply run-schema port log log receipts args))
                               [["plan"]
                                ["audit" "--strict"]
                                ["migrate" "--execute"]
                                ["repair-corrupt" "--execute" "--offline-confirm"]])
          alias-duplicate (run-schema port log log-alias receipts "plan")
          direct-before (run-schema port log telemetry receipts "migrate" "--execute")
          unterminated-before (slurp unterminated-log)
          unterminated-execute (run-schema port unterminated-log unterminated-telemetry
                                           unterminated-receipts
                                           "build-candidate" "--execute" "--offline-confirm"
                                           "--manifest" unterminated-manifest)]
      (check! "mandatory zero-byte telemetry log is accepted as a corpus member"
              (zero? (:exit empty-telemetry-plan))
              (str "exit=" (:exit empty-telemetry-plan) " err=" (:err empty-telemetry-plan)))
      (check! "missing coordination log fails before migration work"
              (and (not (zero? (:exit missing-coordination)))
                   (str/includes? (:err missing-coordination) "coordination corpus log is missing or unreadable"))
              (:err missing-coordination))
      (check! "missing configured telemetry log fails before migration work"
              (and (not (zero? (:exit missing-telemetry)))
                   (str/includes? (:err missing-telemetry) "telemetry corpus log is missing or unreadable"))
              (:err missing-telemetry))
      (check! "blank telemetry configuration is rejected instead of silently omitted"
              (and (not (zero? (:exit blank-telemetry)))
                   (str/includes? (:err blank-telemetry) "telemetry corpus log path is required"))
              (:err blank-telemetry))
      (check! "non-file telemetry corpus path is rejected as unreadable"
              (and (not (zero? (:exit directory-telemetry)))
                   (str/includes? (:err directory-telemetry) "telemetry corpus log is missing or unreadable"))
              (:err directory-telemetry))
      (check! "every verb rejects a directly duplicated corpus path"
              (every? #(and (not (zero? (:exit %)))
                            (str/includes? (:err %) "same canonical path"))
                      duplicate-runs)
              (pr-str (mapv #(select-keys % [:exit :err]) duplicate-runs)))
      (check! "canonical path identity rejects a symlink alias"
              (and (not (zero? (:exit alias-duplicate)))
                   (str/includes? (:err alias-duplicate) "same canonical path"))
              (:err alias-duplicate))
      (check! "direct migration is disabled before the first write"
              (and (not (zero? (:exit direct-before)))
                   (str/includes? (:err direct-before) "direct migrate --execute is disabled")
                   (= log-before (slurp log))
                   (= telemetry-before (slurp telemetry)))
              (str "exit=" (:exit direct-before) " err=" (:err direct-before)))
      (check! "execute rejects an unterminated append boundary before coordinator work"
              (and (not (zero? (:exit unterminated-execute)))
                   (str/includes? (:err unterminated-execute) "not newline-terminated")
                   (= unterminated-before (slurp unterminated-log))
                   (not (.exists (io/file unterminated-receipts))))
              (str "exit=" (:exit unterminated-execute) " err=" (:err unterminated-execute)))
      (check! "corpus validation failures perform no work or receipt writes"
              (and (= log-before (slurp log))
                   (= telemetry-before (slurp telemetry))
                   (not (.exists (io/file receipts))))
              nil))

    (let [invalid-plan (run-schema port invalid-log invalid-telemetry invalid-receipts
                                   "plan" "--verbose")
          invalid-audit (run-schema port invalid-log invalid-telemetry invalid-receipts
                                    "audit" "--strict")]
      (check! "malformed acyclic declarations remain a nonwriting diagnostic plan"
              (and (zero? (:exit invalid-plan))
                   (not (re-find #"(?m)^  set @part_of\s+acyclic\s" (:out invalid-plan)))
                   (not (re-find #"(?m)^  set @custom_edge\s+acyclic\s" (:out invalid-plan))))
              (:out invalid-plan))
      (check! "strict audit reports a malformed persisted acyclic declaration exactly"
              (and (= 1 (:exit invalid-audit))
                   (str/includes? (:out invalid-audit) "@custom_edge acyclic = [\"maybe\"]"))
              (str "exit=" (:exit invalid-audit) " out=" (:out invalid-audit))))

    (check! "real Fram coordinator starts" (eventually #(port-open? port)) nil)

    (let [valid-plan (run-schema port log telemetry receipts "plan" "--verbose")]
      (check! "valid explicit true and false acyclic policy require no rewrite"
              (and (zero? (:exit valid-plan))
                   (not (re-find #"(?m)^  set @part_of\s+acyclic\s" (:out valid-plan)))
                   (not (re-find #"(?m)^  set @depends_on\s+acyclic\s" (:out valid-plan))))
              (:out valid-plan)))

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
              (= 1 (:exit before)) (str "exit=" (:exit before) " out=" (:out before))))

    (let [corrupt-before (run-schema port corrupt-log corrupt-telemetry corrupt-receipts
                                     "audit" "--strict")
          refused (run-schema port corrupt-log corrupt-telemetry corrupt-receipts
                              "build-candidate" "--execute" "--offline-confirm"
                              "--manifest" corrupt-manifest)]
      (check! "strict audit names the malformed predicate"
              (and (= 1 (:exit corrupt-before))
                   (str/includes? (:out corrupt-before) "corrupt predicate"))
              (:out corrupt-before))
      (check! "candidate preflight refuses to register around corrupt bytes"
              (and (not (zero? (:exit refused)))
                   (str/includes? (:out refused) "corrupt-facts-present")
                   (str/includes? (:err refused) "zero coordinator writes attempted"))
              (str "exit=" (:exit refused) " out=" (:out refused) " err=" (:err refused))))

    (let [log-before (slurp corrupt-log)
          telemetry-before (slurp corrupt-telemetry)
          diagnostic (run-schema port corrupt-log corrupt-telemetry corrupt-receipts
                                 "repair-corrupt")
          refused-repair (run-schema port corrupt-log corrupt-telemetry corrupt-receipts
                                     "repair-corrupt" "--execute" "--offline-confirm")]
      (check! "repair-corrupt dry-run names the exact malformed triple"
              (and (zero? (:exit diagnostic))
                   (str/includes? (:out diagnostic) "1 live non-registrable predicate fact(s)")
                   (str/includes? (:out diagnostic) "would retract exact triple")
                   (str/includes? (:out diagnostic) ":p \"\\\"\\\"\""))
              (str "exit=" (:exit diagnostic) " out=" (:out diagnostic) " err=" (:err diagnostic)))
      (check! "repair-corrupt execute fails closed without the corpus transaction surface"
              (and (not (zero? (:exit refused-repair)))
                   (str/includes? (:err refused-repair) "corpus transaction required")
                   (str/includes? (:err refused-repair) "no bytes written"))
              (str "exit=" (:exit refused-repair) " out=" (:out refused-repair)
                   " err=" (:err refused-repair)))
      (check! "diagnostic and refused repair leave both corpus logs byte-identical"
              (and (= log-before (slurp corrupt-log))
                   (= telemetry-before (slurp corrupt-telemetry))
                   (not (.exists (io/file corrupt-receipts))))
              nil))

    (let [migrate (run-schema port log telemetry receipts
                              "build-candidate" "--execute" "--offline-confirm"
                              "--manifest" manifest-path)
          receipt-files (when (.isDirectory (io/file receipts))
                          (vec (.listFiles (io/file receipts))))
          receipt-data (when (= 1 (count receipt-files))
                         (edn/read-string (slurp (first receipt-files))))]
      (check! "offline candidate build exits 0 on the completely reviewed corpus"
              (zero? (:exit migrate))
              (str "exit=" (:exit migrate) " out=" (:out migrate) " err=" (:err migrate)))
      (check! "only the converged candidate build emits a persisted receipt"
              (and (str/includes? (:out migrate) "receipt ")
                   (.isDirectory (io/file receipts))
                   (= 1 (count receipt-files))
                   (= "north-schema-candidate-build/v1" (:format receipt-data))
                   (true? (:converged receipt-data))
                   (true? (:post_matches_simulation receipt-data))
                   (= (:actions_acknowledged receipt-data)
                      (count (:requested_action_identities receipt-data)))
                   (empty? (:remaining_action_identities receipt-data)))
              (str (:out migrate) " receipt=" (pr-str receipt-data))))

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
      (check! "valid explicit false acyclic policy survives migration"
              (= #{"false"} (values facts "@part_of" "acyclic")) nil)
      (check! "valid explicit true acyclic policy survives migration"
              (= #{"true"} (values facts "@depends_on" "acyclic")) nil)
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
      (check! "reviewed ambiguous extension receives only its explicit classification"
              (= #{"vendor/opaque"} (values facts "@ambiguous" "entity_kind")) nil))

    (let [second (run-schema port log telemetry receipts "plan")]
      (check! "post-candidate plan is a zero-delta idempotence proof"
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
