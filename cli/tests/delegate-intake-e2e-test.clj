#!/usr/bin/env bb
;; delegate-intake-e2e-test.clj — the Clojure delegate/spawn INTAKE ladder end to
;; end: capture -> structured-read proof -> projector -> admission -> child
;; identity/ack. SDK admission had a green suite while this seam had none, and it
;; shipped broken (thread 019fb399).
;;
;; Daemon-free: agents-cli loads as a library, the coordinator seam is stubbed at
;; north.coord, capture is a real subprocess against a stub `north`, and the child
;; is a real detached process acknowledged through production await-startup.
;;   bb cli/tests/delegate-intake-e2e-test.clj
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(def temp-dir
  (.toFile (java.nio.file.Files/createTempDirectory
            "north-delegate-intake-" (make-array java.nio.file.attribute.FileAttribute 0))))

(def results (atom []))
(defn check [label pass?]
  (swap! results conj [label (boolean pass?)])
  (println (format "  [%s] %s" (if pass? "PASS" "FAIL") label)))

;; --- stub `north` CLI -------------------------------------------------------
;; Emits the exact structured capture receipt and records its argv, so the test
;; can prove intake never shells a whole-corpus read.
(def argv-log (io/file temp-dir "north-argv.log"))
(def capture-delay-file (io/file temp-dir "capture-delay-seconds"))
(def thread-id "019fb400-0000-7000-8000-00000000abcd")

(def stub-north (io/file temp-dir "north"))
(spit stub-north
      (str "#!/usr/bin/env bash\n"
           "printf '%s\\n' \"$*\" >> " (.getPath argv-log) "\n"
           "if [ \"$1\" = capture ]; then\n"
           "  sleep \"$(cat " (.getPath capture-delay-file) ")\"\n"
           ;; The receipt must echo the EXACT requested title: capture readback
           ;; compares them and a placeholder would pass a partial capture.
           "  printf '{\"id\":\"" thread-id "\",\"thread\":\"@" thread-id "\","
           "\"title\":\"%s\",\"path\":\"/dev/null\",\"expected\":9,"
           "\"committed\":9,\"complete\":true,\"reason\":\"captured\"}\\n' \"$2\"\n"
           "  exit 0\n"
           "fi\n"
           "exit 0\n"))
(.setExecutable stub-north true)
(spit capture-delay-file "0")

(System/setProperty "north.agents.lib" "1")
(def previous-north-bin (System/getenv "NORTH_BIN"))
(load-file (str root "/cli/agents-cli.clj"))

;; NORTH-CLI is resolved at load time; rebind it to the stub for this fixture.
(def ^:dynamic *stub-cli* (.getPath stub-north))

(defn stub-title-receipt [title]
  (json/generate-string
   {:id thread-id :thread (str "@" thread-id) :title title
    :path "/dev/null" :expected 9 :committed 9 :complete true :reason "captured"}))

(defn stub-rows [title]
  [["kind" "thread"] ["title" title] ["committed" "2026-07-30"]
   ["done_when" "bb cli/tests/delegate-intake-e2e-test.clj -> exit 0"]])

(defn argv-lines []
  (if (.isFile argv-log) (str/split-lines (slurp argv-log)) []))

(defn died [f]
  (try (f) ::no-death
       (catch clojure.lang.ExceptionInfo e (.getMessage e))))

