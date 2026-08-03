#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label pass?]
  (swap! checks conj [label (boolean pass?)])
  (println (if pass? "PASS" "FAIL") label))

(defn captured-warning [route]
  (with-out-str
    (with-redefs [north.message-routing/require-live-address (fn [_ _] route)]
      (warn-unarmed-notify! "coordinator"))))

(let [unarmed (captured-warning {:live false :recipient "coordinator"})
      armed (captured-warning {:live true :recipient "coordinator"})]
  (check "spawn with an unarmed notify target emits the wake warning"
         (str/includes? unarmed
                        "NOTIFY TARGET coordinator HAS NO ARMED LISTENER — completions will not wake it; arm: north listen coordinator"))
  (check "spawn with an armed notify target emits no wake warning"
         (str/blank? armed)))

(let [pass (count (filter second @checks))]
  (println (format "spawn notify listener warning: %d / %d PASS" pass (count @checks)))
  (System/exit (if (= pass (count @checks)) 0 1)))
