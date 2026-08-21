#!/usr/bin/env bb
(def root
  (.getCanonicalPath
   (java.io.File.
    (.getParent (java.io.File. (System/getProperty "babashka.file")))
    "../..")))
(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(let [scratch (.toFile
               (java.nio.file.Files/createTempDirectory
                "north-delegate-handoff-"
                (make-array java.nio.file.attribute.FileAttribute 0)))
      artifact (java.io.File. scratch "handoff.json")
      completed (java.io.File. scratch "completed.json")
      malformed (java.io.File. scratch "malformed.json")
      public-mode (java.io.File. scratch "public-mode.json")
      artifact-link (.toPath (java.io.File. scratch "handoff-link.json"))
      private-permissions
      #{java.nio.file.attribute.PosixFilePermission/OWNER_READ
        java.nio.file.attribute.PosixFilePermission/OWNER_WRITE}
      document
      (array-map
       :version 1
       :reason "session_hard_cap"
       :writtenAt "2026-08-21T07:00:00.000Z"
       :hardCapMs 3600000
       :agentId "lane-capped-session"
       :threadId "019f9537-a5d3-7000-8000-000000000001"
       :goal "finish the exact capped deliverable"
       :repo root
       :worktree (str root "/worktrees/capped-session")
       :branch "capped-session"
       :nextAction delegate-handoff-next-action
       :completionClaimed false)
      write-private!
      (fn [file value]
        (spit file (str (json/generate-string value) "\n"))
        (java.nio.file.Files/setPosixFilePermissions
         (.toPath file) private-permissions))]
  (try
    (write-private! artifact document)
    (write-private! completed (assoc document :completionClaimed true))
    (write-private! malformed (assoc document :unexpected "not closed"))
    (spit public-mode (str (json/generate-string document) "\n"))
    (java.nio.file.Files/setPosixFilePermissions
     (.toPath public-mode)
     #{java.nio.file.attribute.PosixFilePermission/OWNER_READ
       java.nio.file.attribute.PosixFilePermission/OWNER_WRITE
       java.nio.file.attribute.PosixFilePermission/GROUP_READ})
    (java.nio.file.Files/createSymbolicLink
     artifact-link (.toPath artifact)
     (make-array java.nio.file.attribute.FileAttribute 0))
    (let [captured (atom nil)]
      (with-redefs [north.topology-authority/require-coordination! (fn [_] nil)
                    resolved-spawn-topology (fn [_] "worker")
                    cmd-spawn (fn [args]
                                (reset! captured
                                        {:args args :request *delegate-request*}))]
        (cmd-delegate ["--handoff" (.getPath artifact)
                       "--role" "analyst" "--dry-run"]))
      (check
       "valid private artifact supplies the exact managed task, thread, and context"
       (and (= ["analyst" (:goal document) "--dry-run"]
               (:args @captured))
            (= (:goal document) (get-in @captured [:request :task]))
            (= (:threadId document)
               (get-in @captured [:request :explicit-thread]))
            (= document
               (json/parse-string
                (get-in @captured [:request :context]) true)))))
    (doseq [[label args expected]
            [["completed artifact is refused before spawn"
              ["--handoff" (.getPath completed) "--role" "analyst"]
              "incomplete v1 handoff contract"]
             ["malformed artifact is refused before spawn"
              ["--handoff" (.getPath malformed) "--role" "analyst"]
              "incomplete v1 handoff contract"]
             ["symlinked artifact is refused before spawn"
              ["--handoff" (str artifact-link) "--role" "analyst"]
              "regular non-symlink file"]
             ["non-private artifact is refused before spawn"
              ["--handoff" (.getPath public-mode) "--role" "analyst"]
              "must have mode 0600"]
             ["conflicting explicit thread is refused before artifact read or spawn"
              ["--handoff" (.getPath artifact) "--role" "analyst"
               "--thread" "other-thread"]
              "supplies its exact thread"]]]
      (let [spawned? (atom false)
            message
            (try
              (with-redefs [north.topology-authority/require-coordination! (fn [_] nil)
                            cmd-spawn (fn [_] (reset! spawned? true))
                            delegate-die
                            (fn [m]
                              (throw (ex-info m {:delegate-die true})))]
                (cmd-delegate args)
                ::no-error)
              (catch clojure.lang.ExceptionInfo error
                (.getMessage error)))]
        (check label
               (and (string? message)
                    (.contains ^String message expected)
                    (false? @spawned?)))))
    (let [message
          (try
            (with-redefs [read-delegate-thread!
                          (fn [id]
                            {:id id
                             :title "settled elsewhere"
                             :facts [{:predicate "title"
                                      :value "settled elsewhere"}
                                     {:predicate "outcome"
                                      :value "completed after cap"}]})
                          delegate-die
                          (fn [m]
                            (throw (ex-info m {:delegate-die true})))]
              (resolve-delegate-thread!
               {:task (:goal document)
                :explicit-thread (:threadId document)
                :handoff? true}
               false)
              ::no-error)
            (catch clojure.lang.ExceptionInfo error
              (.getMessage error)))]
      (check "artifact whose exact thread became terminal is refused before provider spawn"
             (and (string? message)
                  (.contains ^String message "is already terminal (outcome)"))))
    (finally
      (doseq [file (reverse (file-seq scratch))]
        (try (.delete ^java.io.File file) (catch Throwable _ nil))))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\ndelegate handoff adoption: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
