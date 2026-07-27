#!/usr/bin/env bb
;; Representative CLI parity sweep without sockets. The real coordinator
;; handler supplies warm snapshots; the same scratch log supplies cold folds.
(require '[clojure.java.io :as io]
         '[fram.kernel :as k]
         '[fram.main :as fram-main]
         '[fram.rt :as rt]
         '[north.main :as north-main])

(def fram-root
  (or (System/getenv "FRAM_HOME")
      (str (System/getProperty "user.home") "/code/fram")))
(load-file (str fram-root "/coord_daemon.clj"))
(load-file (str fram-root "/bin/fram-fast.clj"))
(reset! snapshot-boot-enabled? false)

(def failures (atom 0))
(def checks (atom 0))

(defn check! [label expected actual]
  (swap! checks inc)
  (let [pass? (= expected actual)]
    (println (if pass? "PASS" "FAIL") label)
    (when-not pass?
      (swap! failures inc)
      (println "  cold:" (pr-str expected))
      (println "  warm:" (pr-str actual)))))

(def parity-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-daemon-cold-parity-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def parity-log (.getCanonicalPath (io/file parity-dir "facts.log")))
(def parity-subject "@019fa542-98de-73bb-a2da-9acc68adca4b")
(def parity-id (subs parity-subject 1))

(spit parity-log
      (apply
       str
       (map-indexed
        (fn [index [predicate value]]
          (str
           (pr-str {:tx (inc index) :op "assert" :l parity-subject
                    :p predicate :r value :by "parity-test"})
           "\n"))
        [["title" "Parity thread"]
         ["owner" "personal"]
         ["kind" "thread"]
         ["committed" "2026-07-28"]])))

(boot-flat! parity-log)

(def warm-response (handle {:op :facts}))
(def parity-warm-facts
  (mapv (fn [[subject predicate value]]
          (k/->Fact subject predicate value))
        (:facts warm-response)))

(let [cold (with-redefs [rt/coord-live-facts (fn [& _] [])]
             (with-out-str (fram-main/cmd-show parity-log parity-id false)))
      warm (with-redefs [rt/coord-port (constantly 7977)
                         rt/coord-show-for-log
                         (fn [_ _ subject]
                           (handle {:op :show :te subject}))]
             (with-out-str
               (fram-fast/fast-show! parity-log parity-id false)))]
  (check! "show stdout is byte-identical warm vs cold" cold warm))

(let [cold (with-redefs [rt/coord-version-for-log (fn [& _] 5)
                         rt/coord-assert-for-log (fn [& _] "ok:6")]
             (with-out-str
               (fram-main/cmd-tell
                parity-log "assert" parity-id "progress" "parity write")))
      warm (with-redefs [rt/coord-port (constantly 7977)
                         rt/coord-version-for-log (fn [& _] 5)
                         rt/coord-assert-for-log (fn [& _] "ok:6")]
             (with-out-str
               (fram-fast/fast-write!
                parity-log "assert" parity-id "progress" "parity write")))]
  (check! "tell stdout is byte-identical warm vs cold" cold warm))

(doseq [[label render]
        [["board" #(north-main/cmd-board parity-log false)]
         ["ready" #(north-main/cmd-ready parity-log false)]]]
  (let [cold (with-redefs [rt/coord-live-facts (fn [& _] [])]
               (with-out-str (render)))
        warm (with-redefs [rt/coord-live-facts (fn [& _] parity-warm-facts)]
               (with-out-str (render)))]
    (check! (str label " stdout is byte-identical warm vs cold") cold warm)))

(println (format "daemon-cold-output-parity: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
