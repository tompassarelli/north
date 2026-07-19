;; inbox-peek.clj — fast, quiet mail heartbeat for the PostToolUse hook.
;; Usage: bb inbox-peek.clj <port> <agent-id>
;; Find a bounded first page of unacked messages for <agent-id> (direct OR in a
;; finite broadcast snapshot), print+flush a small bounded context, then ACK only
;; each message whose complete rendering was emitted. No deliverable bounded mail
;; => print NOTHING, exit 0.
(require '[clojure.java.io :as io])

;; Shared coordination reads plus message-audience's claim/ack protocol.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(def one     north.coord/resolved)
(def candidate-limit 3)
(def delivery-limit 3)
(def output-byte-limit (* 24 1024))
(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))
(defn sha256 [value]
  (let [digest
        (.digest
         (java.security.MessageDigest/getInstance "SHA-256")
         (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8))]
    (apply str
           (map #(format "%02x" (bit-and (int %) 0xff)) digest))))
(defn valid-cursor? [value]
  (and (string? value)
       (<= (utf8-bytes value) 4096)
       ;; Fram cursors are versioned, canonical, unpadded base64url. Accept
       ;; exactly that wire shape; notably, the namespace separator is a dot.
       (boolean
        (re-matches
         #"^fram-query-page-v1\.[A-Za-z0-9_-]+$"
         value))))
(defn persist-cursor! [file cursor]
  (try
    (if (valid-cursor? cursor)
      (let [tmp (io/file (str (.getPath file) "." (java.util.UUID/randomUUID) ".tmp"))]
        (spit tmp cursor)
        (java.nio.file.Files/move
         (.toPath tmp)
         (.toPath file)
         (into-array
          java.nio.file.CopyOption
          [java.nio.file.StandardCopyOption/REPLACE_EXISTING
           java.nio.file.StandardCopyOption/ATOMIC_MOVE])))
      (io/delete-file file true))
    (catch Exception _ nil)))

(let [[port me] *command-line-args*
      port (Integer/parseInt port)
      cursor-dir
      (io/file (or (System/getenv "XDG_RUNTIME_DIR") "/tmp")
               "north-inbox-peek")
      _ (.mkdirs cursor-dir)
      cursor-file (io/file cursor-dir (sha256 (str "recipient\u0000" me)))
      stored-cursor
      (try
        (when (.isFile cursor-file)
          (let [value (slurp cursor-file)]
            (when (valid-cursor? value) value)))
        (catch Exception _ nil))
      _ (when (and (.exists cursor-file) (nil? stored-cursor))
          (io/delete-file cursor-file true))
      page
      (north.message-audience/pending-message-page
       port me #{me} candidate-limit stored-cursor)
      messages
      (:messages page)]
  (loop [remaining messages emitted 0 output-bytes 0]
    (when (and (seq remaining) (< emitted delivery-limit))
      (let [e (first remaining)]
        (if-let [claim
                 (north.message-audience/claim-delivery! port e me)]
          (let [from (or (one port e "from") "?")
                subj (or (one port e "subject") "")
                body (or (one port e "body") "")
                rendered
                (str "✉ from " from " — " subj "\n"
                     "  " body "\n")
                rendered-bytes (utf8-bytes rendered)]
            (if (<= (+ output-bytes rendered-bytes) output-byte-limit)
              (do
                (print rendered)
                ;; Claim first, then flush the complete bounded rendering before
                ;; durable ack. A deadline after print deliberately permits
                ;; replay; no un-emitted or partially emitted item is acked.
                (flush)
                (north.message-audience/complete-delivery!
                 port e me claim)
                (recur (next remaining)
                       (inc emitted)
                       (+ output-bytes rendered-bytes)))
              (do
                ;; Oversized-but-valid mail remains available to the interactive
                ;; inbox; this hook never silently truncates or acknowledges it.
                (north.message-audience/release-delivery-claim! port claim)
                (recur (next remaining) emitted output-bytes))))
          (recur (next remaining) emitted output-bytes)))))
  ;; One deterministic page per hook turn keeps coordinator and token cost
  ;; bounded. Advancing the deletion-safe cursor prevents a prefix of valid but
  ;; hook-oversized mail from starving later bounded messages; reaching the end
  ;; resets the cycle so unacknowledged large mail remains inspectable.
  (persist-cursor! cursor-file (when (:more page) (:next page))))
