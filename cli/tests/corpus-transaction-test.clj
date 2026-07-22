#!/usr/bin/env bb
(require '[babashka.classpath :as cp]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram-out
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_OUT")
                (str root "/../fram/out")))))
(cp/add-classpath fram-out)
(load-file (str root "/cli/corpus-transaction.clj"))
(require '[north.corpus-transaction :as ct])

(def failures (atom []))
(defn check [label value]
  (println (str (if value "  PASS  " "  FAIL  ") label))
  (when-not value (swap! failures conj label)))
(defn capture [f]
  (try {:value (f)}
       (catch Throwable error {:error error})))
(defn throws? [f] (boolean (:error (capture f))))
(defn write! [path value]
  (spit path value)
  (.getCanonicalPath (io/file path)))
(defn append! [path value] (spit path value :append true))
(defn sha [path] (ct/sha256-file path))
(defn slurp* [path] (slurp (io/file path)))
(defn delete-tree! [root]
  (doseq [f (reverse (file-seq root))]
    (java.nio.file.Files/deleteIfExists (.toPath ^java.io.File f))))

(defn make-scenario
  ([] (make-scenario {}))
  ([options]
   (let [tmp (.toFile
              (java.nio.file.Files/createTempDirectory
               "north-corpus-transaction-test-"
               (make-array java.nio.file.attribute.FileAttribute 0)))
         live-dir (doto (io/file tmp "live") .mkdirs)
         candidate-dir (doto (io/file tmp "candidate") .mkdirs)
         state-dir (doto (io/file tmp "state") .mkdirs)
         coord (write! (io/file live-dir "coordination.log") "coord-old\n")
         telem (write! (io/file live-dir "telemetry.log") "telem-old\n")
         candidate-coord
         (write! (io/file candidate-dir "coordination.log") "coord-new\n")
         candidate-telem
         (write! (io/file candidate-dir "telemetry.log") "telem-new\n")]
     {:tmp tmp
      :state-dir state-dir
      :coord coord
      :telem telem
      :candidate-coord candidate-coord
      :candidate-telem candidate-telem
      :expected-live {:coordination coord :telemetry telem}
      :events (atom [])
      :current-lease (atom nil)
      :successor-epoch (atom (:successor-epoch options))
      :options options})))

(defn with-scenario [options f]
  (let [scenario (make-scenario options)]
    (try
      (with-redefs [ct/state-root
                    (fn [] (.getCanonicalPath ^java.io.File (:state-dir scenario)))]
        (f scenario))
      (finally (delete-tree! (:tmp scenario))))))

(defn fresh-plan [scenario]
  (ct/make-plan
   {:purpose "owner-state-machine-test"
    :live (:expected-live scenario)
    :candidate {:coordination (:candidate-coord scenario)
                :telemetry (:candidate-telem scenario)}
    :target {:corpus-max-tx (get-in scenario [:options :target-max] 2)}
    :runtime {:controller "test"}}))

(defn callbacks [scenario]
  (let [{:keys [coord telem candidate-coord candidate-telem events
                current-lease successor-epoch options]} scenario
        epoch (get options :epoch 7)
        resource "north-corpus-maintenance"
        holder (str "owner-test-" epoch)]
    {:expected-live (:expected-live scenario)
     :acquire-lease!
     (fn []
       (let [lease {:ok true :resource resource :holder holder :epoch epoch}]
         (append! coord (str "lease-acquire-" epoch "\n"))
         (reset! current-lease epoch)
         (swap! events conj [:lease-acquire lease])
         lease))
     :release-lease!
     (fn [lease]
       (swap! events conj [:lease-release lease])
       (when (= (:epoch lease) @current-lease)
         (append! coord (str "lease-release-" (:epoch lease) "\n"))
         (reset! current-lease nil))
       {:ok true :state :released})
     :checkpoint-source!
     (fn [_ lease]
       (swap! events conj [:checkpoint (:epoch lease)])
       {:version (get options :checkpoint-version 2)
        :live {:coordination (ct/corpus-file-record "checkpoint coordination" coord)
               :telemetry (ct/corpus-file-record "checkpoint telemetry" telem)}
        ;; Returning these seals explicitly is the candidate-finalization
        ;; contract. The core never trusts the pre-lease plan record here.
        :candidate
        {:coordination (ct/corpus-file-record "final coordination" candidate-coord)
         :telemetry (ct/corpus-file-record "final telemetry" candidate-telem)}
        :target {:corpus-max-tx (get options :target-max 2)}})
     :stop!
     (fn []
       (swap! events conj :stop)
       (when-let [value (:append-during-stop options)]
         ;; This models a write acknowledged after the online checkpoint but
         ;; before the supervisor proves the coordinator offline.
         (append! coord value))
       (when (:stop-fails options)
         (throw (ex-info "stop failed" {}))))
     :start!
     (fn []
       (swap! events conj :start)
       (when (:start-fails options)
         (throw (ex-info "start failed" {})))
       (when-let [value (:append-after-start options)]
         (append! coord value)))
     :assert-offline! #(swap! events conj :offline)
     :verify-restart!
     (fn [journal]
       (swap! events conj [:verified (:data-result journal)])
       (or (:verify-result options)
           {:ok true
            :controller "owner-test"
            :coordination-log coord
            :telemetry-log telem
            :version (get options :target-max 2)}))
     :settle-lease!
     (fn [lease journal]
       (swap! events conj [:lease-settle lease (:data-result journal)])
       (cond
         ;; A successor acquired before ExecStartPost must never be cleared by
         ;; settlement of the journal's older exact epoch.
         @successor-epoch
         {:ok true :state :stale-epoch-noop :successor @successor-epoch}

         (and (= "rolled-back" (:data-result journal))
              (= (:epoch lease) @current-lease))
         (do (append! coord (str "lease-release-" (:epoch lease) "\n"))
             (reset! current-lease nil)
             {:ok true :state :released :epoch (:epoch lease)})

         :else
         (do (reset! current-lease nil)
             {:ok true :state :absent-noop :epoch (:epoch lease)})))}))

(defn crash-at! [boundary f]
  (with-redefs [ct/maybe-inject-crash!
                (fn [phase]
                  (when (= phase boundary)
                    (throw (ex-info
                            (str "simulated crash at " phase)
                            {:north.corpus-transaction/simulated-crash true
                             :phase phase}))))]
    (capture f)))

(println "## sealed plan and checkpoint watermark")
(with-scenario
 {}
 (fn [scenario]
   (let [plan (fresh-plan scenario)]
     (check "plan id is a content seal"
            (boolean (re-matches ct/plan-id-pattern (:plan-id plan))))
     (check "sealed plan verifies"
            (= (:plan-id plan)
               (:plan-id (ct/verify-plan! plan (:expected-live scenario)))))
     (check "plan metadata tamper is refused"
            (throws? #(ct/verify-plan! (assoc plan :purpose "tampered")
                                      (:expected-live scenario)))))))
