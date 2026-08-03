#!/usr/bin/env bb
;; bars-cli.clj — coordinator-facing grooming for one thread's done_when set.
;;
;; Delivery evidence reserves against the bars a thread carries AT DISPATCH, and
;; the reservation manifest bounds that set (32 bars, 512 UTF-8 bytes each). A
;; long-lived thread accumulates bars across runs, so the bound is reached by
;; ACCUMULATION, not by any single bad write — and until now the only recovery
;; was hand-retracting bars one by one after reading an error that named none of
;; them. This surface is the one-step recovery the reserve error points at.
;;
;; What it never does: judge whether a bar was MET. Retiring a bar is grooming;
;; evidence is `north evidence record`. `prune` therefore retires only bars that
;; already carry canonical `bar_evidence`, or bars the caller names verbatim.
;; Unreserved observations (`bar_evidence_unreserved`) are displayed but never
;; make a bar prunable — that would silently promote them to verification.
;;
;;   bb bars-cli.clj list  <thread>
;;   bb bars-cli.clj prune <thread> [--dry-run] [--bar "<exact bar>"]...
;;   bb bars-cli.clj check <thread> <candidate bar>      (advisory; always exit 0)
;;   bb bars-cli.clj echo  <thread>                      (outcome friction echo)
(ns north.bars-cli
  (:require [clojure.java.io :as io]
            [clojure.string :as str]))

(load-file (str (.getParent (io/file *file*)) "/terminal-projection.clj"))

