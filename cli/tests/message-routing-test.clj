#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def fram-out
  (str (or (System/getenv "FRAM_HOME")
           "/home/tom/code/beagle/main/branch-core")
       "/out"))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/message-routing.clj"))
(let [test-file (System/getProperty "babashka.file")
      msg-file (str root "/cli/msg-cli.clj")]
  (System/setProperty "north.msg-cli.lib" "1")
  (System/setProperty "babashka.file" msg-file)
  (try
    (load-file msg-file)
    (finally
      (System/setProperty "babashka.file" test-file))))

(when (= "1" (System/getenv "NORTH_DEAD_RECIPIENT_CHILD"))
  (with-redefs
   [north.message-routing/require-live-address
    (fn [_ _]
      {:live false
       :recipient "dead-session"
       :alternative "live-session"})]
    (admitted-message-recipient! 1 "dead-session" false))
  (System/exit 99))

(def checks (atom []))
(defn check [label value]
  (swap! checks conj [label (boolean value)])
  (println (if value (str "PASS " label) (str "FAIL " label))))

(def facts
  {["@role:north-integrator" "target"] "live-session"
   ["@agent:dead-session" "repo"] "north"
   ["@agent:dead-session" "role"] "integrator"
   ["@agent:live-session" "repo"] "north"
   ["@agent:live-session" "role"] "integrator"
   ["@agent:armed-session" "kind"] "session"
   ["@agent:armed-session" "live_input_state"] "armed"
   ["@agent:armed-session" "live_input_epoch"] "00000000-0000-4000-8000-000000000201"
   ["@agent:stale-native" "kind"] "session"
   ["@agent:stale-native" "live_input_state"] "armed"
   ["@agent:stale-native" "live_input_epoch"] "00000000-0000-4000-8000-000000000202"
   ["@agent:ambiguous-state" "kind"] "session"
   ["@agent:ambiguous-state" "live_input_state"] "armed"
   ["@agent:ambiguous-state" "live_input_epoch"] "00000000-0000-4000-8000-000000000203"
   ["@agent:ambiguous-epoch" "kind"] "session"
   ["@agent:ambiguous-epoch" "live_input_state"] "armed"
   ["@agent:ambiguous-epoch" "live_input_epoch"] "00000000-0000-4000-8000-000000000204"
   ["@agent:managed-armed" "kind"] "lane"
   ["@agent:managed-armed" "live_input_state"] "armed"
   ["@agent:managed-ambiguous" "kind"] "lane"
   ["@agent:managed-ambiguous" "live_input_state"] "armed"})

(def ambiguous-values
  {["@agent:ambiguous-state" "live_input_state"] ["armed" "frozen"]
   ["@agent:ambiguous-epoch" "live_input_epoch"]
   ["00000000-0000-4000-8000-000000000204"
    "00000000-0000-4000-8000-000000000205"]
   ["@agent:managed-ambiguous" "live_input_state"] ["armed" "frozen"]})

(def mail-query-seen (atom nil))
(def many-calls (atom []))

