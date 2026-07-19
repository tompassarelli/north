(ns north.message-id)

(def ^:private timestamp-format
  (java.time.format.DateTimeFormatter/ofPattern "yyyyMMdd-HHmmss"))

(defn fresh-id
  "Sortable timestamp plus UUIDv4's 122 random bits.
   Sender is accepted for call-site compatibility but never enters the durable
   subject id. The three-argument form is the deterministic test seam."
  ([from]
   (fresh-id from (java.time.LocalDateTime/now) (java.util.UUID/randomUUID)))
  ([_from now uuid]
   (str (.format ^java.time.LocalDateTime now timestamp-format)
        "-" uuid)))
