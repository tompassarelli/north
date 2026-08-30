(ns north.message-contract
  (:require [clojure.string :as str]))

^{:line 8 :file "cli/message-contract.bclj"} (def ^String broadcast-address "*")

^{:line 9 :file "cli/message-contract.bclj"} (def max-message-id-bytes 512)

^{:line 10 :file "cli/message-contract.bclj"} (def max-target-bytes 512)

^{:line 11 :file "cli/message-contract.bclj"} (def max-sender-bytes 1024)

^{:line 12 :file "cli/message-contract.bclj"} (def max-subject-bytes ^{:line 12 :file "cli/message-contract.bclj"} (* 16 1024))

^{:line 13 :file "cli/message-contract.bclj"} (def max-body-bytes ^{:line 13 :file "cli/message-contract.bclj"} (* 128 1024))

^{:line 14 :file "cli/message-contract.bclj"} (def handle-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

^{:line 16 :file "cli/message-contract.bclj"} (defn utf8-bytes [value]
  ^{:line 17 :file "cli/message-contract.bclj"} (alength ^{:line 17 :file "cli/message-contract.bclj"} (.getBytes ^{:line 17 :file "cli/message-contract.bclj"} (str value) "UTF-8")))

^{:line 19 :file "cli/message-contract.bclj"} (defn ^Boolean safe-handle? [value maximum]
  ^{:line 22 :file "cli/message-contract.bclj"} (and ^{:line 22 :file "cli/message-contract.bclj"} (string? value) ^{:line 23 :file "cli/message-contract.bclj"} (not ^{:line 23 :file "cli/message-contract.bclj"} (str/blank? value)) ^{:line 24 :file "cli/message-contract.bclj"} (<= ^{:line 24 :file "cli/message-contract.bclj"} (utf8-bytes value) maximum) ^{:line 25 :file "cli/message-contract.bclj"} (boolean ^{:line 25 :file "cli/message-contract.bclj"} (re-matches handle-pattern value))))

^{:line 27 :file "cli/message-contract.bclj"} (defn sender-problem [value]
  ^{:line 28 :file "cli/message-contract.bclj"} (cond
  ^{:line 29 :file "cli/message-contract.bclj"} (not ^{:line 29 :file "cli/message-contract.bclj"} (string? value)) "missing_sender"
  ^{:line 30 :file "cli/message-contract.bclj"} (> ^{:line 30 :file "cli/message-contract.bclj"} (utf8-bytes value) max-sender-bytes) "sender_too_large"
  ^{:line 31 :file "cli/message-contract.bclj"} (not ^{:line 31 :file "cli/message-contract.bclj"} (safe-handle? value max-sender-bytes)) "invalid_sender"
  :else nil))

^{:line 34 :file "cli/message-contract.bclj"} (defn subject-problem [value]
  ^{:line 35 :file "cli/message-contract.bclj"} (cond
  ^{:line 36 :file "cli/message-contract.bclj"} (not ^{:line 36 :file "cli/message-contract.bclj"} (string? value)) "missing_subject"
  ^{:line 37 :file "cli/message-contract.bclj"} (> ^{:line 37 :file "cli/message-contract.bclj"} (utf8-bytes value) max-subject-bytes) "subject_too_large"
  ^{:line 38 :file "cli/message-contract.bclj"} (or ^{:line 38 :file "cli/message-contract.bclj"} (str/blank? value) ^{:line 39 :file "cli/message-contract.bclj"} (re-find #"[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]" value)) "invalid_subject"
  :else nil))

^{:line 43 :file "cli/message-contract.bclj"} (defn body-problem [value]
  ^{:line 44 :file "cli/message-contract.bclj"} (cond
  ^{:line 45 :file "cli/message-contract.bclj"} (not ^{:line 45 :file "cli/message-contract.bclj"} (string? value)) "missing_body"
  ^{:line 46 :file "cli/message-contract.bclj"} (> ^{:line 46 :file "cli/message-contract.bclj"} (utf8-bytes value) max-body-bytes) "body_too_large"
  ^{:line 47 :file "cli/message-contract.bclj"} (or ^{:line 47 :file "cli/message-contract.bclj"} (str/blank? value) ^{:line 47 :file "cli/message-contract.bclj"} (str/includes? value "\u0000")) "invalid_body"
  :else nil))

^{:line 50 :file "cli/message-contract.bclj"} (def producer-diagnostic ^{:line 51 :file "cli/message-contract.bclj"} {"missing_sender" "sender is malformed or too large" "invalid_sender" "sender is malformed or too large" "sender_too_large" "sender is malformed or too large" "missing_subject" "subject is blank, malformed, or too large" "invalid_subject" "subject is blank, malformed, or too large" "subject_too_large" "subject is blank, malformed, or too large" "missing_body" "body is blank, malformed, or too large" "invalid_body" "body is blank, malformed, or too large" "body_too_large" "body is blank, malformed, or too large"})

^{:line 61 :file "cli/message-contract.bclj"} (defn input-problem
  "Return a stable reason before the canonical producer performs any write." [from to subject body]
  ^{:line 67 :file "cli/message-contract.bclj"} (or ^{:line 68 :file "cli/message-contract.bclj"} (some-> ^{:line 68 :file "cli/message-contract.bclj"} (sender-problem from) producer-diagnostic) ^{:line 69 :file "cli/message-contract.bclj"} (if ^{:line 69 :file "cli/message-contract.bclj"} (not ^{:line 69 :file "cli/message-contract.bclj"} (or ^{:line 69 :file "cli/message-contract.bclj"} (= broadcast-address to) ^{:line 70 :file "cli/message-contract.bclj"} (safe-handle? to max-target-bytes))) ^{:line 69 :file "cli/message-contract.bclj"} (do
  "target is malformed or too large")) ^{:line 72 :file "cli/message-contract.bclj"} (some-> ^{:line 72 :file "cli/message-contract.bclj"} (subject-problem subject) producer-diagnostic) ^{:line 73 :file "cli/message-contract.bclj"} (some-> ^{:line 73 :file "cli/message-contract.bclj"} (body-problem body) producer-diagnostic)))
