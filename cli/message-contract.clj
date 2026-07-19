(ns north.message-contract
  "Canonical producer/feed byte and shape contract for durable North mail.")

(require '[clojure.string :as str])

(def broadcast-address "*")
(def max-message-id-bytes 512)
(def max-target-bytes 512)
(def max-sender-bytes 1024)
(def max-subject-bytes (* 16 1024))
(def max-body-bytes (* 128 1024))
(def handle-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn safe-handle? [value maximum]
  (and (string? value)
       (not (str/blank? value))
       (<= (utf8-bytes value) maximum)
       (boolean (re-matches handle-pattern value))))

(defn sender-problem [value]
  (cond
    (not (string? value)) "missing_sender"
    (> (utf8-bytes value) max-sender-bytes) "sender_too_large"
    (not (safe-handle? value max-sender-bytes)) "invalid_sender"
    :else nil))

(defn subject-problem [value]
  (cond
    (not (string? value)) "missing_subject"
    (> (utf8-bytes value) max-subject-bytes) "subject_too_large"
    (or (str/blank? value)
        (re-find #"[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]" value))
    "invalid_subject"
    :else nil))

(defn body-problem [value]
  (cond
    (not (string? value)) "missing_body"
    (> (utf8-bytes value) max-body-bytes) "body_too_large"
    (or (str/blank? value) (str/includes? value "\u0000")) "invalid_body"
    :else nil))

(def producer-diagnostic
  {"missing_sender" "sender is malformed or too large"
   "invalid_sender" "sender is malformed or too large"
   "sender_too_large" "sender is malformed or too large"
   "missing_subject" "subject is blank, malformed, or too large"
   "invalid_subject" "subject is blank, malformed, or too large"
   "subject_too_large" "subject is blank, malformed, or too large"
   "missing_body" "body is blank, malformed, or too large"
   "invalid_body" "body is blank, malformed, or too large"
   "body_too_large" "body is blank, malformed, or too large"})

(defn input-problem
  "Return a stable reason before the canonical producer performs any write."
  [from to subject body]
  (or
   (some-> (sender-problem from) producer-diagnostic)

   (when-not (or (= broadcast-address to)
                 (safe-handle? to max-target-bytes))
     "target is malformed or too large")

   (some-> (subject-problem subject) producer-diagnostic)

   (some-> (body-problem body) producer-diagnostic)))
