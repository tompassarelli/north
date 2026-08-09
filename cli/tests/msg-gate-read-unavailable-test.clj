#!/usr/bin/env bb
;; Rung 0 caller surface — the message gate must not conflate an unreadable
;; projection with "target not live". When the lifecycle read cannot be
;; completed (here: nothing is listening, so the very first read throws), the
;; gate must reject as READ-UNAVAILABLE (exit 3), distinct from a genuine
;; negative like offline/terminal/unsupported (exit 2). Daemon-free: the target
;; port is deliberately left unbound so the read fails at connect.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def msg-cli (str root "/cli/msg-cli.clj"))

(defn free-unbound-port []
  ;; Bind then immediately release: the port is free the instant we return it.
  (let [s (java.net.ServerSocket. 0)]
    (.setReuseAddress s true)
    (let [p (.getLocalPort s)] (.close s) p)))

(def checks (atom []))
(defn check! [label value] (swap! checks conj [label (boolean value)]))

(let [port (free-unbound-port)
      ;; A `message`-subject send drives require-live-message! before any write.
      result (proc/shell
              {:continue true :out :string :err :string
               :extra-env {"FRAM_LOG" "/tmp/north-msg-gate-test.log"
                           ;; Pass the topology-authority gate so the flow reaches
                           ;; the lifecycle READ — that is the surface under test.
                           "AGENT_TOPOLOGY" "orchestrator"
                           "NORTH_COORD_CONNECT_TIMEOUT_MS" "200"
                           "NORTH_COORD_READ_TIMEOUT_MS" "200"}}
              "bb" msg-cli (str port) "send" "director" "some-target" "msg" "body")]
  (check! "unreadable msg projection exits 3 (read-unavailable), not 2 (negative)"
          (= 3 (:exit result)))
  (check! "the rejection names read-unavailable, never 'target not live/offline'"
          (and (str/includes? (:err result) "read-unavailable")
               (not (str/includes? (:err result) "offline"))
               (not (str/includes? (:err result) "terminal")))))

(let [failed (remove second @checks)]
  (doseq [[label ok?] @checks]
    (println (str (if ok? "PASS " "FAIL ") label)))
  (if (seq failed)
    (do (println (str "msg gate read-unavailable: " (count failed) " FAILED"))
        (System/exit 1))
    (println (str "msg gate read-unavailable: PASS (" (count @checks) " checks)"))))
