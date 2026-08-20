#!/usr/bin/env bb
;; Finite broadcast audience contract across msg inbox, the PostToolUse peek,
;; and the live listener. Uses a throwaway coordinator: no live North state.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      "/home/tom/code/beagle/main/store"))
(def msg-cli (str root "/cli/msg-cli.clj"))
(def peek-cli (str root "/cli/inbox-peek.clj"))
(def listener-cli (str root "/cli/north-listen.clj"))
(def presence-cli (str root "/cli/presence-cli.clj"))
(def north-wrapper (str root "/bin/north"))
(def north-arm (str root "/bin/north-arm"))
(when-not (.isFile (io/file store "bin/beagle-store-server"))
  (throw (ex-info "current Beagle store engine is required" {:store store})))
(load-file (str root "/cli/coord.clj"))
(def checks (atom []))
(def children (atom []))
(def test-log (atom nil))
(def test-telemetry-log (atom nil))

(defn test-env [port]
  {"BEAGLE_STORE_LOG" @test-log
   "BEAGLE_STORE_TELEMETRY_LOG" @test-telemetry-log
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})

(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn free-port [] (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Exception _ false)))
(defn await-predicate [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 240) false
          :else (do (Thread/sleep 25) (recur (inc attempt))))))
(defn await-daemon-boot [predicate]
  (loop [attempt 0]
    (cond (predicate) true
          (>= attempt 300) false
          :else (do (Thread/sleep 250) (recur (inc attempt))))))
(defn fail-daemon-boot! [daemon]
  (try (proc/destroy-tree daemon) (catch Exception _ nil))
  (let [result (deref daemon 5000 nil)]
    (throw
     (ex-info
      "throwaway Beagle Store coordinator failed to start"
      {:exit (:exit result)
       :stdout (or (:out result) "<unavailable>")
       :stderr (or (:err result) "<unavailable>")}))))
(defn assert-fact! [port subject predicate value]
  (north.coord/append! port subject predicate value))
(defn values-of [port subject predicate]
  (set (north.coord/many port subject predicate)))
(defn subjects-with-value [port predicate value]
  (->> (north.coord/query-rows
        port
        {:find "subject"
         :rules
         [{:head {:rel "subject" :args [{:var "subject"}]}
           :body [{:rel "triple"
                   :args [{:var "subject"} predicate value]}]}]})
       (map first)
       set))
(defn run-cli [path port & args]
  (apply proc/shell {:continue true :out :string :err :string
                     :extra-env (test-env port)}
         "bb" path (str port) args))
(defn run-cli-with-env [path port extra-env & args]
  (apply proc/shell
         {:continue true :out :string :err :string
          :extra-env (merge (test-env port) extra-env)}
         "bb" path (str port) args))
(defn run-msg [port & args] (apply run-cli msg-cli port args))
(defn register! [port handle]
  (run-cli presence-cli port "register" handle (str "/tmp/" handle) handle))
