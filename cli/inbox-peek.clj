;; inbox-peek.clj — fast, quiet mail heartbeat for the PostToolUse hook.
;; Usage: bb inbox-peek.clj <port> <agent-id>
;; Find unacked messages for <agent-id> (direct OR in a finite broadcast snapshot),
;; print each
;; readable, then ACK each (acked_by + acked_at) so it's delivered exactly once and never
;; re-surfaces. No unacked mail => print NOTHING, exit 0. Standalone: helper fns copied
;; from the shared coordination and message-audience substrates.
(require '[clojure.java.io :as io])

;; Shared coordination reads plus message-audience's claim/ack protocol.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(def one     north.coord/resolved)

(let [[port me] *command-line-args*
      port (Integer/parseInt port)]
  (doseq [e (sort (north.message-audience/pending-message-ids port me #{me}))]
    (when-let [claim (north.message-audience/claim-delivery! port e me)]
      (let [from (or (one port e "from") "?")
            subj (or (one port e "subject") "")
            body (or (one port e "body") "")]
        (println (str "✉ from " from " — " subj))
        (println (str "  " body))
        ;; Claim first, then flush output before the durable ack. The claim
        ;; prevents a live listener from printing concurrently; if this process
        ;; dies after print, lease expiry deliberately preserves at-least-once.
        (flush)
        (north.message-audience/complete-delivery! port e me claim)))))
