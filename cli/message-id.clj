(ns north.message-id
  (:require [clojure.java.io :as io]))

^{:line 13 :file "cli/message-id.bclj"} (def timestamp-format ^{:line 14 :file "cli/message-id.bclj"} (java.time.format.DateTimeFormatter/ofPattern "yyyyMMdd-HHmmss"))

^{:line 16 :file "cli/message-id.bclj"} (def actor-key-namespaces ^{:line 16 :file "cli/message-id.bclj"} #{"agent" "session" "managed"})

^{:line 17 :file "cli/message-id.bclj"} (def actor-key-pattern #"^[A-Za-z0-9._:-]+$")

^{:line 18 :file "cli/message-id.bclj"} (def actor-key-max-bytes 512)

^{:line 20 :file "cli/message-id.bclj"} (defn utf8-bytes [value]
  ^{:line 21 :file "cli/message-id.bclj"} (alength ^{:line 21 :file "cli/message-id.bclj"} (.getBytes ^{:line 21 :file "cli/message-id.bclj"} (str value) "UTF-8")))

^{:line 23 :file "cli/message-id.bclj"} (defn ^String sha256 [^String value]
  ^{:line 24 :file "cli/message-id.bclj"} (let [digest ^{:line 24 :file "cli/message-id.bclj"} (.digest ^{:line 24 :file "cli/message-id.bclj"} (java.security.MessageDigest/getInstance "SHA-256") ^{:line 25 :file "cli/message-id.bclj"} (.getBytes value "UTF-8"))]
  ^{:line 26 :file "cli/message-id.bclj"} (apply str ^{:line 27 :file "cli/message-id.bclj"} (map ^{:line 27 :file "cli/message-id.bclj"} (fn [byte] ^{:line 28 :file "cli/message-id.bclj"} (format "%02x" ^{:line 28 :file "cli/message-id.bclj"} (bit-and ^{:line 28 :file "cli/message-id.bclj"} (int byte) 255))) digest))))

^{:line 31 :file "cli/message-id.bclj"} (defn actor-key
  "Derive one filesystem-safe domain-separated key for an intentional North actor." [^String namespace ^String raw]
  ^{:line 35 :file "cli/message-id.bclj"} (if ^{:line 35 :file "cli/message-id.bclj"} (and ^{:line 35 :file "cli/message-id.bclj"} (contains? actor-key-namespaces namespace) ^{:line 36 :file "cli/message-id.bclj"} (not ^{:line 36 :file "cli/message-id.bclj"} (empty? raw)) ^{:line 37 :file "cli/message-id.bclj"} (<= ^{:line 37 :file "cli/message-id.bclj"} (utf8-bytes raw) actor-key-max-bytes) ^{:line 38 :file "cli/message-id.bclj"} (boolean ^{:line 38 :file "cli/message-id.bclj"} (re-matches actor-key-pattern raw))) ^{:line 35 :file "cli/message-id.bclj"} (do
  ^{:line 39 :file "cli/message-id.bclj"} (sha256 ^{:line 39 :file "cli/message-id.bclj"} (str "north-actor-key-v1\u0000" namespace "\u0000" raw)))))

^{:line 41 :file "cli/message-id.bclj"} (defn fresh-id
  "Sortable timestamp plus UUIDv4's 122 random bits.\n   Sender is accepted for producer call-site context but never enters the\n   durable subject id. The three-argument form is the deterministic test seam."
  ([from]
    ^{:line 46 :file "cli/message-id.bclj"} (fresh-id from ^{:line 46 :file "cli/message-id.bclj"} (java.time.LocalDateTime/now) ^{:line 46 :file "cli/message-id.bclj"} (java.util.UUID/randomUUID)))
  ([_from now uuid]
    ^{:line 50 :file "cli/message-id.bclj"} (str ^{:line 50 :file "cli/message-id.bclj"} (.format now timestamp-format) "-" uuid)))

^{:line 52 :file "cli/message-id.bclj"} (defn- ^Boolean direct-invocation? []
  ^{:line 53 :file "cli/message-id.bclj"} (= ^{:line 53 :file "cli/message-id.bclj"} (.getCanonicalPath ^{:line 53 :file "cli/message-id.bclj"} (io/file *file*)) ^{:line 54 :file "cli/message-id.bclj"} (.getCanonicalPath ^{:line 55 :file "cli/message-id.bclj"} (io/file ^{:line 55 :file "cli/message-id.bclj"} (System/getProperty "babashka.file")))))

^{:line 57 :file "cli/message-id.bclj"} (defn- run-cli! [args]
  ^{:line 58 :file "cli/message-id.bclj"} (if ^{:line 58 :file "cli/message-id.bclj"} (and ^{:line 58 :file "cli/message-id.bclj"} (= 3 ^{:line 58 :file "cli/message-id.bclj"} (count args)) ^{:line 59 :file "cli/message-id.bclj"} (= "actor-key" ^{:line 59 :file "cli/message-id.bclj"} (first args))) ^{:line 60 :file "cli/message-id.bclj"} (let [bind__0 ^{:line 60 :file "cli/message-id.bclj"} (actor-key ^{:line 60 :file "cli/message-id.bclj"} (nth args 1) ^{:line 60 :file "cli/message-id.bclj"} (nth args 2))]
  ^{:line 60 :file "cli/message-id.bclj"} (if bind__0 ^{:line 60 :file "cli/message-id.bclj"} (let [key bind__0]
  ^{:line 61 :file "cli/message-id.bclj"} (print key)) ^{:line 62 :file "cli/message-id.bclj"} (System/exit 2))) ^{:line 63 :file "cli/message-id.bclj"} (System/exit 2)))

^{:line 65 :file "cli/message-id.bclj"} (if ^{:line 65 :file "cli/message-id.bclj"} (direct-invocation?) ^{:line 65 :file "cli/message-id.bclj"} (do
  ^{:line 66 :file "cli/message-id.bclj"} (run-cli! ^{:line 66 :file "cli/message-id.bclj"} (vec *command-line-args*))))
