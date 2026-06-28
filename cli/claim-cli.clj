;; claim-cli.clj <port> {claim|release|status} <thread> [holder] [ttl-ms]
;; Phase 3 — atomic work-claiming. REUSE the fram exclusive-lease primitive
;; (cnf_coord acquire-lease!/release-lease!): a holder leases @lease:<thread> with
;; mutual exclusion enforced IN the coordinator's lock, so two agents racing to drive
;; the SAME thread can't both win — exactly the race agentchat's advisory lockfiles
;; never closed. On grant we also set driver=<holder> for board visibility.
(require '[clojure.edn :as edn] '[clojure.java.io :as io])

(defn send-op [port op]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w (.getBytes (str (pr-str op) "\n"))) (.flush w)
      (edn/read-string (.readLine r)))))

(defn lease-key [thread] (str "@lease:" thread))

(let [[ps verb & args] *command-line-args*
      port (Integer/parseInt ps)]
  (case verb
    "claim"                              ; <thread> <holder> [ttl-ms]  — atomic claim-the-driver
    (let [[thread holder ttl] args
          ttl-ms (Integer/parseInt (or ttl "600000"))
          r (send-op port {:op :acquire-lease :holder holder :res (lease-key thread) :ttl-ms ttl-ms})]
      (if (:reject r)
        (do (println (format "DENIED %s — held by %s (epoch %s)" thread (:holder r) (:epoch r)))
            (System/exit 1))
        (let [v (:version (send-op port {:op :version}))]
          (send-op port {:op :assert :te thread :p "driver" :r (str "@" holder) :base v})  ; board visibility
          (println (format "CLAIMED %s by %s (epoch %s, ttl %ds)" thread holder (:epoch r) (quot ttl-ms 1000))))))

    "release"                            ; <thread> <holder>
    (let [[thread holder] args
          r (send-op port {:op :release-lease :holder holder :res (lease-key thread)})]
      (println (format "released %s by %s -> %s" thread holder (pr-str r))))

    "status"                             ; <thread> — who holds it (fence check w/ epoch 0 reports current holder)
    (let [[thread] args
          d (:value (send-op port {:op :resolved :te thread :p "driver"}))]
      (println (format "%s driver=%s" thread (or d "(none)"))))

    (do (println "usage: claim-cli.clj <port> {claim|release|status} <thread> [holder] [ttl-ms]") (System/exit 2))))
