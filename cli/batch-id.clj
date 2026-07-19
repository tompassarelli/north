(ns north.batch-id)

(def ^:private timestamp-format
  (java.time.format.DateTimeFormatter/ofPattern "yyyyMMdd-HHmmss"))

(defn fresh-id
  "Sortable fixture-batch prefix plus UUIDv4's full collision domain."
  ([]
   (fresh-id (java.time.LocalDateTime/now) (java.util.UUID/randomUUID)))
  ([now uuid]
   (str (.format ^java.time.LocalDateTime now timestamp-format)
        "-" uuid)))