;; framrpc-client.clj is loaded lazily so a stale classpath fails at connect!
;; (clean refusal), never at namespace load.
(defn- ensure-native-client! []
  (when-not (find-ns 'north.framrpc-client)
    (load-file (str (.getParent (io/file *file*)) "/framrpc-client.clj"))))

(def ^:private usage
  (str "usage:\n"
       "  north bars list  <thread>\n"
       "  north bars prune <thread> [--dry-run] [--bar \"<exact bar>\"]...\n"
       "  north bars check <thread> <candidate bar>\n"
       "  north bars echo  <thread>\n"))

(defn- die! [message]
  (binding [*out* *err*] (println (str "north bars: " message)))
  (System/exit 1))

;; SpaceId is the whole fence: connect validates the served space before any
;; read, so a coordinator serving another corpus is refused rather than answered.
;; The v0.3 :for-log envelope has no native analogue and needs none.
(defn- connect! []
  (let [host (or (not-empty (System/getenv "NORTH_FRAMRPC_HOST")) "127.0.0.1")
        port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977"))
        space (or (not-empty (System/getenv "FRAM_SPACE_ID")) "north-coordination")]
    (try
      (ensure-native-client!)
      ((ns-resolve 'north.framrpc-client 'connect)
       host port space {:connect-timeout-ms 2000 :read-timeout-ms 30000})
      (catch Exception error
        (die! (str "coordinator at " host ":" port " did not answer for space "
                   space " (" (.getMessage error) ")"))))))

(defn- rpc-close! [client]
  ((ns-resolve 'north.framrpc-client 'close!) client))

(defn- rpc-scan-all! [client subject predicate object]
  ((ns-resolve 'north.framrpc-client 'scan-all!) client subject predicate object))

(defn- rpc-retract-projected! [client triple]
  ((ns-resolve 'north.framrpc-client 'retract-projected!) client triple))

;; fram.types is only resolved once framrpc-client.clj (which requires it)
;; has loaded, i.e. after a successful ensure-native-client! — never at
;; bars-cli's own namespace analysis.
(defn- triple-slot1 [triple] ((ns-resolve 'fram.types 'triple-slot1) triple))
(defn- triple-slot2 [triple] ((ns-resolve 'fram.types 'triple-slot2) triple))
(defn- new-triple [subject predicate object]
  ((ns-resolve 'fram.types 'triple) subject predicate object))

(defn- thread-entity [raw]
  (let [value (str raw)
        canonical (if (str/starts-with? value "@") value (str "@" value))]
    (when-not (north.terminal-projection/valid-thread-entity? canonical)
      (die! (str "invalid thread id " (pr-str raw))))
    canonical))

(defn- facts-of [client subject]
  ;; Exact-subject bar grooming stays on the indexed scan pattern. A Datalog
  ;; query for this already-ground shape paid whole-query planning on every
  ;; outcome write as the corpus grew.
  (let [rows
        (try
          (:rows (rpc-scan-all! client subject nil nil))
          (catch Exception error
            (die! (str "coordinator did not answer a read for " subject
                       " (" (.getMessage error) ")"))))]
    (reduce (fn [acc triple]
              (let [predicate (triple-slot1 triple)
                    value (triple-slot2 triple)]
                ;; The v0.3 :show envelope was string-typed; a non-string Term
                ;; carries no groomable bar fact.
                (if (and (string? predicate) (string? value))
                  (update acc predicate (fnil conj #{}) value)
                  acc)))
            {}
            rows)))

(defn- thread-facts! [client thread]
  (let [facts (facts-of client thread)
        titles (get facts "title" #{})]
    (when-not (and (= 1 (count titles))
                   (string? (first titles))
                   (not (str/blank? (first titles))))
      (die! (str thread " is not a title-bearing thread")))
    facts))

(defn- evidenced?
  "Same containment rule north's own ✓/○ done-bars echo uses (stale/bar-mark),
   so `list` and `north tell <id> outcome` never disagree about a bar."
  [evidence bar]
  (boolean (some #(and (string? %) (str/includes? % bar)) evidence)))

(defn- bar-rows [facts]
  (let [values (north.terminal-projection/done-bar-values facts)
        evidence (get facts "bar_evidence" #{})
        unreserved (get facts "bar_evidence_unreserved" #{})]
    (mapv (fn [bar]
            {:bar bar
             :evidenced (evidenced? evidence bar)
             :unreserved (evidenced? unreserved bar)
             :bytes (or (north.terminal-projection/utf8-byte-count bar) 0)})
          (sort values))))

(defn- over-limit? [rows]
  (or (> (count rows) north.terminal-projection/max-delivery-bars)
      (some #(> (:bytes %) north.terminal-projection/max-delivery-bar-utf8-bytes)
            rows)))

(defn- mark [row]
  (cond (:evidenced row) "✓"
        (:unreserved row) "~"
        :else "○"))

(defn cmd-echo
  "The outcome-write friction gradient. It is deliberately silent for a
   bar-less subject and advisory only; bin/north ignores read failures so this
   display can never block a human's outcome write."
  [thread]
  (let [entity (thread-entity thread)
        client (connect!)
        rows (try (bar-rows (facts-of client entity))
                  (finally (rpc-close! client)))]
    (when (seq rows)
      (println
       (str "DONE BARS on " entity
            " — this outcome claims they are met; cite probe + observed result:"))
      (doseq [row rows]
        ;; Match the historical cmd-done-bars contract: unreserved observations
        ;; are not canonical bar evidence at outcome judgment time.
        (println (str "  " (if (:evidenced row) "✓" "○") " " (:bar row))))
      (println
       (str "  evidence: north tell " (subs entity 1)
            " bar_evidence \"<bar> → <observed result>\"")))))

(defn- print-summary [thread rows]
  (let [limit north.terminal-projection/max-delivery-bars
        oversized (filterv #(> (:bytes %)
                               north.terminal-projection/max-delivery-bar-utf8-bytes)
                           rows)
        prunable (count (filterv :evidenced rows))]
    (println (format "%d bar(s) · limit %d · %d evidenced"
                     (count rows) limit prunable))
    (when (seq oversized)
      (println (format "  %d bar(s) exceed the %d-byte per-bar reserve limit"
                       (count oversized)
                       north.terminal-projection/max-delivery-bar-utf8-bytes)))
    (if (over-limit? rows)
      (println (str "  reserve BLOCKED — managed runs on this thread cannot"
                    " reserve delivery evidence."
                    (if (pos? prunable)
                      (str " Retire the evidenced bars in one step: north bars"
                           " prune " (subs thread 1))
                      (str " No bar carries evidence; retire named bars with:"
                           " north bars prune " (subs thread 1)
                           " --bar \"<exact bar>\""))))
      (println "  reserve OK"))))

(defn cmd-list [thread]
  (let [entity (thread-entity thread)
        client (connect!)
        rows (try (bar-rows (thread-facts! client entity))
                  (finally (rpc-close! client)))]
    (println (str "DONE BARS on " entity
                  " — ✓ evidenced · ~ unreserved observation only · ○ open"))
    (doseq [row rows]
      (println (format "  %s [%4d B] %s" (mark row) (:bytes row) (:bar row))))
    (print-summary entity rows)))

(defn cmd-prune [thread options]
  (let [entity (thread-entity thread)
        client (connect!)
        facts (thread-facts! client entity)
        rows (bar-rows facts)
        named (:bars options)
        stored (set (map :bar rows))
        unknown (remove stored named)
        targets (if (seq named)
                  (filterv #(contains? (set named) (:bar %)) rows)
                  (filterv :evidenced rows))]
    (when (seq unknown)
      (die! (str "no such done_when on " entity ": "
                 (north.terminal-projection/done-bar-diagnostic unknown))))
    (when (empty? targets)
      (println (str "nothing to retire on " entity
                    " — no bar carries canonical bar_evidence."
                    " Name the stale ones: north bars prune " (subs entity 1)
                    " --bar \"<exact bar>\""))
      (System/exit 0))
    (doseq [row targets]
      (println (str (if (:dry-run options) "would retire " "retired ")
                    (mark row) " " (:bar row)))
      (when-not (:dry-run options)
        ;; Retiring a bar means it LEAVES the projection; the raw wire retract
        ;; would withdraw one occurrence and leave an accumulated duplicate live.
        (try
          (rpc-retract-projected!
           client (new-triple entity "done_when" (:bar row)))
          (catch Exception error
            (die! (str "coordinator rejected retracting " (pr-str (:bar row))
                       " (" (pr-str (or (:type (ex-data error))
                                        (.getMessage error))) ")"))))))
    (let [remaining (if (:dry-run options)
                      rows
                      (bar-rows (facts-of client entity)))]
      (rpc-close! client)
      (println (format "%s %d bar(s) on %s"
                       (if (:dry-run options) "would retire" "retired")
                       (count targets) entity))
      (print-summary entity remaining))))

(defn cmd-check
  "Advisory pre-write warning for `north tell <thread> done_when <bar>`. Never
   blocks and never fails: a thread may legitimately carry bars a managed run
   will not reserve against, but the author should learn it at write time rather
   than when a lane's reservation fails hours later."
  [thread candidate]
  (try
    (let [entity (thread-entity thread)
          client (connect!)
          facts (try (facts-of client entity)
                     (finally (rpc-close! client)))
          values (set (north.terminal-projection/done-bar-values facts))
          canonical (north.terminal-projection/canonical-evidence-text candidate)
          new? (not (contains? values candidate))
          total (if new? (inc (count values)) (count values))
          limit north.terminal-projection/max-delivery-bars
          bar-limit north.terminal-projection/max-delivery-bar-utf8-bytes
          bytes (or (north.terminal-projection/utf8-byte-count
                     (or canonical (str candidate)))
                    0)]
      (binding [*out* *err*]
        (when (> total limit)
          (println
           (str "north: WARNING — " entity " would carry " total
                " done_when bars; the delivery-evidence reserve limit is " limit
                ", so managed runs on this thread cannot reserve."
                " Retire stale bars in one step: north bars prune "
                (subs entity 1))))
        (when (> bytes bar-limit)
          (println
           (str "north: WARNING — this bar is " bytes " UTF-8 bytes; the"
                " per-bar reserve limit is " bar-limit
                " bytes, so managed runs on " entity " cannot reserve."
                " Split it into separate probe + expected-result bars.")))))
    ;; Advisory only: a missing coordinator must not make an ordinary tell fail.
    (catch Exception _ nil))
  nil)

(defn- parse-prune-options [args]
  (loop [remaining args options {:bars [] :dry-run false}]
    (if-let [argument (first remaining)]
      (case argument
        "--dry-run" (recur (rest remaining) (assoc options :dry-run true))
        "--bar" (let [value (second remaining)]
                  (when-not value (die! "--bar requires an exact bar value"))
                  (recur (drop 2 remaining)
                         (update options :bars conj value)))
        (die! (str "unknown option " (pr-str argument) "\n" usage)))
      options)))

(defn -main [& args]
  (let [[verb thread & rest-args] args]
    (case verb
      "list" (do (when-not thread (die! usage)) (cmd-list thread))
      "prune" (do (when-not thread (die! usage))
                  (cmd-prune thread (parse-prune-options rest-args)))
      "check" (do (when-not (and thread (first rest-args)) (die! usage))
                  (cmd-check thread (first rest-args)))
      "echo" (do (when-not thread (die! usage)) (cmd-echo thread))
      ("--help" "-h" "help" nil) (println usage)
      (die! (str "unknown verb " (pr-str verb) "\n" usage)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
