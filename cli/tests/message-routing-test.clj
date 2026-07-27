#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
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
   ["@agent:armed-session" "live_input_state"] "armed"})

(def acknowledged #{"@msg:acked"})

(defn resolved [_ subject predicate]
  (get facts [subject predicate]))

(defn page [_ query _ _]
  (let [rules (get query :rules)
        body (get-in rules [0 :body])
        role (some #(when (= "holds" (second (:args %)))
                      (nth (:args %) 2))
                   body)
        same-route? (some #(= "repo" (second (:args %))) body)
        mail? (= "mail_candidate" (:find query))]
    {:ok
     (cond
       mail? [["@msg:dead" "sender-a" "dead-session" "2026-07-27T10:00:00Z"]
              ["@msg:live" "sender-b" "live-session" "2026-07-27T11:00:00Z"]
              ["@msg:acked" "sender-c" "dead-session" "2026-07-27T09:00:00Z"]]
       (= role "@role:legacy-reviewer") [["@agent:armed-session"]]
       same-route? [["@agent:dead-session"] ["@agent:live-session"]]
       :else [])
     :more false
     :next nil}))

(with-redefs [north.coord/resolved resolved
              north.coord/query-page page
              north.coord/many
              (fn [_ subject predicate]
                (if (and (= predicate "acked_by")
                         (contains? acknowledged subject))
                  ["recipient"]
                  []))
              north.coord/online? (fn [_ control] (= control "live-session"))]
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
  (check "armed listener passes without a lease"
         (= "armed-session"
            (:recipient
             (north.message-routing/require-live-address
              1 "legacy-reviewer"))))
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
         (= [{:message "@msg:dead"
              :sender "sender-a"
              :recipient "dead-session"
              :resolved-recipient "dead-session"
              :age-ms 7200000
              :age "2h"}]
            (:rows
             (north.message-routing/dead-letter-scan
              1 (java.time.Instant/parse "2026-07-27T12:00:00Z"))))))

(let [result
      (proc/shell
       {:continue true :out :string :err :string}
       "env" "NORTH_DEAD_RECIPIENT_CHILD=1"
       "bb" (System/getProperty "babashka.file"))
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