(with-scenario
 {:checkpoint-version 9 :target-max 8}
 (fn [scenario]
   (let [result (capture #(ct/apply-plan! (fresh-plan scenario)
                                          (callbacks scenario)))]
     (check "candidate watermark below the post-lease checkpoint is refused"
            (some? (:error result)))
     (check "watermark refusal happens before supervisor stop"
            (not (some #{:stop} @(:events scenario))))
     (check "pre-stop watermark refusal releases the exact acquired lease"
            (some #(and (vector? %) (= :lease-release (first %)))
                  @(:events scenario)))
     (check "watermark refusal leaves no active journal"
            (not (.exists (io/file (ct/journal-path))))))))

(println "## all-new commit and online suffix tolerance")
(with-scenario
 {:append-after-start "acknowledged-online-suffix\n"}
 (fn [scenario]
   (let [result (ct/apply-plan! (fresh-plan scenario) (callbacks scenario))]
     (check "apply reports all-new success" (:ok result))
     (check "both candidate corpora were installed"
            (and (str/starts-with? (slurp* (:coord scenario)) "coord-new\n")
                 (= "telem-new\n" (slurp* (:telem scenario)))))
     (check "legitimate post-start append survives prefix settlement"
            (str/includes? (slurp* (:coord scenario))
                           "acknowledged-online-suffix\n"))
     (check "lease identity is durable and settled by exact epoch"
            (some (fn [event]
                    (and (= :lease-settle (first event))
                         (= "north-corpus-maintenance"
                            (get-in event [1 :resource]))
                         (= 7 (get-in event [1 :epoch]))))
                  (filter vector? @(:events scenario))))
     (check "receipt is immutable and content addressed"
            (let [path (get-in result [:receipt :path])]
              (and (.isFile (io/file path))
                   (re-find #"receipt-[0-9a-f]{64}\.edn$" path))))
     (check "receipt carries exact runtime, target, corpus, and lease evidence"
            (let [receipt (ct/read-edn-file!
                           "terminal receipt" (get-in result [:receipt :path]))]
              (and (= true (get-in receipt [:runtime-verification :ok]))
                   (= "test" (get-in receipt [:runtime :controller]))
                   (= 2 (get-in receipt [:target :corpus-max-tx]))
                   (= true (get-in receipt [:pre-settlement-corpus :prefix-match]))
                   (= true (get-in receipt [:post-settlement-corpus :prefix-match]))
                   (= true (get-in receipt [:lease-settlement :ok]))
                   (nil? (:completed-at receipt)))))
     (check "journal clears only after settlement receipt"
            (not (.exists (io/file (ct/journal-path))))))))

(println "## acknowledged write between checkpoint and stop is never lost")
(with-scenario
 {:append-during-stop "acknowledged-racing-write\n"}
 (fn [scenario]
   (let [result (ct/apply-plan! (fresh-plan scenario) (callbacks scenario))
         live (slurp* (:coord scenario))]
     (check "post-checkpoint drift aborts candidate installation"
            (and (false? (:ok result)) (= :source-drift (:aborted result))))
     (check "acknowledged append survives rollback byte-for-byte"
            (str/includes? live "acknowledged-racing-write\n"))
     (check "rollback does not truncate to original-live provenance"
            (and (str/includes? live "lease-acquire-7\n")
                 (str/includes? live "lease-release-7\n")))
     (check "drift path restarts and settles instead of serving candidates"
            (and (some #{:start} @(:events scenario))
                 (not (str/starts-with? live "coord-new\n")))))))

(println "## every pre/post-stop durable crash boundary")
(doseq [boundary ["pre-stop-journal"
                  "stopped"
                  "preimage-prepared"
                  "stages-prepared"
                  "prepared"
                  "journal"
                  "coordination-renamed"
                  "coordination-journaled"
                  "telemetry-renamed"
                  "telemetry-journaled"]]
  (with-scenario
   (if (= boundary "stopped")
     {:append-during-stop "acknowledged-before-stopped-boundary\n"}
     {})
   (fn [scenario]
     (let [cb (callbacks scenario)
           crashed (crash-at!
                    boundary
                    #(ct/apply-plan! (fresh-plan scenario) cb))]
       (check (str boundary " interruption is injected")
              (and (:error crashed)
                   (:north.corpus-transaction/simulated-crash
                    (ex-data (:error crashed)))))
       (check (str boundary " retains durable intent and exact lease")
              (let [journal (ct/read-edn-file!
                             "crash journal" (ct/journal-path))]
                (and (= 7 (get-in journal [:lease :epoch]))
                     (= "north-corpus-maintenance"
                        (get-in journal [:lease :resource])))))
       ;; The pre-stop boundary dies before invoking the supervisor. Boot recovery
       ;; only runs once the service is offline, so the owner test performs that
       ;; exact external transition before calling recover.
       (when (= boundary "pre-stop-journal") ((:stop! cb)))
       ;; Recovery authority is the journal + immutable objects, never the
       ;; builder's original candidate paths.
       (java.nio.file.Files/deleteIfExists
        (.toPath (io/file (:candidate-coord scenario))))
       (java.nio.file.Files/deleteIfExists
        (.toPath (io/file (:candidate-telem scenario))))
       (let [recovered (ct/recover-active!
                        {:expected-live (:expected-live scenario)
                         :assert-offline! (:assert-offline! cb)})
             expect-new? (#{"telemetry-renamed" "telemetry-journaled"}
                          boundary)]
         (check (str boundary " recovery chooses one terminal corpus")
                (= (if expect-new? "committed" "rolled-back")
                   (:data-result recovered)))
         (check (str boundary " never leaves a mixed pair")
                (if expect-new?
                  (and (= "coord-new\n" (slurp* (:coord scenario)))
                       (= "telem-new\n" (slurp* (:telem scenario))))
                  (and (str/starts-with? (slurp* (:coord scenario)) "coord-old\n")
                       (= "telem-old\n" (slurp* (:telem scenario))))))
         (when (= boundary "stopped")
           (check "crash immediately after stop preserves an acknowledged append"
                  (str/includes? (slurp* (:coord scenario))
                                 "acknowledged-before-stopped-boundary\n")))
         ((:start! cb))
         (ct/settle-active! cb)
         (check (str boundary " settles exact lease then clears journal")
                (not (.exists (io/file (ct/journal-path))))))))))

(println "## post-restart settlement crash boundaries")
(doseq [boundary ["settlement-verified" "lease-settled" "receipt-published"
                  "settlement-receipt" "settlement-cleared"]]
  (with-scenario
   {}
   (fn [scenario]
     (let [cb (callbacks scenario)
           crashed (crash-at!
                    boundary
                    #(ct/apply-plan! (fresh-plan scenario) cb))
           receipts-before (set (map #(.getCanonicalPath ^java.io.File %)
                                     (filter #(.isFile ^java.io.File %)
                                             (file-seq (io/file (ct/state-root)
                                                                "receipts")))))
           settlements-before
           (count (filter #(and (vector? %) (= :lease-settle (first %)))
                          @(:events scenario)))]
       (check (str boundary " interruption is injected")
              (some? (:error crashed)))
       (let [settled (ct/settle-active! cb)
             settlements-after
             (count (filter #(and (vector? %) (= :lease-settle (first %)))
                            @(:events scenario)))]
         (check (str boundary " retry reaches a clean terminal state")
                (and settled (not (.exists (io/file (ct/journal-path))))))
         (when (= boundary "settlement-receipt")
           (check "recorded receipt prevents duplicate lease settlement"
                  (= settlements-before settlements-after)))
         (when (= boundary "receipt-published")
           (let [receipts-after
                 (set (map #(.getCanonicalPath ^java.io.File %)
                           (filter #(.isFile ^java.io.File %)
                                   (file-seq (io/file (ct/state-root)
                                                      "receipts")))))]
             (check "receipt publication retry is content-idempotent"
                    (= receipts-before receipts-after)))))))))

(println "## runtime-unverified is retry-settleable without a double start")
(with-scenario
 {:start-fails true}
 (fn [scenario]
   (let [cb (callbacks scenario)
         failure (capture #(ct/apply-plan! (fresh-plan scenario) cb))]
     (check "restart failure is classified after durable data resolution"
            (and (:error failure)
                 (:north.corpus-transaction/data-resolved
                  (ex-data (:error failure)))
                 (false? (:north.corpus-transaction/runtime-verified
                          (ex-data (:error failure))))))
     (check "failed restart is attempted exactly once"
            (= 1 (count (filter #{:start} @(:events scenario)))))
     (check "runtime-unverified journal remains durable"
            (= :runtime-unverified
               (:phase (ct/read-edn-file!
                        "runtime-unverified journal" (ct/journal-path)))))
     (let [healthy (callbacks (assoc scenario :options {}))]
       (ct/settle-active! healthy)
       (check "later authoritative restart can settle pending exact epoch"
              (not (.exists (io/file (ct/journal-path)))))
       (check "settlement retry does not invoke start a second time"
              (= 1 (count (filter #{:start} @(:events scenario)))))))))

(println "## non-positive runtime verdict never clears authority")
(with-scenario
 {:verify-result {:ok false :reason :wrong-runtime}}
 (fn [scenario]
   (let [cb (callbacks scenario)
         failure (capture #(ct/apply-plan! (fresh-plan scenario) cb))]
     (check "explicit false restart verdict fails settlement"
            (and (:error failure)
                 (= {:ok false :reason :wrong-runtime}
                    (get-in (ct/read-edn-file!
                             "false-verdict journal" (ct/journal-path))
                            [:runtime-verification]))))
     (check "false restart verdict leaves retryable journal authority"
            (= :runtime-unverified
               (:phase (ct/read-edn-file!
                        "false-verdict journal" (ct/journal-path)))))
     (check "repeating a false verdict cannot clear the journal"
            (and (throws? #(ct/settle-active! cb))
                 (.isFile (io/file (ct/journal-path)))))
     (let [healthy (callbacks (assoc scenario :options {}))]
       (ct/settle-active! healthy)
       (check "later positive verdict settles the same journal"
              (not (.exists (io/file (ct/journal-path)))))))))

(println "## stale epoch, artifact validation, and contention")
(with-scenario
 {:successor-epoch 99}
 (fn [scenario]
   (let [result (ct/apply-plan! (fresh-plan scenario) (callbacks scenario))]
     (check "stale journal epoch settlement does not disturb successor"
            (and (:ok result) (= 99 @(:successor-epoch scenario)))))))
(with-scenario
 {}
 (fn [scenario]
   (let [cb (callbacks scenario)
         _ (crash-at! "prepared"
                      #(ct/apply-plan! (fresh-plan scenario) cb))
         journal (ct/read-edn-file! "prepared journal" (ct/journal-path))
         manifest (get-in journal [:preimage-manifest :path])]
     (append! manifest "tamper\n")
     (check "tampered immutable preimage manifest is refused during recovery"
            (throws? #(ct/recover-active!
                       {:expected-live (:expected-live scenario)
                        :assert-offline! (:assert-offline! cb)}))))))
(with-scenario
 {}
 (fn [scenario]
   (let [held (ct/acquire-maintenance-lock!)]
     (try
       (check "state singleton refuses a concurrent maintenance transaction"
              (throws? #(ct/acquire-maintenance-lock!)))
       (finally (ct/close-lock! held))))))
(with-scenario
 {}
 (fn [scenario]
   (let [plan (fresh-plan scenario)]
     (append! (:candidate-telem scenario) "tamper\n")
     (check "candidate tamper is refused before lease acquisition"
            (throws? #(ct/verify-plan! plan (:expected-live scenario)))))))

(println "## append-boundary invariant")
(doseq [[label target]
        [["unterminated live coordination" :coord]
         ["unterminated live telemetry" :telem]
         ["unterminated candidate coordination" :candidate-coord]
         ["unterminated candidate telemetry" :candidate-telem]]]
  (with-scenario
   {}
   (fn [scenario]
     (write! (get scenario target) "unterminated")
     (let [before (into {}
                        (map (fn [key] [key (slurp* (get scenario key))])
                             [:coord :telem :candidate-coord :candidate-telem]))
           refused (capture #(fresh-plan scenario))
           after (into {}
                       (map (fn [key] [key (slurp* (get scenario key))])
                            [:coord :telem :candidate-coord :candidate-telem]))]
       (check (str label " is refused at transaction seal")
              (some? (:error refused)))
       (check (str label " refusal mutates no corpus bytes") (= before after))
       (check (str label " refusal creates no journal")
              (not (.exists (io/file (ct/journal-path)))))))))
(with-scenario
 {}
 (fn [scenario]
   (write! (:telem scenario) "")
   (write! (:candidate-telem scenario) "")
   (let [result (ct/apply-plan! (fresh-plan scenario) (callbacks scenario))]
     (check "zero-byte telemetry is a valid append boundary" (:ok result))
     (check "zero-byte telemetry remains zero-byte after commit"
            (zero? (.length (io/file (:telem scenario))))))))

(if (seq @failures)
  (do (println (str "FAILURES: " (str/join ", " @failures)))
      (System/exit 1))
  (println "corpus-transaction owner state-machine tests: PASS"))
