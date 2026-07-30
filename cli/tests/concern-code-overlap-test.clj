#!/usr/bin/env bb
;; ============================================================================
;; concern-code-overlap-test.clj — thread 019f1010-2705 CLI wiring.
;; Boots a throwaway SPINE board + a throwaway warm CODE daemon (over a tiny
;; freshly ingested two-definition corpus) and drives bin/concern's CLI
;; end-to-end, asserting:
;;   - declare resolves a code-NODE footprint onto the CODE port; the spine carries
;;     code_port but NEVER a footprint fact (no port partition, acceptance 6);
;;   - overlap surfaces a caller-coupled peer via the daemon's blast-closure join
;;     (a footprint declared seconds ago, no render/merge — acceptance 2/3);
;;   - status appends a monotone `reached` maturity level (set-single! is gone, 4);
;;   - a repo with no code daemon DEGRADES to the path-string footprint (acceptance 7).
;; Daemon-side scope-correctness + rename-stability live in fram's
;; tests/coord_concern_overlap_test.clj; this guards the north CLI seam.
;; SKIPs cleanly if Fram's compiled out/ or the Beagle CLI is absent.
;;   bb cli/tests/concern-code-overlap-test.clj
;; ============================================================================
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[babashka.process :as p])

(def test-script (or (System/getProperty "babashka.file") *file*))
(def lode (-> (io/file test-script)
              .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def fram (str (System/getProperty "user.home") "/code/fram/main"))
(def beagle-home
  (or (System/getenv "BEAGLE_HOME")
      (str (System/getProperty "user.home") "/code/beagle/main")))
(def beagle-bin
  (or (System/getenv "FRAM_BEAGLE")
      (str beagle-home "/bin/beagle")))
(when-not (and (.exists (io/file (str fram "/out")))
               (.canExecute (io/file beagle-bin)))
  (println "SKIP — fram out/ or the Beagle CLI is absent.")
  (System/exit 0))

(defn op [port log o]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout s 30000)
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w
              (.getBytes
               (str (pr-str {:op :for-log
                             :expected-log (.getCanonicalPath (io/file log))
                             :request o})
                    "\n")))
      (.flush w)
      (edn/read-string (.readLine r)))))
(defn port-free? [p] (try (with-open [s (java.net.Socket.)]
                            (.connect s (java.net.InetSocketAddress. "127.0.0.1" (int p)) 250) false)
                          (catch Exception _ true)))
(defn ephemeral-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.setReuseAddress socket false)
    (.getLocalPort socket)))
(def ports
  (loop [chosen []]
    (if (= 4 (count chosen))
      chosen
      (let [candidate (ephemeral-port)]
        (recur (if (and (port-free? candidate)
                        (not (contains? (set chosen) candidate)))
                 (conj chosen candidate)
                 chosen))))))
