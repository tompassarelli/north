#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def failures (atom 0))
(def checks (atom 0))

(defn check [label pass?]
  (swap! checks inc)
  (println (if pass? "PASS" "FAIL") label)
  (when-not pass? (swap! failures inc)))

(let [id "019fa542-98de-73bb-a2da-9acc68adca4b"
      subject (str "@" id)
      calls (atom [])]
  (with-redefs
   [north.coord/show-rows
    (fn [port requested]
      (swap! calls conj [port requested])
      [["owner" "personal"] ["title" "Valid title-bearing thread"]])]
    (check "spawn title validation accepts a thread visible through exact show"
           (title-bearing-thread? id))
    (check "spawn title validation asks for the exact canonical subject once"
           (= [[7977 subject]] @calls))))

(let [id "019fa542-98de-73bb-a2da-9acc68adca40"]
  (with-redefs
   [north.coord/show-rows (fn [_ _] [["owner" "personal"]])]
    (check "spawn title validation still rejects a factful non-thread"
           (not (title-bearing-thread? id)))))

(println (format "spawn-thread-title-validation: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