(try
  ;; ---- A. structured-read proof: exact subject, never a whole-corpus read ---
  (let [asked (atom [])]
    (with-redefs [delegate-die (fn [m] (throw (ex-info m {:delegate-die true})))
                  north.coord/show-rows (fn [port subject]
                                          (swap! asked conj [port subject])
                                          (stub-rows "A0 intake gate"))]
      (let [thread (read-delegate-thread! thread-id)]
        (check "intake proves a delegate thread through the exact-subject projection"
               (and (= thread-id (:id thread))
                    (= "A0 intake gate" (:title thread))
                    (:committed? thread)
                    (= 1 (count (:done-when thread)))))
        (check "intake asks the coordinator for exactly one canonical subject"
               (= [[7977 (str "@" thread-id)]] @asked))
        (check "intake never shells a whole-corpus `north json show`"
               (not-any? #(str/starts-with? % "json show") (argv-lines))))))

  ;; ---- B. a degraded coordinator is not an absent thread -------------------
  (let [attempts (atom 0)]
    (with-redefs [delegate-die (fn [m] (throw (ex-info m {:delegate-die true})))
                  structured-read-retry-ms 1
                  north.coord/show-rows (fn [_ _]
                                          (swap! attempts inc)
                                          (throw (ex-info "coordinator response deadline exceeded"
                                                          {:type :coordinator-response-timeout})))]
      (let [message (died #(read-delegate-thread! thread-id))]
        (check "an unreadable coordinator fails as an unproven read, not a missing thread"
               (and (string? message)
                    (str/includes? message "cannot prove delegate thread")
                    (not (str/includes? message "is not a title-bearing"))))
        (check "the structured read retries a transient coordinator failure"
               (= structured-read-attempts @attempts)))))

  (with-redefs [north.coord/show-rows (fn [_ _] [["owner" "personal"]])]
    (check "admission still refuses a well-formed id that names no thread"
           (= :untitled (thread-title-verdict thread-id))))
  (with-redefs [north.coord/show-rows
                (fn [_ _] (throw (ex-info "coordinator response deadline exceeded" {})))]
    (check "admission separates an unreadable coordinator from an absent thread"
           (= :unreadable (thread-title-verdict thread-id))))

  ;; ---- C. capture: the real subprocess boundary and its budget --------------
  (with-redefs [delegate-die (fn [m] (throw (ex-info m {:delegate-die true})))
                NORTH-CLI *stub-cli*
                north.coord/show-rows (fn [_ _] (stub-rows "Capture through the durable boundary"))]
    (let [captured (capture-delegate-thread! "Capture through the durable boundary")]
      (check "capture accepts an exact structured receipt and reads the thread back"
             (and (= thread-id (:id captured))
                  (= :captured (:source captured))
                  (= "Capture through the durable boundary" (:title captured))))
      (check "capture is issued through the durable North capture verb"
             (some #(str/starts-with? % "capture ") (argv-lines)))))

  ;; The budget must exceed a whole-log render behind bin/north's verb slot; a
  ;; capture slower than the old 15s deadline must still be admitted.
  (spit capture-delay-file "16")
  (with-redefs [delegate-die (fn [m] (throw (ex-info m {:delegate-die true})))
                NORTH-CLI *stub-cli*
                north.coord/show-rows (fn [_ _] (stub-rows "Slow capture is still a capture"))]
    (let [captured (capture-delegate-thread! "Slow capture is still a capture")]
      (check "a capture slower than the old 15s deadline is still admitted"
             (and (= thread-id (:id captured))
                  (>= delegate-capture-timeout-ms 120000)))))
  (spit capture-delay-file "0")

  ;; ---- D. projector leg: both modes of the admission projector -------------
  (let [projector (str root "/cli/orchestration-project-cli.clj")
        invoked (p/shell {:out :string :err :string :continue true} "bb" projector)
        library (p/shell {:out :string :err :string :continue true}
                         "bb" "-e" (str "(load-file " (pr-str projector) ") "
                                        "(println (some? (resolve 'project-bundle)))"))]
    (check "the admission projector runs its main guard instead of crashing at load"
           (and (= 2 (:exit invoked))
                (str/includes? (:out invoked) "staffing | provider")))
    (check "the admission projector still loads as a library without executing"
           (and (zero? (:exit library))
                (= "true" (str/trim (:out library))))))

  ;; ---- E. child identity + startup acknowledgement -------------------------
  (let [log (io/file temp-dir "child.log")
        identity-file (io/file temp-dir "child-identity.json")
        base-env (into {} (System/getenv))
        child ["bash" "-c" "printf published > \"$NORTH_TEST_IDENTITY_FILE\"; sleep 20"]
        process (north.spawn-process/launch-detached!
                 child (assoc base-env "NORTH_TEST_IDENTITY_FILE" (.getPath identity-file)) log)
        facts (fn [_]
                (when (.isFile identity-file)
                  (let [base {"kind" "lane" "role" "executor"
                              "goal" "run the intake gate"
                              "provider" "anthropic" "provider_target" "claude-personal"
                              "live_input" "streaming" "live_input_state" "armed"
                              "live_input_epoch" "00000000-0000-4000-8000-00000000e2e3"
                              "model" "claude-sonnet-5" "effort" "low"
                              "composition_kind" "preset" "composition_id" "executor"
                              "composition_overrides" "[]"
                              "repo" "north" "spawned_at" "2026-07-30T00:00:00Z"
                              "display_handle" "anthropic-sonnet-low-executor-intake"
                              "display_name" "anthropic:claude-personal · sonnet · low · orchestration:executor"}]
                    (assoc base "identity_manifest_sha256"
                           (north.agent-provenance/manifest-sha256 base)))))
        startup (north.spawn-process/await-startup
                 process "lane-intake-e2e" log facts (constantly true)
                 :timeout-ms 10000 :poll-ms 20)]
    (check "a launched managed child publishes identity and acknowledges startup"
           (and (= :ready (:status startup))
                (= "anthropic-sonnet-low-executor-intake" (:handle startup))))
    (north.spawn-process/stop-process! process))

  (check "the startup budget outlasts the child's own bounded admission preflight"
         (>= north.spawn-process/default-startup-timeout-ms 120000))

  (finally
    (doseq [file (reverse (file-seq temp-dir))] (io/delete-file file true))))

(let [rows @results pass (count (filter second rows))]
  (println (format "\ndelegate intake e2e: %d / %d PASS" pass (count rows)))
  (System/exit (if (= pass (count rows)) 0 1)))