(def spine (nth ports 0))
(def cport (nth ports 1))
(def spine-telemetry-port (nth ports 2))
(def code-telemetry-port (nth ports 3))
(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-concern-code-overlap"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(defn temp-path [name] (.getCanonicalPath (io/file tmp name)))
(defn delete-tree! [root]
  (doseq [file (reverse (file-seq root))]
    (io/delete-file file true)))
(def spine-log (temp-path "spine.log"))
(def spine-telemetry-log (temp-path "spine-telemetry.log"))
(def code-cpy (temp-path "code.log"))
(def code-telemetry-log (temp-path "code-telemetry.log"))
(def code-source (temp-path "overlap.bclj"))
(def spine-output (io/file tmp "spine.out"))
(def code-output (io/file tmp "code.out"))
(doseq [path [spine-log spine-telemetry-log code-telemetry-log]]
  (spit path ""))
(spit code-source
      (str "#lang beagle/clj\n\n"
           "(defn callee [] nil)\n"
           "(defn caller [] (callee))\n"))
(def ingest-result
  @(p/process {:dir fram
               :extra-env {"BEAGLE_HOME" beagle-home
                           "FRAM_BEAGLE" beagle-bin}
               :out :string
               :err :string}
              "bin/fram-ingest-code" code-source
              "--root" (.getCanonicalPath tmp)
              "--out" code-cpy))
(when-not (zero? (:exit ingest-result))
  (binding [*out* *err*]
    (println "ABORT — failed to ingest the isolated overlap corpus:")
    (println (:err ingest-result)))
  (delete-tree! tmp)
  (System/exit 1))

;; Current flat migration obtains link-ness from schema facts. Ingest preserves
;; integer AST objects as @node references but emits only the AST itself, so seed
;; every actual fN predicate plus the derived bound_to edge in this disposable
;; log before ordinary boot. This stays valid when migration once again honors
;; the documented @ convention.
(def ingested-lines
  (with-open [reader (io/reader code-cpy)]
    (mapv edn/read-string (line-seq reader))))
(def ast-ref-predicates
  (-> (->> ingested-lines
           (map :p)
           (filter #(and (string? %) (re-matches #"f[0-9]+" %)))
           set)
      (conj "bound_to")
      sort))
(def max-ingest-tx (reduce max 0 (keep :tx ingested-lines)))
(with-open [writer (io/writer code-cpy :append true)]
  (doseq [[offset predicate] (map-indexed vector ast-ref-predicates)]
    (.write writer
            (str (pr-str {:tx (+ max-ingest-tx offset 1)
                          :op "assert"
                          :l (str "@" predicate)
                          :p "value_kind"
                          :r "ref"
                          :ts (str (java.time.Instant/now))
                          :by "concern-code-overlap-test"})
                 "\n"))))

(def spine-env
  {"FRAM_LOG" spine-log
   "FRAM_TELEMETRY_LOG" spine-telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str spine-telemetry-port)})
(def code-env
  {"FRAM_LOG" code-cpy
   "FRAM_TELEMETRY_LOG" code-telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str code-telemetry-port)})

(defn spawn-spine []
  (p/process {:dir fram :out spine-output
              :extra-env (assoc spine-env
                                "FRAM_REQUIRE_LOG_FENCE" "1"
                                "FRAM_SINGLE_VALUED" "code_port code_log")
              :err :out}
             "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
             (str spine) spine-log))
(defn spawn-code []
  (p/process {:dir fram
              :extra-env (assoc code-env "FRAM_REQUIRE_LOG_FENCE" "1")
              :out code-output
              :err :out}
             "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
             (str cport) code-cpy))
(println "booting spine" spine "+ code" cport
         "concurrently over isolated fixture logs (readiness budget: 60s)…")
(def sp (spawn-spine))
(def cp (spawn-code))
(def daemons [{:name "spine" :port spine :process sp :output spine-output}
              {:name "code" :port cport :process cp :output code-output}])
(defn process-alive? [process]
  (try (p/alive? process) (catch Throwable _ false)))
(defn daemon-state [{:keys [name port process]}]
  {:name name
   :port port
   :ready? (not (port-free? port))
   :alive? (process-alive? process)})
(defn killall []
  (doseq [{:keys [process]} daemons]
    (try (p/destroy-tree process) (catch Throwable _ nil))))
(defn cleanup []
  (killall)
  (delete-tree! tmp))