(defn sent-subject [result]
  (second (re-find #"sent (@msg:[^ ]+)" (:out result))))
(defn managed-actor-key [value]
  (let [digest
        (.digest
         (java.security.MessageDigest/getInstance "SHA-256")
         (.getBytes
          (str "north-actor-key-v1\u0000managed\u0000" value)
          java.nio.charset.StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" (bit-and (int %) 0xff)) digest))))
(defn inbox-has? [port handle subject]
  (str/includes? (:out (run-msg port "inbox" handle)) subject))
(defn start-listener! [port handle log & flags]
  (let [child (apply proc/process {:out log :err log
                                   :extra-env (test-env port)}
                     "bb" listener-cli (str port) handle flags)]
    (swap! children conj child)
    child))
(defn log-has? [file text]
  (and (.exists (io/file file)) (str/includes? (slurp file) text)))
(defn mail-count [file]
  (count (re-seq #"(?m)^✉  MAIL " (if (.exists (io/file file)) (slurp file) ""))))
(defn stop-child! [child]
  (when child
    (try (proc/destroy-tree child) (catch Exception _ nil))
    (swap! children (fn [xs] (vec (remove #(identical? % child) xs))))))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-message-audience" (make-array java.nio.file.attribute.FileAttribute 0)))
      facts (io/file tmp "coordination.storelog")
      telemetry (io/file tmp "telemetry.storelog")
      daemon (do
               (proc/process
                {:dir store :out :string :err :string
                 :extra-env {"BEAGLE_STORE_SERVER_RUNTIME" "jvm-dev"
                             "BEAGLE_STORE_SERVER_QUIET" "1"
                             "BEAGLE_STORE_SERVER_XMX" "1g"}}
                (str store "/bin/beagle-store-server") "serve" (str port)
                (.getCanonicalPath facts) "north-coordination"))]
  (reset! test-log (.getCanonicalPath facts))
  (reset! test-telemetry-log (.getCanonicalPath telemetry))
  (try
    (let [started? (await-daemon-boot
                    #(try
                       (= :ready (:state (north.coord/status port)))
                       (catch Exception _ false)))]
      (check "throwaway current Beagle Store server starts" started?)
      (when-not started?
        (fail-daemon-boot! daemon)))
    (let [wrapper-probe
          (proc/shell
           {:continue true :out :string :err :string
            :extra-env {"HOME" (.getPath tmp)
                        "NORTH_BB" "/run/current-system/sw/bin/echo"
                        "NORTH_PORT" (str port)}}
           north-wrapper "listen" "wrapper-probe")]
      (check "north listen behavior honors port and acknowledges before caller flags"
             (and (zero? (:exit wrapper-probe))
                  (str/includes?
                   (:out wrapper-probe)
                   (str listener-cli " " port
                        " wrapper-probe --once --ack")))))
    (check "north-arm acknowledges before its one-shot exit"
           (str/includes? (slurp north-arm)
                          "exec \"$SCRIPT_DIR/north\" listen"))
    (doseq [handle ["sender" "alice" "bob"]]
      (check (str handle " has a live session lease")
             (zero? (:exit (register! port handle)))))

    ;; Saturate the recipient's raw `to` index with canonical-looking
    ;; first-party attention entities that sort before the real mail. A
    ;; canonical client guard cannot distinguish these rows, so the pending
    ;; relation must exclude both attention kinds before bounded pagination.
    (let [recipient "mail-isolation"
          junk-ids
          (mapv #(format "@msg:aaa-attention-junk-%03d" %) (range 300))
          message "@msg:zz-mail-isolation"
          runtime (doto (io/file tmp "mail-isolation-runtime") .mkdirs)]
      (doseq [[index junk] (map-indexed vector junk-ids)]
        (assert-fact! port junk "kind"
                      (if (even? index) "notification" "subscription"))
        (assert-fact! port junk "to" recipient))
      (doseq [[predicate value]
              [["from" "sender"]
               ["subject" "mail survives routing saturation"]
               ["body" "complete canonical envelope"]
               ["sent_at" "2026-07-30T00:00:00Z"]
               ["to" recipient]]]
        (assert-fact! port message predicate value))
      (let [manual (run-msg port "inbox" recipient)
            peek
            (run-cli-with-env
             peek-cli port
             {"XDG_RUNTIME_DIR" (.getCanonicalPath runtime)}
             recipient)]
        (check "manual inbox excludes first-party attention entities"
               (and (zero? (:exit manual))
                    (str/includes?
                     (:out manual) "mail survives routing saturation")
                    (not (str/includes? (:out manual) "attention-junk"))))
        (check "one bounded hook page reaches mail behind 300 attention rows"
               (and (zero? (:exit peek))
                    (str/includes?
                     (:out peek) "mail survives routing saturation")
                    (= #{recipient} (values-of port message "acked_by"))))
        (check "attention entities are never acknowledged as mail"
               (empty?
                (set/intersection
                 (set junk-ids)
                 (subjects-with-value port "acked_by" recipient))))))

    ;; A live listener and an inbox-only recipient are both frozen into the same
    ;; send-time snapshot. The sender is explicitly excluded.
    (let [bob-log (io/file tmp "bob-once.log")
          bob-listener (start-listener! port "bob" bob-log "--once" "--ack")]
      (check "live listener establishes scoped subscription"
             (await-predicate #(log-has? bob-log "listening")))
      (let [result (run-msg port "send" "sender" "*" "snapshot-one" "finite audience")
            message (sent-subject result)]
        (check "broadcast send succeeds" (and (zero? (:exit result)) message))
        (check "send reports finite sender-excluding snapshot"
               (str/includes? (:out result) "2 snapshotted recipients; sender excluded"))
        (check "broadcast facts name exactly the then-live peers"
               (= #{"alice" "bob"} (values-of port message "broadcast_to")))
        (check "broadcast contract is versioned"
               (= #{"snapshot-v1"} (values-of port message "broadcast_audience_version")))
        (check "sender cannot consume its own broadcast"
               (not (inbox-has? port "sender" "snapshot-one")))
        (check "inbox consumer sees an eligible unacked broadcast"
               (inbox-has? port "alice" "snapshot-one"))
        (check "live listener receives and acks exactly once"
               (await-predicate
                #(and (log-has? bob-log "snapshot-one")
                      (= #{"bob"} (values-of port message "acked_by")))))
        (check "one trigger produces one live delivery" (= 1 (mail-count bob-log)))
        (let [bob-peek (run-cli peek-cli port "bob")]
          (check "listener acknowledgement prevents PostToolUse redelivery"
                 (and (zero? (:exit bob-peek)) (str/blank? (:out bob-peek)))))
        (let [first-peek (run-cli peek-cli port "alice")
              second-peek (run-cli peek-cli port "alice")]
          (check "PostToolUse peek prints then acks eligible broadcast"
                 (and (zero? (:exit first-peek))
                      (str/includes? (:out first-peek) "snapshot-one")
                      (str/includes? (:out first-peek) "finite audience")))
          (check "PostToolUse peek does not repeat acknowledged mail"
                 (and (zero? (:exit second-peek)) (str/blank? (:out second-peek))))
          (check "ack set is bounded by the finite audience"
                 (= #{"alice" "bob"} (values-of port message "acked_by"))))

        ;; A session first appearing after the send can neither discover nor
        ;; manually acknowledge that old broadcast.
        (check "future session registers"
               (zero? (:exit (register! port "charlie"))))
        (check "future session never discovers an old broadcast"
               (not (inbox-has? port "charlie" "snapshot-one")))
        (let [rejected (run-msg port "ack" "charlie" message)]
          (check "manual ack cannot grow the audience"
                 (and (= 2 (:exit rejected))
                      (str/includes? (:out rejected) "not addressed")
                      (= #{"alice" "bob"} (values-of port message "acked_by")))))
        (stop-child! bob-listener)))

    ;; Legacy wildcard messages have no broadcast_to facts. Even a live scoped
    ;; listener receives the transport trigger but must ignore it and remain
    ;; armed until legitimately addressed.
    (let [charlie-log (io/file tmp "charlie-legacy.log")
          charlie-listener (start-listener! port "charlie" charlie-log "--once" "--ack")
          legacy "@msg:legacy-wildcard"]
      (check "legacy fixture listener establishes subscription"
             (await-predicate #(log-has? charlie-log "listening")))
      (doseq [[predicate value] [["from" "legacy-sender"]
                                 ["subject" "immortal-legacy"]
                                 ["body" "must stay inert"]
                                 ["sent_at" "2026-07-16T00:00:00Z"]]]
        (assert-fact! port legacy predicate value))
      (assert-fact! port legacy "to" "*")
      (Thread/sleep 150)
      (check "audience-less legacy wildcard is not delivered or acked"
             (and (not (log-has? charlie-log "immortal-legacy"))
                  (empty? (values-of port legacy "acked_by"))
                  (not (inbox-has? port "charlie" "immortal-legacy"))))
      (let [direct (run-msg port "send" "sender" "charlie" "direct-after-legacy" "wake")]
        (check "ignored legacy event does not disarm --once listener"
               (and (zero? (:exit direct))
                    (await-predicate #(log-has? charlie-log "direct-after-legacy"))
                    (= 1 (mail-count charlie-log)))))
      (stop-child! charlie-listener))

    ;; The live listener and several PostToolUse peek processes race on the same
    ;; direct message immediately after each `to` commit. The coordinator claim
    ;; must elect one printer; acked_by alone would leave a query-then-ack hole.
    (check "racer has a live session lease"
           (zero? (:exit (register! port "racer"))))
    (let [racer-log (io/file tmp "racer-simultaneous.log")
          racer-listener (start-listener! port "racer" racer-log "--ack")]
      (check "simultaneous-delivery listener is armed"
             (await-predicate #(log-has? racer-log "listening")))
      (let [rounds
            (mapv
             (fn [i]
               (let [token (str "simultaneous-" i "-" (java.util.UUID/randomUUID))
                     send-result (run-msg port "send" "sender" "racer" token "simultaneous body")
                     message (sent-subject send-result)
                     peeks (mapv (fn [_]
                                   (proc/process {:out :string :err :string
                                                  :extra-env (test-env port)}
                                                 "bb" peek-cli (str port) "racer"))
                                 (range 4))
                     peek-results (mapv deref peeks)]
                 {:token token :message message
                  :send-result send-result :peek-results peek-results}))
             (range 24))]
        (check "simultaneous send and peek processes all exit cleanly"
               (every? (fn [{:keys [send-result message peek-results]}]
                         (and message
                              (zero? (:exit send-result))
                              (every? #(zero? (:exit %)) peek-results)))
                       rounds))
        (check "every raced message reaches one durable acknowledgement"
               (await-predicate
                #(every? (fn [{:keys [message]}]
                           (= #{"racer"} (values-of port message "acked_by")))
                         rounds)))
        (let [listener-output (slurp racer-log)]
          (check "listener-vs-PostToolUse races print every message exactly once"
                 (every?
                  (fn [{:keys [token peek-results]}]
                    (= 1
                       (+ (count (re-seq (re-pattern (java.util.regex.Pattern/quote token))
                                         listener-output))
                          (reduce +
                                  (map #(count
                                         (re-seq
                                          (re-pattern (java.util.regex.Pattern/quote token))
                                          (:out %)))
                                       peek-results)))))
                  rounds))))
      (stop-child! racer-listener))
    (check "racer lease is retired before broadcast snapshot stress"
           (zero? (:exit (run-cli presence-cli port "forget" "racer"))))

    ;; The hook intentionally leaves messages whose complete rendering exceeds
    ;; its 24 KiB output budget unacknowledged. Its persisted Beagle Store cursor must
    ;; still make deterministic progress past more than one full candidate page,
    ;; or a large prefix permanently starves bounded mail behind it.
    (let [runtime (io/file tmp "peek-cursor-runtime")
          recipient "cursor-recipient"
          large-ids (mapv #(str "@msg:peek-large-" %) (range 1 5))
          tail-id "@msg:peek-small-tail"
          large-body (apply str (repeat (* 25 1024) "x"))]
      (.mkdirs runtime)
      (doseq [message (conj large-ids tail-id)]
        (doseq [[predicate value]
                [["from" "cursor-sender"]
                 ["subject" (if (= message tail-id)
                              "bounded tail"
                              "hook-oversized")]
                 ["body" (if (= message tail-id)
                           "tail survives a large prefix"
                           large-body)]
                 ["sent_at" "2026-07-19T00:00:00Z"]
                 ["to" recipient]]]
          (assert-fact! port message predicate value)))
      (let [env {"XDG_RUNTIME_DIR" (.getCanonicalPath runtime)}
            first-peek (run-cli-with-env peek-cli port env recipient)
            state-file
            (io/file runtime "north-inbox-peek"
                     (managed-actor-key recipient))
            persisted (when (.isFile state-file)
                        (edn/read-string (slurp state-file)))
            second-peek (run-cli-with-env peek-cli port env recipient)]
        (check "first bounded peek skips one full oversized page without output"
               (and (zero? (:exit first-peek))
                    (str/blank? (:out first-peek))))
        (check "peek persists a strict engine-issued page without inventing a cursor"
               (and (map? persisted)
                    (= "north-inbox-spool-v1" (:schema persisted))
                    (= (managed-actor-key recipient) (:actor-key persisted))
                    (= ["@msg:peek-large-4" tail-id] (:ids persisted))
                    ;; This five-row relation ended in the first engine page.
                    ;; Nil is therefore the exact engine continuation, not a
                    ;; cursor derived from the last cached ID.
                    (nil? (:next persisted))))
        (check "a later bounded peek reaches and emits the small tail"
               (and (zero? (:exit second-peek))
                    (str/includes? (:out second-peek) "bounded tail")
                    (str/includes? (:out second-peek)
                                   "tail survives a large prefix")))
        (check "the emitted tail alone is durably acknowledged"
               (= #{recipient} (values-of port tail-id "acked_by")))
        (check "every hook-oversized prefix message remains unacknowledged"
               (every? #(empty? (values-of port % "acked_by")) large-ids))))

    ;; Concurrent producers publish multiple complete snapshots while three
    ;; scoped listeners are armed. Every eligible listener acks each message
    ;; once; no sender/future identity can enlarge any ack set.
    (let [handles ["alice" "bob" "charlie"]
          listener-pairs
          (mapv (fn [handle]
                  (let [log (io/file tmp (str handle "-burst.log"))]
                    [handle log (start-listener! port handle log "--ack")]))
                handles)]
      (doseq [[handle log _] listener-pairs]
        (check (str handle " burst listener is armed")
               (await-predicate #(log-has? log "listening"))))
      (let [producers
            (mapv (fn [i]
                    (proc/process {:out :string :err :string
                                   :extra-env (test-env port)}
                                  "bb" msg-cli (str port) "send" "sender" "*"
                                  (str "burst-" i) (str "body-" i)))
                  (range 8))
            results (mapv deref producers)
            messages (mapv sent-subject results)]
        (check "all concurrent producers complete with unique message ids"
               (and (every? #(zero? (:exit %)) results)
                    (every? some? messages)
                    (= 8 (count (set messages)))))
        (check "every concurrent snapshot has the same finite audience"
               (every? #(= (set handles) (values-of port % "broadcast_to")) messages))
        (check "all eligible live listeners ack every concurrent broadcast"
               (await-predicate
                #(every? (fn [message]
                           (= (set handles) (values-of port message "acked_by")))
                         messages)))
        (doseq [[handle log _] listener-pairs]
          (check (str handle " receives every burst message exactly once")
                 (= 8 (mail-count log))))
        (let [before (into {} (map (fn [message]
                                     [message (values-of port message "acked_by")])
                                   messages))]
          (doseq [handle handles]
            (run-cli peek-cli port handle))
          (check "consumer replays cannot enlarge settled broadcast ack sets"
                 (= before
                    (into {} (map (fn [message]
                                    [message (values-of port message "acked_by")])
                                  messages))))))
      (doseq [[_ _ listener] listener-pairs] (stop-child! listener)))

    (finally
      (doseq [child @children] (stop-child! child))
      (proc/destroy-tree daemon)
      (doseq [file (reverse (file-seq tmp))]
        (io/delete-file file true)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nmessage audience integration: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