(defn mail-query-settles-server-side? [query]
  (let [rules (mapcat identity (:strata query))
        clauses (mapcat :body rules)]
    (and (= "mail_candidate" (:find query))
         (some #(= "mail_settled" (get-in % [:head :rel])) rules)
         (some #(= "acked_by" (second (:args %))) clauses)
         (some #(= "delivery_rejected_by" (second (:args %))) clauses)
         (some #(and (= "mail_settled" (:rel %))
                     (true? (:neg %)))
               clauses))))

(defn resolved [_ subject predicate]
  (get facts [subject predicate]))

(defn resolved-envelope [_ subject predicate]
  (let [values
        (or (get ambiguous-values [subject predicate])
            (some-> (get facts [subject predicate]) vector)
            [])
        members (count values)]
    {:value (first values)
     :members members
     :ambiguous? (> members 1)
     :values values
     :version 1}))

(defn page [_ query _ _]
  (let [rules (get query :rules)
        body (get-in rules [0 :body])
        role (some #(when (= "holds" (second (:args %)))
                      (nth (:args %) 2))
                   body)
        same-route? (some #(= "repo" (second (:args %))) body)
        mail? (= "mail_candidate" (:find query))]
    (when mail?
      (reset! mail-query-seen query))
    {:rows
     (cond
       mail? (cond->
              [["@msg:dead" "sender-a" "dead-session" "2026-07-27T11:30:00Z"]
               ["@msg:live" "sender-b" "live-session" "2026-07-27T11:45:00Z"]
               ["@msg:historical" "sender-c" "old-session" "2026-07-27T09:00:00Z"]]
               (not (mail-query-settles-server-side? query))
               (conj ["@msg:acked" "sender-d" "dead-session"
                      "2026-07-27T08:00:00Z"]))
       (= role "@role:reviewer-alias") [["@agent:armed-session"]]
       same-route? [["@agent:dead-session"] ["@agent:live-session"]]
       :else [])
     :done? true
     :cursor nil
     :served-version 1}))

(with-redefs [north.coord/resolved resolved
              north.coord/resolved-envelope resolved-envelope
              north.coord/query-page page
              north.coord/many
              (fn [_ subject predicate]
                (swap! many-calls conj [subject predicate])
                [])
              north.coord/lease-status
              (fn [_ resource]
                (when-let
                 [holder
                  (get
                   {"listener:armed-session"
                    "00000000-0000-4000-8000-000000000201"
                    "listener:ambiguous-state"
                    "00000000-0000-4000-8000-000000000203"
                    "listener:ambiguous-epoch"
                   "00000000-0000-4000-8000-000000000204"}
                   resource)]
                  {:resource resource :holder holder :exp 9999999999999
                   :online? true}))
              north.coord/session-online?
              (fn [_ control] (= control "live-session"))]
  (check "direct live recipient passes"
         (= {:address "live-session" :recipient "live-session"
             :kind :direct :live true}
            (north.message-routing/require-live-address 1 "live-session")))
  (check "durable role alias resolves to its current live session"
         (= {:address "north-integrator" :recipient "live-session"
             :kind :alias :live true}
            (north.message-routing/require-live-address
             1 "north-integrator")))
  (check "msg-cli send admission uses the alias's concrete live session"
         (= "live-session"
            (admitted-message-recipient! 1 "north-integrator" false)))
  (check "msg-cli dead-drop deliberately bypasses absent-recipient admission"
         (= "dead-session"
            (admitted-message-recipient! 1 "dead-session" true)))
  (check "native armed listener passes with a matching unexpired generation lease"
         (= "armed-session"
            (:recipient
             (north.message-routing/require-live-address
              1 "reviewer-alias"))))
  (check "durable armed state without its native listener lease is unreachable"
         (false? (north.message-routing/recipient-live? 1 "stale-native")))
  (check "an ambiguous listener state rejects even when armed is selected"
         (false?
          (north.message-routing/recipient-live? 1 "ambiguous-state")))
  (check "an ambiguous listener epoch rejects even when the holder is selected"
         (false?
          (north.message-routing/recipient-live? 1 "ambiguous-epoch")))
  (check "managed lane route authority remains unchanged"
         (true? (north.message-routing/recipient-live? 1 "managed-armed")))
  (check "an ambiguous managed route state fails closed"
         (false?
          (north.message-routing/recipient-live? 1 "managed-ambiguous")))
  (check "dead recipient fails and names a live same-route successor"
         (= {:live false :recipient "dead-session"
             :alternative "live-session"}
            (select-keys
             (north.message-routing/require-live-address 1 "dead-session")
             [:live :recipient :alternative])))
  (check "broadcast remains a finite-audience special address"
         (= :broadcast
            (:kind (north.message-routing/require-live-address 1 "*"))))
  (check "dead-letter scan returns only pending mail to absent identities"
         (= [{:message "@msg:historical"
              :sender "sender-c"
              :recipient "old-session"
              :resolved-recipient "old-session"
              :age-ms 10800000
              :age "3h"}
             {:message "@msg:dead"
              :sender "sender-a"
              :recipient "dead-session"
              :resolved-recipient "dead-session"
              :age-ms 1800000
              :age "30m"}]
            (:rows
             (north.message-routing/dead-letter-scan
              1 (java.time.Instant/parse "2026-07-27T12:00:00Z")))))
  (check "mail query excludes acknowledged and rejected messages on the server"
         (mail-query-settles-server-side? @mail-query-seen))
  (check "full dead-letter scan performs no per-message many reads"
         (empty? @many-calls))
  (check "readiness keeps historical mail without probing it and checks recent mail"
         (= [{:message "@msg:historical"
              :sender "sender-c"
              :recipient "old-session"
              :resolved-recipient "old-session"
              :age-ms 10800000
              :age "3h"
              :historical? true}
             {:message "@msg:dead"
              :sender "sender-a"
              :recipient "dead-session"
              :resolved-recipient "dead-session"
              :age-ms 1800000
              :age "30m"}]
            (:rows
             (north.message-routing/readiness-dead-letter-scan
              1 3600000
              (java.time.Instant/parse "2026-07-27T12:00:00Z")))))
  (check "readiness dead-letter scan performs no per-message many reads"
         (empty? @many-calls)))

(with-redefs
 [north.coord/session-online? (fn [& _] false)
  north.coord/resolved-envelope
  (fn [& _] {:value nil :members 0 :ambiguous? false :values [] :version 1})]
  (check "an offline canonical session cannot admit a direct route"
         (false? (north.message-routing/recipient-live? 1 "offline"))))

(let [result
      (proc/shell
       {:continue true :out :string :err :string}
       "env" "NORTH_DEAD_RECIPIENT_CHILD=1"
       "bb" "-cp" fram-out (System/getProperty "babashka.file"))
      diagnostic (str (:out result) "\n" (:err result))]
  (check "msg-cli send admission hard-fails for a dead recipient"
         (= 2 (:exit result)))
  (check "dead-recipient failure names the dead id, live successor, and override"
         (and (str/includes? diagnostic "dead-session")
              (str/includes? diagnostic "live same repo/role session: live-session")
              (str/includes? diagnostic "--dead-drop"))))

(let [failed (remove second @checks)]
  (println (str "message routing: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