(.addShutdownHook (Runtime/getRuntime) (Thread. cleanup))
(defn await-daemons []
  (let [started (System/nanoTime)]
    (loop [attempt 0]
      (let [states (mapv daemon-state daemons)
            elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
        (cond
          (some (comp not :alive?) states)
          {:ready? false :reason :process-exited :elapsed-ms elapsed-ms :states states}

          (every? :ready? states)
          {:ready? true :elapsed-ms elapsed-ms :states states}

          (>= attempt 240)
          {:ready? false :reason :timeout :elapsed-ms elapsed-ms :states states}

          :else
          (do (Thread/sleep 250) (recur (inc attempt))))))))
(defn output-text [file]
  (if (.exists ^java.io.File file)
    (slurp file)
    "<output file absent>"))
(def boot-result (await-daemons))
(when-not (:ready? boot-result)
  (killall)
  (let [settled
        (into {}
              (map (fn [{:keys [name process]}]
                     [name (try (deref process 5000 nil)
                                (catch Throwable error
                                  {:diagnostic-error (.getMessage error)}))]))
              daemons)]
    (binding [*out* *err*]
      (println "ABORT — throwaway daemons failed to start:" (:reason boot-result)
               "after" (:elapsed-ms boot-result) "ms")
      (doseq [{:keys [name port alive? ready?]} (:states boot-result)
              :let [daemon (first (filter #(= name (:name %)) daemons))
                    result (get settled name)]]
        (println " " name "port=" port "alive=" alive? "ready=" ready?
                 "exit=" (or (:exit result) "<unavailable>"))
        (println (str "  " name ".out:") (output-text (:output daemon))))))
  (cleanup)
  (System/exit 1))

(def fails (atom 0))
(defn check [label ok?] (println (str "  " (if ok? "PASS" "FAIL") " — " label)) (when-not ok? (swap! fails inc)))
(defn cli-result [env & args]
  @(apply p/process {:dir lode :extra-env env :out :string :err :string}
          "bb" "cli/concern-cli.clj" (str spine) args))
(defn cli [env & args]
  (:out (apply cli-result env args)))

;; Address the seed by stable module/name, then consume only the daemon-returned
;; opaque node IDs. The fenced :blast is also the first warm call-graph read.
(def some-blast
  (op cport code-cpy {:op :blast :module "overlap" :name "callee"}))
(when-not (pos? (:count some-blast 0))
  (binding [*out* *err*]
    (println "ABORT — isolated corpus has no caller-bearing binding:"
             some-blast))
  (cleanup)
  (System/exit 1))
(def node (:node some-blast))
(def caller (first (:blast some-blast)))
(println "fixture node" node "->" (:count some-blast) "callers; using caller" caller)
(check "warm daemon resolves a code node with callers" (and node caller (pos? (:count some-blast 0))))

(def canonical-spine-log (.getCanonicalPath (io/file spine-log)))
(def canonical-code-log (.getCanonicalPath (io/file code-cpy)))
(def env (merge spine-env
                {"NORTH_CODE_LOG" canonical-code-log
                 "NORTH_CODE_PORT" (str cport)}))
(defn concern-subjects []
  (->> (op spine spine-log
           {:op :query
            :query {:find "c"
                    :rules [{:head {:rel "c" :args [{:var "c"}]}
                             :body [{:rel "triple"
                                     :args [{:var "c"} "kind" "concern"]}]}]}})
       :ok
       (map first)
       set))

;; Fail before any spine mutation when either half of the code-store identity
;; is absent or points at a different strict corpus.
(def missing-log
  (cli-result (assoc spine-env
                     "NORTH_CODE_PORT" (str cport))
              "declare" "missing-log" "~/code/fram" "must not land" node))
(def relative-log
  (cli-result (assoc spine-env
                     "NORTH_CODE_LOG" "relative/code.log"
                     "NORTH_CODE_PORT" (str cport))
              "declare" "relative-log" "~/code/fram" "must not land" node))
(def malformed-port
  (cli-result (assoc spine-env
                     "NORTH_CODE_LOG" canonical-code-log
                     "NORTH_CODE_PORT" "not-a-port")
              "declare" "malformed-port" "~/code/fram" "must not land" node))
(def out-of-range-port
  (cli-result (assoc spine-env
                     "NORTH_CODE_LOG" canonical-code-log
                     "NORTH_CODE_PORT" "65536")
              "declare" "range-port" "~/code/fram" "must not land" node))
(def wrong-log-file (io/file tmp "wrong-code.log"))
(spit wrong-log-file "")
(def wrong-log
  (cli-result (assoc spine-env
                     "NORTH_CODE_LOG" (.getCanonicalPath wrong-log-file)
                     "NORTH_CODE_PORT" (str cport))
              "declare" "wrong-log" "~/code/fram" "must not land" node))
(check "code port without code log fails configuration before mutation"
       (and (= 2 (:exit missing-log))
            (str/includes? (:err missing-log)
                           "NORTH_CODE_PORT and NORTH_CODE_LOG must be supplied together")
            (empty? (concern-subjects))))
(check "relative code log is rejected before cwd can affect identity"
       (and (= 2 (:exit relative-log))
            (str/includes? (:err relative-log)
                           "NORTH_CODE_LOG must be an absolute path")
            (empty? (concern-subjects))))
(check "malformed and out-of-range code ports fail cleanly before mutation"
       (and (= 2 (:exit malformed-port))
            (= 2 (:exit out-of-range-port))
            (str/includes? (:err malformed-port)
                           "integer from 1 through 65535")
            (str/includes? (:err out-of-range-port)
                           "integer from 1 through 65535")
            (empty? (concern-subjects))))
(check "wrong code corpus fails its exact handshake before mutation"
       (and (= 3 (:exit wrong-log))
            (str/includes? (:err wrong-log) ":log-mismatch")
            (empty? (concern-subjects))))

(def lww-subject "@concern-code-store-lww")
(doseq [[predicate first-value second-value]
        [["code_port" "37610" "37611"]
         ["code_log" "/tmp/first-code.log" "/tmp/second-code.log"]]]
  (op spine spine-log
      {:op :assert :te lww-subject :p predicate :r first-value})
  (op spine spine-log
      {:op :assert :te lww-subject :p predicate :r second-value}))
(check "code-store identity predicates are LWW and never conflict"
       (and (= #{"37611"}
               (set (:values
                     (op spine spine-log
                         {:op :resolved :te lww-subject :p "code_port"}))))
            (= #{"/tmp/second-code.log"}
               (set (:values
                     (op spine spine-log
                         {:op :resolved :te lww-subject :p "code_log"}))))))

(def outA (cli env "declare" "alice" "~/code/fram" "rework kernel ctor" node))
(def cidA (second (re-find #"(concern-\d+-[a-f0-9]+)" outA)))
(cli env "declare" "bob" "~/code/fram" "tweak a caller" caller)
(def ov (cli spine-env "overlap" cidA))
(println outA)
(check "declare resolved a code-node footprint (not a path string)"
       (str/includes? outA "footprint(code)"))
(check "overlap surfaces the caller-coupled peer (@bob) via blast-closure"
       (and (str/includes? ov "@bob") (str/includes? ov "SHARES (blast-closure)")))
(check "concern persists the exact code corpus for cwd-independent overlap"
       (= canonical-code-log
          (:value
           (op spine spine-log
               {:op :resolved :te (str "@" cidA) :p "code_log"}))))

;; footprint lands on the CODE port ONLY — never the spine board (no port partition)
(defn footprints [port log]
  (->> (op port log {:op :query :query {:find "e" :rules
                  [{:head {:rel "e" :args [{:var "e"}]}
                    :body [{:rel "triple" :args [{:var "c"} "footprint" {:var "e"}]}]}]}})
       :ok (map first) set))
(check "footprint facts land on the CODE port"
       (contains? (footprints cport code-cpy) node))
(check "footprint NEVER lands on the spine board (port partition)"
       (empty? (footprints spine spine-log)))

;; monotone maturity — status appends `reached`, derives the max level (set-single! gone)
(cli env "status" cidA "likely-to-land")
(def lsout (cli env "ls" "~/code/fram"))
(check "status appends a monotone `reached` level (status derived = likely-to-land)"
       (str/includes? lsout "likely-to-land"))

;; a non-flipped repo (no code daemon) degrades to path-string footprint
(def outFb (cli spine-env
                "declare" "carol" "~/code/other" "non-flipped" "src/foo.clj,src/bar.clj"))
(check "no code daemon -> path-string footprint fallback + fram-code-on nudge"
       (and (str/includes? outFb "touches {") (str/includes? outFb "fram-code-on")))

(cleanup)
(if (zero? @fails)
  (do (println "\nconcern-cli code-overlap: ALL PASS") (System/exit 0))
  (do (println (str "\nconcern-cli code-overlap: " @fails " FAIL")) (System/exit 1)))
