#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/message-routing.clj"))

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

(defn resolved [_ subject predicate]
  (get facts [subject predicate]))

(defn page [_ query _ _]
  (let [rules (get query :rules)
        body (get-in rules [0 :body])
        role (some #(when (= "holds" (second (:args %)))
                      (nth (:args %) 2))
                   body)
        same-route? (some #(= "repo" (second (:args %))) body)]
    {:ok
     (cond
       (= role "@role:legacy-reviewer") [["@agent:armed-session"]]
       same-route? [["@agent:dead-session"] ["@agent:live-session"]]
       :else [])
     :more false
     :next nil}))

(with-redefs [north.coord/resolved resolved
              north.coord/query-page page
              north.coord/online? (fn [_ control] (= control "live-session"))]
  (check "direct live recipient passes"
         (= {:address "live-session" :recipient "live-session"
             :kind :direct :live true}
            (north.message-routing/require-live-address 1 "live-session")))
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
            (:kind (north.message-routing/require-live-address 1 "*")))))

(let [failed (remove second @checks)]
  (println (str "message routing: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
