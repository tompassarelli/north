#!/usr/bin/env bb
;; Real Fram socket gate for north.delivery-evidence-internal/record! under
;; concurrent write traffic (thread 019f9f12-b5fa).
;;
;; The bug: commit-record-once! retried its assert-at-version race with a
;; FIXED 16-attempt tight loop and no backoff/deadline. Every other commit path
;; sharing the coordinator's global version (reserve!'s doseq, assert-after-
;; read!) already retries against an absolute deadline with equal-jitter
;; backoff, so unrelated traffic buys retries instead of a false refusal. A
;; live, valid, in-bounds evidence write must never be refused just because the
;; coordinator was busy; if a refusal IS correct, it must name its own cause
;; rather than falling through to the generic rejection line.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file *file*)) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                (str root "/../fram")))))
(when-not (.isFile (io/file fram "coord_daemon.clj"))
  (throw
   (ex-info
    "Fram checkout not found; set FRAM_PATH or clone it beside North"
    {:fram fram})))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/terminal-projection.clj"))
(load-file (str root "/cli/delivery-evidence-internal.clj"))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
      true)
    (catch Exception _ false)))

(defn eventually [f]
  (loop [remaining 200]
    (cond
      (try (f) (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(let [port (free-port)
      dir (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-evidence-contention"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "facts.log")
      _ (spit log "")
      daemon
      (proc/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"
                    "FRAM_LOG" (.getPath log)
                    "FRAM_TELEMETRY_LOG" (.getPath (io/file dir "telemetry.log"))
                    "FRAM_THREADS" (.getPath (io/file dir "threads"))}}
       "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
       (str port) (.getPath log))
      checks (atom [])
      check! (fn [label value]
               (swap! checks conj [label (boolean value)]))]
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check! "real Fram daemon starts"
            (eventually #(port-open? port)))

    ;; --- Set up one thread with N*M active done_when bars and reserve one run
    ;; against it, exactly as a managed lane's provider process would. ---
    (let [thread "@thread:evidence-contention"
          writer-count 4
          records-per-writer 4
          bars-by-writer
          (mapv
           (fn [writer-index]
             (mapv
              #(str "bar-" writer-index "-" %)
              (range records-per-writer)))
           (range writer-count))
          bars (vec (mapcat identity bars-by-writer))
          run "@run:evidence-contention"
          reporter "@agent:evidence-contention"
          capability (str/join (repeat 64 "a"))
          capability-sha256 (north.terminal-projection/sha256 capability)]
      (north.coord/append! port thread "title" "evidence contention fixture")
      (doseq [bar bars]
        (north.coord/append! port thread "done_when" bar))
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})

      ;; --- Flood the coordinator with UNRELATED global-version churn from a
      ;; separate writer while every bar is recorded CONCURRENTLY, each from
      ;; its OWN `bb` subprocess — exactly the shape production traffic takes
      ;; (one CLI invocation per `north evidence record`, launched by many
      ;; lanes at once), so no in-process budget atom is shared across bars.
      ;; Before the fix this reliably exhausted the fixed 16-try loop and
      ;; refused a perfectly valid, live-reservation write. ---
      (let [running? (atom true)
            churn-writes (atom 0)
            writer
            (future
              (while @running?
                (north.coord/append!
                 port "@unrelated-evidence-churn" "noise"
                 (str (swap! churn-writes inc)))
                (Thread/sleep 15)))
            writer-path (str root "/cli/delivery-evidence-internal.clj")
            submit
            (fn []
              (->> bars-by-writer
                   (pmap
                    (fn [writer-bars]
                      (mapv
                       (fn [bar]
                         (let [request
                               (json/generate-string
                                {"run" run "thread" thread "reporter" reporter
                                 "capability" capability
                                 "bar" bar "observed" (str "exit 0 " bar)})
                               outcome
                               (proc/process
                                {:in request :out :string :err :string
                                 :extra-env {"FRAM_LOG" (.getPath log)}}
                                "bb" writer-path (str port) "record")
                               done @outcome]
                           {:bar bar :exit (:exit done) :err (:err done)}))
                       writer-bars)))
                   (doall)
                   (mapcat identity)
                   vec))
            submissions
            (try
              [(submit) (submit)]
              (finally
                (reset! running? false)
                @writer))
            results (vec (mapcat identity submissions))]
        (check! "unrelated churn actually raced the commits"
                (>= @churn-writes 5))
        (check! "N writers x M records plus exact replays all acknowledge"
                (every? #(zero? (:exit %)) results))
        (when-let [failed (seq (filter #(not (zero? (:exit %))) results))]
          (println "  [FAILURES]" (mapv :err failed)))

        (let [stored
              (mapv
               #(json/parse-string %)
               (north.coord/many port run "run_bar_evidence"))
              stored-bars (mapv #(get % "bar") stored)
              projected
              (north.coord/many port thread "bar_evidence")]
          (check! "all N*M evidence records are stored with zero loss"
                  (= (set bars) (set stored-bars)))
          (check! "exact replay is idempotent: zero run-record duplicates"
                  (and (= (count bars) (count stored))
                       (every? #(= 1 %)
                               (vals (frequencies stored-bars)))))
          (check! "exact replay is idempotent: zero thread-projection duplicates"
                  (= (count bars) (count projected))))))

    ;; --- The refusal, when contention genuinely cannot converge inside its
    ;; budget, must NAME ITSELF rather than reporting the generic rejection
    ;; line every other cause already shares. ---
    (let [thread "@thread:evidence-contention-exhaustion"
          run "@run:evidence-contention-exhaustion"
          reporter "@agent:evidence-contention-exhaustion"
          capability (str/join (repeat 64 "b"))
          capability-sha256 (north.terminal-projection/sha256 capability)]
      (north.coord/append! port thread "title" "evidence exhaustion fixture")
      (north.coord/append! port thread "done_when" "only-bar")
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})
      ;; Deterministic non-convergence, not a race against real timing: every
      ;; assert-at-version attempt for THIS run is forced to conflict, so the
      ;; retry budget is GUARANTEED to exhaust rather than depending on a
      ;; background writer happening to land in a narrow window.
      (with-redefs
       [north.coord/send-op
        (let [original north.coord/send-op]
          (fn [target-port operation]
            (if (and (= :assert-at-version (:op operation))
                     (= "run_bar_evidence" (:p operation))
                     (= run (:te operation)))
              {:reject :conflict}
              (original target-port operation))))
        north.delivery-evidence-internal/commit-retry-budget-ms 50]
        (let [caught
              (try
                (north.delivery-evidence-internal/record!
                 port {"run" run "thread" thread "reporter" reporter
                       "capability" capability
                       "bar" "only-bar" "observed" "exit 0"})
                nil
                (catch Exception error error))]
          (check! "a non-converging commit throws"
                  (some? caught))
          (check! "the refusal names its own cause (contention, not a generic reject)"
                  (and (some? caught)
                       (str/includes?
                        (.getMessage caught)
                        "RETRYABLE: evidence commit contention"))))))

    (finally
      (proc/process ["kill" (str (:pid daemon))])
      (doseq [[label ok?] @checks]
        (println (if ok? "  [OK]" "  [FAIL]") label))
      (System/exit
       (if (every? second @checks) 0 1)))))
