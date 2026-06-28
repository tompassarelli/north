;; inbox-peek.clj — fast, quiet mail heartbeat for the PostToolUse hook.
;; Usage: bb inbox-peek.clj <port> <agent-id>
;; Find unacked messages for <agent-id> (to∈{id,"*"} AND id NOT in acked_by), print each
;; readable, then ACK each (acked_by + acked_at) so it's delivered exactly once and never
;; re-surfaces. No unacked mail => print NOTHING, exit 0. Standalone: helper fns copied
;; verbatim from msg-cli.clj (send-op/assert!/one/many/messages/for-me?).
(require '[clojure.edn :as edn] '[clojure.java.io :as io])

(defn send-op [port op]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w (.getBytes (str (pr-str op) "\n"))) (.flush w)
      (edn/read-string (.readLine r)))))

(defn assert! [port te p r]                 ; OCC at current :version; retry on reject
  (loop [tries 4]
    (let [v (:version (send-op port {:op :version}))
          res (send-op port {:op :assert :te te :p p :r (str r) :base v})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn one  [port te p] (:value  (send-op port {:op :resolved :te te :p p})))
(defn many [port te p] (:values (send-op port {:op :resolved :te te :p p})))  ; all values (multi-valued)

(defn messages [port]      ; -> [[@msg-entity to-handle] ...]  (every entity carrying a `to`)
  (:ok (send-op port {:op :query
                      :query {:find "m"
                              :rules [{:head {:rel "m" :args [{:var "e"} {:var "to"}]}
                                       :body [{:rel "triple" :args [{:var "e"} "to" {:var "to"}]}]}]}})))

(defn for-me? [to me] (or (= to me) (= to "*")))

(let [[port me] *command-line-args*
      port (Integer/parseInt port)]
  (doseq [[e to] (sort (or (messages port) []))]
    (when (and (for-me? to me) (not (contains? (set (many port e "acked_by")) me)))
      (let [from (or (one port e "from") "?")
            subj (or (one port e "subject") "")
            body (or (one port e "body") "")]
        (println (str "✉ from " from " — " subj))
        (println (str "  " body))
        ;; ack last: print delivers, then mark so it never re-surfaces (exactly-once on success)
        (assert! port e "acked_by" me)
        (assert! port e "acked_at" (str (java.time.Instant/now)))))))
