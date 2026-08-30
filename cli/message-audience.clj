(ns north.message-audience
  (:require [cheshire.core :as json]
            [clojure.set :as set]
            [clojure.string :as str]
            [north.coord :as coord]))

^{:line 32 :file "cli/message-audience.bclj"} (def ^String broadcast-address "*")

^{:line 33 :file "cli/message-audience.bclj"} (def ^String audience-predicate "broadcast_to")

^{:line 34 :file "cli/message-audience.bclj"} (def ^String audience-version-predicate "broadcast_audience_version")

^{:line 35 :file "cli/message-audience.bclj"} (def ^String audience-version "snapshot-v1")

^{:line 36 :file "cli/message-audience.bclj"} (def delivery-claim-ttl-ms 30000)

^{:line 37 :file "cli/message-audience.bclj"} (def ^String rejection-predicate "delivery_rejection")

^{:line 38 :file "cli/message-audience.bclj"} (def ^String rejected-by-predicate "delivery_rejected_by")

^{:line 39 :file "cli/message-audience.bclj"} (def ^String msg-manifest-predicate "target_identity_manifest_sha256")

^{:line 40 :file "cli/message-audience.bclj"} (def rejection-reasons ^{:line 41 :file "cli/message-audience.bclj"} #{"invalid_message_id" "missing_sender" "invalid_sender" "sender_too_large" "missing_subject" "invalid_subject" "subject_too_large" "missing_body" "invalid_body" "body_too_large" "message_too_large" "msg_manifest_missing" "msg_type_invalid" "msg_route_invalid" "msg_route_stale" "msg_route_not_armed"})

^{:line 47 :file "cli/message-audience.bclj"} (def max-rejection-recipient-bytes 512)

^{:line 48 :file "cli/message-audience.bclj"} (def max-direct-addresses 256)

^{:line 49 :file "cli/message-audience.bclj"} (def max-direct-address-bytes 512)

^{:line 50 :file "cli/message-audience.bclj"} (def pending-page-limit 256)

^{:line 51 :file "cli/message-audience.bclj"} (def manifest-sha256-bytes 64)

^{:line 52 :file "cli/message-audience.bclj"} (def max-message-id-bytes 512)

^{:line 53 :file "cli/message-audience.bclj"} (defn utf8-bytes [value]
  ^{:line 54 :file "cli/message-audience.bclj"} (alength ^{:line 54 :file "cli/message-audience.bclj"} (.getBytes ^{:line 54 :file "cli/message-audience.bclj"} (str value) "UTF-8")))

^{:line 55 :file "cli/message-audience.bclj"} (def max-rejection-evidence-bytes ^{:line 60 :file "cli/message-audience.bclj"} (utf8-bytes ^{:line 61 :file "cli/message-audience.bclj"} (json/generate-string ^{:line 62 :file "cli/message-audience.bclj"} (sorted-map "expectedManifest" ^{:line 63 :file "cli/message-audience.bclj"} (apply str ^{:line 63 :file "cli/message-audience.bclj"} (repeat manifest-sha256-bytes "a")) "observedManifest" ^{:line 64 :file "cli/message-audience.bclj"} (apply str ^{:line 64 :file "cli/message-audience.bclj"} (repeat manifest-sha256-bytes "b")) "reason" ^{:line 65 :file "cli/message-audience.bclj"} (apply max-key utf8-bytes rejection-reasons) "recipient" ^{:line 66 :file "cli/message-audience.bclj"} (apply str ^{:line 66 :file "cli/message-audience.bclj"} (repeat max-rejection-recipient-bytes "r"))))))

^{:line 68 :file "cli/message-audience.bclj"} (defn ^String bare-handle [handle]
  ^{:line 69 :file "cli/message-audience.bclj"} (-> ^{:line 69 :file "cli/message-audience.bclj"} (str handle) ^{:line 70 :file "cli/message-audience.bclj"} (str/replace-first #"^@agent:" "") ^{:line 71 :file "cli/message-audience.bclj"} (str/replace-first #"^@session:" "")))

^{:line 73 :file "cli/message-audience.bclj"} (defn ^Boolean canonical-message-id?
  "Only canonical @msg subjects enter human-mail consumers. Routing predicates\n   are shared by other coordination entities, so `to` alone is never proof that\n   a subject is mail." [value]
  ^{:line 77 :file "cli/message-audience.bclj"} (and ^{:line 77 :file "cli/message-audience.bclj"} (string? value) ^{:line 78 :file "cli/message-audience.bclj"} (<= ^{:line 78 :file "cli/message-audience.bclj"} (utf8-bytes value) max-message-id-bytes) ^{:line 79 :file "cli/message-audience.bclj"} (boolean ^{:line 80 :file "cli/message-audience.bclj"} (re-matches #"^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

^{:line 82 :file "cli/message-audience.bclj"} (defn ^String canonical-message-reference
  "Normalize the CLI's optional @msg prefix without admitting any other\n   coordination subject kind." [value]
  ^{:line 85 :file "cli/message-audience.bclj"} (let [^String raw ^{:line 85 :file "cli/message-audience.bclj"} (str value)
   ^String candidate ^{:line 86 :file "cli/message-audience.bclj"} (if ^{:line 86 :file "cli/message-audience.bclj"} (str/starts-with? raw "@") raw ^{:line 88 :file "cli/message-audience.bclj"} (str "@msg:" raw))]
  ^{:line 89 :file "cli/message-audience.bclj"} (if ^{:line 89 :file "cli/message-audience.bclj"} (canonical-message-id? candidate) candidate ^{:line 91 :file "cli/message-audience.bclj"} (throw ^{:line 91 :file "cli/message-audience.bclj"} (ex-info "message id is malformed or too large" ^{:line 92 :file "cli/message-audience.bclj"} {:type :invalid-message-id})))))

^{:line 94 :file "cli/message-audience.bclj"} (defn ^Boolean complete-message-envelope?
  "A canonical subject prefix is necessary but not sufficient: require the\n   complete mail envelope that every production publisher writes before\n   its routing edge." [port ^String message]
  ^{:line 100 :file "cli/message-audience.bclj"} (let [values ^{:line 100 :file "cli/message-audience.bclj"} (mapv ^{:line 100 :file "cli/message-audience.bclj"} (fn [^String predicate] ^{:line 101 :file "cli/message-audience.bclj"} (coord/resolved! port message predicate)) ^{:line 102 :file "cli/message-audience.bclj"} ["from" "subject" "body" "sent_at"])]
  ^{:line 103 :file "cli/message-audience.bclj"} (and ^{:line 103 :file "cli/message-audience.bclj"} (canonical-message-id? message) ^{:line 104 :file "cli/message-audience.bclj"} (= 4 ^{:line 104 :file "cli/message-audience.bclj"} (count values)) ^{:line 105 :file "cli/message-audience.bclj"} (string? ^{:line 105 :file "cli/message-audience.bclj"} (nth values 0)) ^{:line 106 :file "cli/message-audience.bclj"} (string? ^{:line 106 :file "cli/message-audience.bclj"} (nth values 1)) ^{:line 107 :file "cli/message-audience.bclj"} (string? ^{:line 107 :file "cli/message-audience.bclj"} (nth values 2)) ^{:line 108 :file "cli/message-audience.bclj"} (string? ^{:line 108 :file "cli/message-audience.bclj"} (nth values 3)))))

^{:line 110 :file "cli/message-audience.bclj"} (defn online-handles
  "Finite session audience at one database observation. Liveness uses the\n   same unexpired renewable-lease rule as the presence roster." [port now]
  ^{:line 115 :file "cli/message-audience.bclj"} (:handles ^{:line 115 :file "cli/message-audience.bclj"} (coord/online-session-handles! port now)))

^{:line 117 :file "cli/message-audience.bclj"} (defn snapshot-broadcast!
  "Persist a finite audience before the wildcard `to` fact, excluding the sender. The caller\n   must publish `to` last so subscribers cannot observe a partial snapshot." [port ^String message from]
  ^{:line 123 :file "cli/message-audience.bclj"} (let [^String sender ^{:line 123 :file "cli/message-audience.bclj"} (bare-handle from)
   recipients ^{:line 124 :file "cli/message-audience.bclj"} (disj ^{:line 124 :file "cli/message-audience.bclj"} (online-handles port ^{:line 124 :file "cli/message-audience.bclj"} (System/currentTimeMillis)) sender)
   result ^{:line 125 :file "cli/message-audience.bclj"} (coord/publish! port ^{:line 127 :file "cli/message-audience.bclj"} [^{:line 127 :file "cli/message-audience.bclj"} {:op :set :subject message :predicate "broadcast_audience_version" :values ^{:line 129 :file "cli/message-audience.bclj"} [audience-version] :cardinality :one} ^{:line 130 :file "cli/message-audience.bclj"} {:op :set :subject message :predicate "broadcast_to" :values ^{:line 132 :file "cli/message-audience.bclj"} (vec recipients) :cardinality :many}])]
  ^{:line 133 :file "cli/message-audience.bclj"} (if ^{:line 133 :file "cli/message-audience.bclj"} (:reject result) ^{:line 133 :file "cli/message-audience.bclj"} (do
  ^{:line 134 :file "cli/message-audience.bclj"} (throw ^{:line 134 :file "cli/message-audience.bclj"} (ex-info "broadcast audience publication rejected" ^{:line 135 :file "cli/message-audience.bclj"} {:type :broadcast-audience-write-rejected :message message :result result}))))
  recipients))

^{:line 139 :file "cli/message-audience.bclj"} (defn audience [port ^String message]
  ^{:line 142 :file "cli/message-audience.bclj"} (set ^{:line 142 :file "cli/message-audience.bclj"} (coord/many! port message audience-predicate)))

^{:line 144 :file "cli/message-audience.bclj"} (defn- ^String sha256 [^String value]
  ^{:line 145 :file "cli/message-audience.bclj"} (let [digest ^{:line 145 :file "cli/message-audience.bclj"} (.digest ^{:line 145 :file "cli/message-audience.bclj"} (java.security.MessageDigest/getInstance "SHA-256") ^{:line 146 :file "cli/message-audience.bclj"} (.getBytes ^{:line 146 :file "cli/message-audience.bclj"} (str value) "UTF-8"))]
  ^{:line 147 :file "cli/message-audience.bclj"} (apply str ^{:line 147 :file "cli/message-audience.bclj"} (map ^{:line 147 :file "cli/message-audience.bclj"} (fn [%1] ^{:line 147 :file "cli/message-audience.bclj"} (format "%02x" ^{:line 147 :file "cli/message-audience.bclj"} (bit-and ^{:line 147 :file "cli/message-audience.bclj"} (int %1) 255))) digest))))

^{:line 149 :file "cli/message-audience.bclj"} (defn ^String delivery-claim-resource [^String message recipient]
  ^{:line 152 :file "cli/message-audience.bclj"} (str "message-delivery:" ^{:line 153 :file "cli/message-audience.bclj"} (sha256 ^{:line 153 :file "cli/message-audience.bclj"} (str message "\u0000" ^{:line 153 :file "cli/message-audience.bclj"} (bare-handle recipient)))))

^{:line 155 :file "cli/message-audience.bclj"} (defn ^Boolean acknowledged? [port ^String message recipient]
  ^{:line 159 :file "cli/message-audience.bclj"} (contains? ^{:line 159 :file "cli/message-audience.bclj"} (set ^{:line 159 :file "cli/message-audience.bclj"} (coord/many! port message "acked_by")) ^{:line 160 :file "cli/message-audience.bclj"} (bare-handle recipient)))

^{:line 162 :file "cli/message-audience.bclj"} (defn ^Boolean rejected? [port ^String message recipient]
  ^{:line 166 :file "cli/message-audience.bclj"} (contains? ^{:line 166 :file "cli/message-audience.bclj"} (set ^{:line 166 :file "cli/message-audience.bclj"} (coord/many! port message rejected-by-predicate)) ^{:line 167 :file "cli/message-audience.bclj"} (bare-handle recipient)))

^{:line 169 :file "cli/message-audience.bclj"} (defn release-delivery-claim!
  "Release after durable settlement. A transient release failure must not turn\n   successful output into a hook failure; the short lease expires naturally." [port {:keys [resource holder epoch]}]
  ^{:line 174 :file "cli/message-audience.bclj"} (try
  ^{:line 175 :file "cli/message-audience.bclj"} (coord/release-lease! port ^{:line 176 :file "cli/message-audience.bclj"} (coord/lease-fence resource holder epoch))
  (catch Exception _
    nil)))

^{:line 179 :file "cli/message-audience.bclj"} (defn claim-delivery!
  "Atomically elect one live consumer for MESSAGE/RECIPIENT. A short database\n   lease closes the listener-vs-hook query/ack race. It is released after ack;\n   if the winner dies first, expiry restores at-least-once delivery. Therefore\n   concurrent healthy consumers print once, while a crash after print but before\n   ack may still replay—the honest non-transactional-output boundary."
  ([port ^String message recipient]
    ^{:line 188 :file "cli/message-audience.bclj"} (claim-delivery! port message recipient delivery-claim-ttl-ms))
  ([port ^String message recipient ttl-ms]
    ^{:line 193 :file "cli/message-audience.bclj"} (if ^{:line 193 :file "cli/message-audience.bclj"} (not ^{:line 193 :file "cli/message-audience.bclj"} (and ^{:line 193 :file "cli/message-audience.bclj"} (integer? ttl-ms) ^{:line 194 :file "cli/message-audience.bclj"} (pos? ttl-ms) ^{:line 195 :file "cli/message-audience.bclj"} (<= ttl-ms delivery-claim-ttl-ms))) ^{:line 193 :file "cli/message-audience.bclj"} (do
  ^{:line 196 :file "cli/message-audience.bclj"} (throw ^{:line 196 :file "cli/message-audience.bclj"} (ex-info "delivery claim TTL is outside the supported bound" ^{:line 197 :file "cli/message-audience.bclj"} {:type :invalid-delivery-claim-ttl :ttl-ms ttl-ms :max-ttl-ms delivery-claim-ttl-ms}))))
    ^{:line 200 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 200 :file "cli/message-audience.bclj"} (bare-handle recipient)]
  ^{:line 201 :file "cli/message-audience.bclj"} (if ^{:line 201 :file "cli/message-audience.bclj"} (not ^{:line 201 :file "cli/message-audience.bclj"} (or ^{:line 201 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) ^{:line 202 :file "cli/message-audience.bclj"} (rejected? port message recipient))) ^{:line 201 :file "cli/message-audience.bclj"} (do
  ^{:line 203 :file "cli/message-audience.bclj"} (let [^String resource ^{:line 203 :file "cli/message-audience.bclj"} (delivery-claim-resource message recipient)
   ^String holder ^{:line 204 :file "cli/message-audience.bclj"} (str "message-consumer:" recipient ":" ^{:line 204 :file "cli/message-audience.bclj"} (java.util.UUID/randomUUID))
   result ^{:line 205 :file "cli/message-audience.bclj"} (coord/acquire-lease! port resource holder ttl-ms)]
  ^{:line 206 :file "cli/message-audience.bclj"} (if ^{:line 206 :file "cli/message-audience.bclj"} (:ok result) ^{:line 206 :file "cli/message-audience.bclj"} (do
  ^{:line 207 :file "cli/message-audience.bclj"} (let [claim ^{:line 207 :file "cli/message-audience.bclj"} (select-keys result ^{:line 207 :file "cli/message-audience.bclj"} [:resource :holder :epoch])]
  ^{:line 209 :file "cli/message-audience.bclj"} (if ^{:line 209 :file "cli/message-audience.bclj"} (or ^{:line 209 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) ^{:line 210 :file "cli/message-audience.bclj"} (rejected? port message recipient)) ^{:line 211 :file "cli/message-audience.bclj"} (do
  ^{:line 211 :file "cli/message-audience.bclj"} (release-delivery-claim! port claim)
  nil) claim))))))))))

^{:line 214 :file "cli/message-audience.bclj"} (defn ^Boolean complete-delivery!
  "Commit the durable ack after output has been flushed, then release CLAIM." [port ^String message recipient claim]
  ^{:line 220 :file "cli/message-audience.bclj"} (try
  ^{:line 221 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 221 :file "cli/message-audience.bclj"} (bare-handle recipient)
   outcome ^{:line 222 :file "cli/message-audience.bclj"} (coord/assert-batch-after-read! port message ^{:line 224 :file "cli/message-audience.bclj"} (fn [] ^{:line 225 :file "cli/message-audience.bclj"} (cond
  ^{:line 226 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 227 :file "cli/message-audience.bclj"} (throw ^{:line 228 :file "cli/message-audience.bclj"} (ex-info "message was already rejected for recipient" ^{:line 229 :file "cli/message-audience.bclj"} {:type :message-already-rejected :message message :recipient recipient}))
  ^{:line 233 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) ^{:line 234 :file "cli/message-audience.bclj"} {:done :already-acknowledged}
  :else ^{:line 237 :file "cli/message-audience.bclj"} {:facts ^{:line 238 :file "cli/message-audience.bclj"} [^{:line 238 :file "cli/message-audience.bclj"} {:p "acked_by" :r recipient :cardinality :many} ^{:line 239 :file "cli/message-audience.bclj"} {:p "acked_at" :r ^{:line 240 :file "cli/message-audience.bclj"} (str ^{:line 240 :file "cli/message-audience.bclj"} (java.time.Instant/now)) :cardinality :one}]})))]
  ^{:line 242 :file "cli/message-audience.bclj"} (if ^{:line 242 :file "cli/message-audience.bclj"} (:reject outcome) ^{:line 242 :file "cli/message-audience.bclj"} (do
  ^{:line 243 :file "cli/message-audience.bclj"} (throw ^{:line 243 :file "cli/message-audience.bclj"} (ex-info "message acknowledgement rejected" ^{:line 244 :file "cli/message-audience.bclj"} {:type :message-ack-rejected :message message :recipient recipient :result outcome}))))
  ^{:line 247 :file "cli/message-audience.bclj"} (if ^{:line 247 :file "cli/message-audience.bclj"} (not ^{:line 247 :file "cli/message-audience.bclj"} (acknowledged? port message recipient)) ^{:line 247 :file "cli/message-audience.bclj"} (do
  ^{:line 248 :file "cli/message-audience.bclj"} (throw ^{:line 248 :file "cli/message-audience.bclj"} (ex-info "message acknowledgement read-back mismatch" ^{:line 249 :file "cli/message-audience.bclj"} {:type :message-ack-readback-mismatch :message message :recipient recipient}))))
  true)
  (finally
    ^{:line 253 :file "cli/message-audience.bclj"} (release-delivery-claim! port claim))))

^{:line 255 :file "cli/message-audience.bclj"} (defn ^Boolean reject-delivery!
  "Terminally settle one permanently impossible recipient delivery without\n   claiming successful output. Evidence lands first; delivery_rejected_by is\n   the durable settlement marker that removes it from pending replay." [port ^String message recipient claim {:keys [reason expected-manifest observed-manifest]}]
  ^{:line 264 :file "cli/message-audience.bclj"} (try
  ^{:line 265 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 265 :file "cli/message-audience.bclj"} (bare-handle recipient)]
  ^{:line 266 :file "cli/message-audience.bclj"} (if ^{:line 266 :file "cli/message-audience.bclj"} (not ^{:line 266 :file "cli/message-audience.bclj"} (and ^{:line 266 :file "cli/message-audience.bclj"} (<= ^{:line 266 :file "cli/message-audience.bclj"} (utf8-bytes recipient) max-rejection-recipient-bytes) ^{:line 268 :file "cli/message-audience.bclj"} (boolean ^{:line 269 :file "cli/message-audience.bclj"} (re-matches #"^[A-Za-z0-9][A-Za-z0-9._:-]*$" recipient)))) ^{:line 266 :file "cli/message-audience.bclj"} (do
  ^{:line 271 :file "cli/message-audience.bclj"} (throw ^{:line 271 :file "cli/message-audience.bclj"} (ex-info "message rejection recipient is malformed" ^{:line 272 :file "cli/message-audience.bclj"} {:type :invalid-message-rejection}))))
  ^{:line 273 :file "cli/message-audience.bclj"} (if ^{:line 273 :file "cli/message-audience.bclj"} (not ^{:line 273 :file "cli/message-audience.bclj"} (contains? rejection-reasons reason)) ^{:line 273 :file "cli/message-audience.bclj"} (do
  ^{:line 274 :file "cli/message-audience.bclj"} (throw ^{:line 274 :file "cli/message-audience.bclj"} (ex-info "unsupported message rejection reason" ^{:line 275 :file "cli/message-audience.bclj"} {:type :invalid-message-rejection :reason reason}))))
  ^{:line 276 :file "cli/message-audience.bclj"} (doseq [[label value] ^{:line 276 :file "cli/message-audience.bclj"} [^{:line 276 :file "cli/message-audience.bclj"} ["expected manifest" expected-manifest] ^{:line 277 :file "cli/message-audience.bclj"} ["observed manifest" observed-manifest]]
   :when value]
  ^{:line 279 :file "cli/message-audience.bclj"} (if ^{:line 279 :file "cli/message-audience.bclj"} (not ^{:line 279 :file "cli/message-audience.bclj"} (and ^{:line 279 :file "cli/message-audience.bclj"} (string? value) ^{:line 280 :file "cli/message-audience.bclj"} (re-matches #"^[0-9a-f]{64}$" value))) ^{:line 279 :file "cli/message-audience.bclj"} (do
  ^{:line 281 :file "cli/message-audience.bclj"} (throw ^{:line 281 :file "cli/message-audience.bclj"} (ex-info ^{:line 281 :file "cli/message-audience.bclj"} (str label " is malformed") ^{:line 282 :file "cli/message-audience.bclj"} {:type :invalid-message-rejection :field label})))))
  ^{:line 284 :file "cli/message-audience.bclj"} (let [^String evidence ^{:line 284 :file "cli/message-audience.bclj"} (json/generate-string ^{:line 285 :file "cli/message-audience.bclj"} (cond-> ^{:line 285 :file "cli/message-audience.bclj"} (sorted-map "reason" reason "recipient" recipient) expected-manifest ^{:line 289 :file "cli/message-audience.bclj"} (assoc "expectedManifest" expected-manifest) observed-manifest ^{:line 291 :file "cli/message-audience.bclj"} (assoc "observedManifest" observed-manifest)))]
  ^{:line 292 :file "cli/message-audience.bclj"} (if ^{:line 292 :file "cli/message-audience.bclj"} (> ^{:line 292 :file "cli/message-audience.bclj"} (utf8-bytes evidence) max-rejection-evidence-bytes) ^{:line 292 :file "cli/message-audience.bclj"} (do
  ^{:line 293 :file "cli/message-audience.bclj"} (throw ^{:line 293 :file "cli/message-audience.bclj"} (ex-info "message rejection evidence exceeds its byte bound" ^{:line 294 :file "cli/message-audience.bclj"} {:type :invalid-message-rejection}))))
  ^{:line 295 :file "cli/message-audience.bclj"} (let [outcome ^{:line 295 :file "cli/message-audience.bclj"} (coord/assert-batch-after-read! port message ^{:line 297 :file "cli/message-audience.bclj"} (fn [] ^{:line 298 :file "cli/message-audience.bclj"} (let [evidence-values ^{:line 298 :file "cli/message-audience.bclj"} (set ^{:line 298 :file "cli/message-audience.bclj"} (coord/many! port message rejection-predicate))]
  ^{:line 300 :file "cli/message-audience.bclj"} (cond
  ^{:line 301 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) ^{:line 302 :file "cli/message-audience.bclj"} (throw ^{:line 303 :file "cli/message-audience.bclj"} (ex-info "message was already acknowledged by recipient" ^{:line 305 :file "cli/message-audience.bclj"} {:type :message-already-acknowledged :message message :recipient recipient}))
  ^{:line 309 :file "cli/message-audience.bclj"} (and ^{:line 309 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 310 :file "cli/message-audience.bclj"} (contains? evidence-values evidence)) ^{:line 311 :file "cli/message-audience.bclj"} {:done :already-rejected}
  ^{:line 313 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 314 :file "cli/message-audience.bclj"} (throw ^{:line 315 :file "cli/message-audience.bclj"} (ex-info "message already has different rejection evidence" ^{:line 317 :file "cli/message-audience.bclj"} {:type :message-rejection-conflict :message message :recipient recipient}))
  :else ^{:line 322 :file "cli/message-audience.bclj"} {:facts ^{:line 323 :file "cli/message-audience.bclj"} [^{:line 323 :file "cli/message-audience.bclj"} {:p rejection-predicate :r evidence :cardinality :many} ^{:line 325 :file "cli/message-audience.bclj"} {:p rejected-by-predicate :r recipient :cardinality :many}]}))))]
  ^{:line 327 :file "cli/message-audience.bclj"} (if ^{:line 327 :file "cli/message-audience.bclj"} (:reject outcome) ^{:line 327 :file "cli/message-audience.bclj"} (do
  ^{:line 328 :file "cli/message-audience.bclj"} (throw ^{:line 328 :file "cli/message-audience.bclj"} (ex-info "message rejection publication was rejected" ^{:line 329 :file "cli/message-audience.bclj"} {:type :message-rejection-write-rejected :message message :recipient recipient :result outcome})))))
  ^{:line 332 :file "cli/message-audience.bclj"} (if ^{:line 332 :file "cli/message-audience.bclj"} (not ^{:line 332 :file "cli/message-audience.bclj"} (and ^{:line 332 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 333 :file "cli/message-audience.bclj"} (contains? ^{:line 333 :file "cli/message-audience.bclj"} (set ^{:line 333 :file "cli/message-audience.bclj"} (coord/many! port message rejection-predicate)) evidence))) ^{:line 332 :file "cli/message-audience.bclj"} (do
  ^{:line 336 :file "cli/message-audience.bclj"} (throw ^{:line 336 :file "cli/message-audience.bclj"} (ex-info "message rejection read-back mismatch" ^{:line 337 :file "cli/message-audience.bclj"} {:type :message-rejection-readback-mismatch :message message :recipient recipient}))))
  true))
  (finally
    ^{:line 341 :file "cli/message-audience.bclj"} (release-delivery-claim! port claim))))

^{:line 343 :file "cli/message-audience.bclj"} (defn- ^Boolean safe-direct-address? [address]
  ^{:line 344 :file "cli/message-audience.bclj"} (and ^{:line 344 :file "cli/message-audience.bclj"} (string? address) ^{:line 345 :file "cli/message-audience.bclj"} (<= ^{:line 345 :file "cli/message-audience.bclj"} (utf8-bytes address) max-direct-address-bytes) ^{:line 346 :file "cli/message-audience.bclj"} (boolean ^{:line 347 :file "cli/message-audience.bclj"} (re-matches #"^[A-Za-z0-9][A-Za-z0-9._:-]*$" address))))

^{:line 349 :file "cli/message-audience.bclj"} (defn bounded-direct-addresses
  "Validate and deduplicate the finite direct audience without first\n   materializing an attacker-sized role collection." [recipient direct-addresses]
  ^{:line 354 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 354 :file "cli/message-audience.bclj"} (bare-handle recipient)]
  ^{:line 355 :file "cli/message-audience.bclj"} (if ^{:line 355 :file "cli/message-audience.bclj"} (not ^{:line 355 :file "cli/message-audience.bclj"} (safe-direct-address? recipient)) ^{:line 355 :file "cli/message-audience.bclj"} (do
  ^{:line 356 :file "cli/message-audience.bclj"} (throw ^{:line 356 :file "cli/message-audience.bclj"} (ex-info "message recipient is malformed" ^{:line 357 :file "cli/message-audience.bclj"} {:type :invalid-message-recipient}))))
  ^{:line 358 :file "cli/message-audience.bclj"} (loop [remaining ^{:line 358 :file "cli/message-audience.bclj"} (seq direct-addresses)
   addresses ^{:line 359 :file "cli/message-audience.bclj"} #{recipient}
   scanned 0]
  ^{:line 361 :file "cli/message-audience.bclj"} (if ^{:line 361 :file "cli/message-audience.bclj"} (nil? remaining) ^{:line 362 :file "cli/message-audience.bclj"} (vec ^{:line 362 :file "cli/message-audience.bclj"} (sort addresses)) ^{:line 363 :file "cli/message-audience.bclj"} (let [address ^{:line 363 :file "cli/message-audience.bclj"} (first remaining)]
  ^{:line 364 :file "cli/message-audience.bclj"} (if ^{:line 364 :file "cli/message-audience.bclj"} (>= scanned max-direct-addresses) ^{:line 364 :file "cli/message-audience.bclj"} (do
  ^{:line 365 :file "cli/message-audience.bclj"} (throw ^{:line 365 :file "cli/message-audience.bclj"} (ex-info "direct message address input exceeds its bound" ^{:line 366 :file "cli/message-audience.bclj"} {:type :direct-address-limit-exceeded :max max-direct-addresses}))))
  ^{:line 368 :file "cli/message-audience.bclj"} (if ^{:line 368 :file "cli/message-audience.bclj"} (not ^{:line 368 :file "cli/message-audience.bclj"} (safe-direct-address? address)) ^{:line 368 :file "cli/message-audience.bclj"} (do
  ^{:line 369 :file "cli/message-audience.bclj"} (throw ^{:line 369 :file "cli/message-audience.bclj"} (ex-info "direct message address is malformed" ^{:line 370 :file "cli/message-audience.bclj"} {:type :invalid-direct-address :address address}))))
  ^{:line 372 :file "cli/message-audience.bclj"} (let [next-addresses ^{:line 372 :file "cli/message-audience.bclj"} (conj addresses address)]
  ^{:line 373 :file "cli/message-audience.bclj"} (if ^{:line 373 :file "cli/message-audience.bclj"} (> ^{:line 373 :file "cli/message-audience.bclj"} (count next-addresses) max-direct-addresses) ^{:line 373 :file "cli/message-audience.bclj"} (do
  ^{:line 374 :file "cli/message-audience.bclj"} (throw ^{:line 374 :file "cli/message-audience.bclj"} (ex-info "direct message address set exceeds its bound" ^{:line 375 :file "cli/message-audience.bclj"} {:type :direct-address-limit-exceeded :max max-direct-addresses}))))
  ^{:line 377 :file "cli/message-audience.bclj"} (recur ^{:line 377 :file "cli/message-audience.bclj"} (next remaining) next-addresses ^{:line 377 :file "cli/message-audience.bclj"} (inc scanned))))))))

^{:line 379 :file "cli/message-audience.bclj"} (defn pending-query
  "One stratified program for direct + broadcast candidates minus durable ack\n   and rejection settlement. First-party attention entities are excluded before\n   bounded pagination, while malformed canonical mail remains visible for\n   terminal rejection. Dynamic direct-address rules are strictly bounded before\n   this data structure exists." [recipient direct-addresses]
  ^{:line 387 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 387 :file "cli/message-audience.bclj"} (bare-handle recipient)
   addresses ^{:line 388 :file "cli/message-audience.bclj"} (bounded-direct-addresses recipient direct-addresses)
   direct-rules ^{:line 389 :file "cli/message-audience.bclj"} (mapv ^{:line 390 :file "cli/message-audience.bclj"} (fn [^String address] ^{:line 391 :file "cli/message-audience.bclj"} {:head ^{:line 391 :file "cli/message-audience.bclj"} {:rel "message_candidate" :args ^{:line 391 :file "cli/message-audience.bclj"} [^{:line 391 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 392 :file "cli/message-audience.bclj"} [^{:line 392 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 393 :file "cli/message-audience.bclj"} [^{:line 393 :file "cli/message-audience.bclj"} {:var "e"} "to" address]}]}) addresses)
   base-rules ^{:line 395 :file "cli/message-audience.bclj"} (reduce conj direct-rules ^{:line 398 :file "cli/message-audience.bclj"} [^{:line 398 :file "cli/message-audience.bclj"} {:head ^{:line 398 :file "cli/message-audience.bclj"} {:rel "message_candidate" :args ^{:line 398 :file "cli/message-audience.bclj"} [^{:line 398 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 399 :file "cli/message-audience.bclj"} [^{:line 399 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 400 :file "cli/message-audience.bclj"} [^{:line 400 :file "cli/message-audience.bclj"} {:var "e"} "broadcast_to" recipient]} ^{:line 401 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 402 :file "cli/message-audience.bclj"} [^{:line 402 :file "cli/message-audience.bclj"} {:var "e"} "to" broadcast-address]}]} ^{:line 403 :file "cli/message-audience.bclj"} {:head ^{:line 403 :file "cli/message-audience.bclj"} {:rel "attention_entity" :args ^{:line 403 :file "cli/message-audience.bclj"} [^{:line 403 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 404 :file "cli/message-audience.bclj"} [^{:line 404 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 405 :file "cli/message-audience.bclj"} [^{:line 405 :file "cli/message-audience.bclj"} {:var "e"} "kind" "notification"]}]} ^{:line 406 :file "cli/message-audience.bclj"} {:head ^{:line 406 :file "cli/message-audience.bclj"} {:rel "attention_entity" :args ^{:line 406 :file "cli/message-audience.bclj"} [^{:line 406 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 407 :file "cli/message-audience.bclj"} [^{:line 407 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 408 :file "cli/message-audience.bclj"} [^{:line 408 :file "cli/message-audience.bclj"} {:var "e"} "kind" "subscription"]}]} ^{:line 409 :file "cli/message-audience.bclj"} {:head ^{:line 409 :file "cli/message-audience.bclj"} {:rel "message_acknowledged" :args ^{:line 409 :file "cli/message-audience.bclj"} [^{:line 409 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 410 :file "cli/message-audience.bclj"} [^{:line 410 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 411 :file "cli/message-audience.bclj"} [^{:line 411 :file "cli/message-audience.bclj"} {:var "e"} "acked_by" recipient]}]} ^{:line 412 :file "cli/message-audience.bclj"} {:head ^{:line 412 :file "cli/message-audience.bclj"} {:rel "message_rejected" :args ^{:line 412 :file "cli/message-audience.bclj"} [^{:line 412 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 413 :file "cli/message-audience.bclj"} [^{:line 413 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 414 :file "cli/message-audience.bclj"} [^{:line 414 :file "cli/message-audience.bclj"} {:var "e"} rejected-by-predicate recipient]}]}])]
  ^{:line 415 :file "cli/message-audience.bclj"} {:find "pending_message" :strata ^{:line 417 :file "cli/message-audience.bclj"} [base-rules ^{:line 418 :file "cli/message-audience.bclj"} [^{:line 418 :file "cli/message-audience.bclj"} {:head ^{:line 418 :file "cli/message-audience.bclj"} {:rel "pending_message" :args ^{:line 418 :file "cli/message-audience.bclj"} [^{:line 418 :file "cli/message-audience.bclj"} {:var "e"}]} :body ^{:line 419 :file "cli/message-audience.bclj"} [^{:line 419 :file "cli/message-audience.bclj"} {:rel "message_candidate" :args ^{:line 419 :file "cli/message-audience.bclj"} [^{:line 419 :file "cli/message-audience.bclj"} {:var "e"}]} ^{:line 420 :file "cli/message-audience.bclj"} {:rel "attention_entity" :args ^{:line 421 :file "cli/message-audience.bclj"} [^{:line 421 :file "cli/message-audience.bclj"} {:var "e"}] :neg true} ^{:line 422 :file "cli/message-audience.bclj"} {:rel "message_acknowledged" :args ^{:line 423 :file "cli/message-audience.bclj"} [^{:line 423 :file "cli/message-audience.bclj"} {:var "e"}] :neg true} ^{:line 424 :file "cli/message-audience.bclj"} {:rel "message_rejected" :args ^{:line 425 :file "cli/message-audience.bclj"} [^{:line 425 :file "cli/message-audience.bclj"} {:var "e"}] :neg true}]}]]}))

^{:line 427 :file "cli/message-audience.bclj"} (defn pending-msg-query
  "The pending relation restricted to messages admitted by the managed msg\n   producer. The immutable route-manifest fact is the producer's durable type\n   marker; filtering on it avoids terminal teardown being blocked by ordinary\n   inbox mail." [recipient direct-addresses]
  ^{:line 434 :file "cli/message-audience.bclj"} (update-in ^{:line 435 :file "cli/message-audience.bclj"} (pending-query recipient direct-addresses) ^{:line 436 :file "cli/message-audience.bclj"} [:strata 1 0 :body] conj ^{:line 438 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 439 :file "cli/message-audience.bclj"} [^{:line 439 :file "cli/message-audience.bclj"} {:var "e"} msg-manifest-predicate ^{:line 439 :file "cli/message-audience.bclj"} {:var "manifest"}]}))

^{:line 441 :file "cli/message-audience.bclj"} (defn- ^Boolean valid-message-row? [row]
  ^{:line 442 :file "cli/message-audience.bclj"} (and ^{:line 442 :file "cli/message-audience.bclj"} (vector? row) ^{:line 443 :file "cli/message-audience.bclj"} (= 1 ^{:line 443 :file "cli/message-audience.bclj"} (count row)) ^{:line 444 :file "cli/message-audience.bclj"} (string? ^{:line 444 :file "cli/message-audience.bclj"} (first row))))

^{:line 446 :file "cli/message-audience.bclj"} (defn- ^Boolean valid-message-rows? [rows]
  ^{:line 447 :file "cli/message-audience.bclj"} (reduce ^{:line 447 :file "cli/message-audience.bclj"} (fn [^Boolean valid row] ^{:line 450 :file "cli/message-audience.bclj"} (and valid ^{:line 450 :file "cli/message-audience.bclj"} (valid-message-row? row))) true rows))

^{:line 453 :file "cli/message-audience.bclj"} (defn pending-message-page
  "Read one bounded deterministic pending page. AFTER is a Beagle Store cursor for\n   stable read-only consumers; delivery replay intentionally restarts at nil\n   after settling each page."
  ([port recipient direct-addresses]
    ^{:line 460 :file "cli/message-audience.bclj"} (pending-message-page port recipient direct-addresses pending-page-limit nil))
  ([port recipient direct-addresses limit after]
    ^{:line 467 :file "cli/message-audience.bclj"} (let [response ^{:line 467 :file "cli/message-audience.bclj"} (coord/query-page! port ^{:line 468 :file "cli/message-audience.bclj"} (pending-query recipient direct-addresses) limit after)]
  ^{:line 469 :file "cli/message-audience.bclj"} (if ^{:line 469 :file "cli/message-audience.bclj"} (not ^{:line 469 :file "cli/message-audience.bclj"} (and ^{:line 469 :file "cli/message-audience.bclj"} (<= ^{:line 469 :file "cli/message-audience.bclj"} (count ^{:line 469 :file "cli/message-audience.bclj"} (:rows response)) limit) ^{:line 470 :file "cli/message-audience.bclj"} (valid-message-rows? ^{:line 470 :file "cli/message-audience.bclj"} (:rows response)))) ^{:line 469 :file "cli/message-audience.bclj"} (do
  ^{:line 471 :file "cli/message-audience.bclj"} (throw ^{:line 471 :file "cli/message-audience.bclj"} (ex-info "pending message page has malformed rows" ^{:line 472 :file "cli/message-audience.bclj"} {:type :malformed-pending-message-page}))))
  ^{:line 473 :file "cli/message-audience.bclj"} (let [rows ^{:line 473 :file "cli/message-audience.bclj"} (filterv ^{:line 474 :file "cli/message-audience.bclj"} (fn [row] ^{:line 475 :file "cli/message-audience.bclj"} (canonical-message-id? ^{:line 475 :file "cli/message-audience.bclj"} (first row))) ^{:line 476 :file "cli/message-audience.bclj"} (:rows response))]
  ^{:line 480 :file "cli/message-audience.bclj"} (assoc response :rows rows :messages ^{:line 480 :file "cli/message-audience.bclj"} (mapv first rows))))))

^{:line 482 :file "cli/message-audience.bclj"} (defn pending-msg-page
  "Read one bounded deterministic page of unsettled managed msg messages."
  ([port recipient direct-addresses]
    ^{:line 487 :file "cli/message-audience.bclj"} (pending-msg-page port recipient direct-addresses pending-page-limit nil))
  ([port recipient direct-addresses limit after]
    ^{:line 494 :file "cli/message-audience.bclj"} (let [response ^{:line 494 :file "cli/message-audience.bclj"} (coord/query-page! port ^{:line 495 :file "cli/message-audience.bclj"} (pending-msg-query recipient direct-addresses) limit after)]
  ^{:line 496 :file "cli/message-audience.bclj"} (if ^{:line 496 :file "cli/message-audience.bclj"} (not ^{:line 496 :file "cli/message-audience.bclj"} (and ^{:line 496 :file "cli/message-audience.bclj"} (<= ^{:line 496 :file "cli/message-audience.bclj"} (count ^{:line 496 :file "cli/message-audience.bclj"} (:rows response)) limit) ^{:line 497 :file "cli/message-audience.bclj"} (valid-message-rows? ^{:line 497 :file "cli/message-audience.bclj"} (:rows response)))) ^{:line 496 :file "cli/message-audience.bclj"} (do
  ^{:line 498 :file "cli/message-audience.bclj"} (throw ^{:line 498 :file "cli/message-audience.bclj"} (ex-info "pending msg page has malformed rows" ^{:line 499 :file "cli/message-audience.bclj"} {:type :malformed-pending-msg-page}))))
  ^{:line 500 :file "cli/message-audience.bclj"} (let [rows ^{:line 500 :file "cli/message-audience.bclj"} (filterv ^{:line 501 :file "cli/message-audience.bclj"} (fn [row] ^{:line 502 :file "cli/message-audience.bclj"} (canonical-message-id? ^{:line 502 :file "cli/message-audience.bclj"} (first row))) ^{:line 503 :file "cli/message-audience.bclj"} (:rows response))]
  ^{:line 504 :file "cli/message-audience.bclj"} (assoc response :rows rows :messages ^{:line 504 :file "cli/message-audience.bclj"} (mapv first rows))))))

^{:line 506 :file "cli/message-audience.bclj"} (defn- recipient-keyed-ids
  "Message ids from one bounded positive-triple query. Keeping negation in the\n   client-side set difference preserves the indexed join shape and bounds every\n   server response." [port body]
  ^{:line 512 :file "cli/message-audience.bclj"} (let [{:keys [rows]} ^{:line 512 :file "cli/message-audience.bclj"} (coord/bounded-query! port ^{:line 514 :file "cli/message-audience.bclj"} {:find "pending_candidate" :rules ^{:line 515 :file "cli/message-audience.bclj"} [^{:line 515 :file "cli/message-audience.bclj"} {:head ^{:line 515 :file "cli/message-audience.bclj"} {:rel "pending_candidate" :args ^{:line 515 :file "cli/message-audience.bclj"} [^{:line 515 :file "cli/message-audience.bclj"} {:var "e"}]} :body body}]} coord/query-page-row-limit)]
  ^{:line 518 :file "cli/message-audience.bclj"} (reduce ^{:line 518 :file "cli/message-audience.bclj"} (fn [ids row] ^{:line 521 :file "cli/message-audience.bclj"} (conj ids ^{:line 521 :file "cli/message-audience.bclj"} (first row))) ^{:line 522 :file "cli/message-audience.bclj"} #{} rows)))

^{:line 524 :file "cli/message-audience.bclj"} (defn pending-message-ids
  "All pending ids for human/read-only callers (the `msg inbox` view). Same set as\n   `pending-query`: direct + broadcast-audience candidates, minus durable ack and\n   rejection settlement — but computed from recipient-keyed index lookups so it\n   returns in O(recipient's mail), never the whole-corpus scan that stratified\n   negation forces. Live replay uses pending-message-page directly, not this vector." [port recipient direct-addresses]
  ^{:line 533 :file "cli/message-audience.bclj"} (let [^String recipient ^{:line 533 :file "cli/message-audience.bclj"} (bare-handle recipient)
   addresses ^{:line 534 :file "cli/message-audience.bclj"} (bounded-direct-addresses recipient direct-addresses)
   direct ^{:line 535 :file "cli/message-audience.bclj"} (reduce ^{:line 536 :file "cli/message-audience.bclj"} (fn [acc ^String address] ^{:line 539 :file "cli/message-audience.bclj"} (set/union acc ^{:line 540 :file "cli/message-audience.bclj"} (recipient-keyed-ids port ^{:line 541 :file "cli/message-audience.bclj"} [^{:line 541 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 541 :file "cli/message-audience.bclj"} [^{:line 541 :file "cli/message-audience.bclj"} {:var "e"} "to" address]}]))) ^{:line 542 :file "cli/message-audience.bclj"} #{} addresses)
   broadcast ^{:line 543 :file "cli/message-audience.bclj"} (recipient-keyed-ids port ^{:line 544 :file "cli/message-audience.bclj"} [^{:line 544 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 544 :file "cli/message-audience.bclj"} [^{:line 544 :file "cli/message-audience.bclj"} {:var "e"} audience-predicate recipient]} ^{:line 545 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 545 :file "cli/message-audience.bclj"} [^{:line 545 :file "cli/message-audience.bclj"} {:var "e"} "to" broadcast-address]}])
   acknowledged ^{:line 546 :file "cli/message-audience.bclj"} (recipient-keyed-ids port ^{:line 547 :file "cli/message-audience.bclj"} [^{:line 547 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 547 :file "cli/message-audience.bclj"} [^{:line 547 :file "cli/message-audience.bclj"} {:var "e"} "acked_by" recipient]}])
   rejected ^{:line 548 :file "cli/message-audience.bclj"} (recipient-keyed-ids port ^{:line 549 :file "cli/message-audience.bclj"} [^{:line 549 :file "cli/message-audience.bclj"} {:rel "triple" :args ^{:line 549 :file "cli/message-audience.bclj"} [^{:line 549 :file "cli/message-audience.bclj"} {:var "e"} rejected-by-predicate recipient]}])]
  ^{:line 550 :file "cli/message-audience.bclj"} (->> ^{:line 550 :file "cli/message-audience.bclj"} (set/difference ^{:line 550 :file "cli/message-audience.bclj"} (set/union direct broadcast) ^{:line 551 :file "cli/message-audience.bclj"} (set/union acknowledged rejected)) ^{:line 552 :file "cli/message-audience.bclj"} (filterv ^{:line 552 :file "cli/message-audience.bclj"} (fn [^String message] ^{:line 553 :file "cli/message-audience.bclj"} (complete-message-envelope? port message))) sort vec)))

^{:line 557 :file "cli/message-audience.bclj"} (defn ^Boolean deliverable?
  "Whether RECIPIENT may consume MESSAGE addressed TO. DIRECT-ADDRESSES contains\n   the recipient's own handle plus any roles it currently holds. Broadcasts\n   deliberately consult only the snapshotted concrete recipient handle." [port ^String message ^String to recipient direct-addresses]
  ^{:line 566 :file "cli/message-audience.bclj"} (and ^{:line 567 :file "cli/message-audience.bclj"} (canonical-message-id? message) ^{:line 568 :file "cli/message-audience.bclj"} (if ^{:line 568 :file "cli/message-audience.bclj"} (= broadcast-address to) ^{:line 569 :file "cli/message-audience.bclj"} (contains? ^{:line 569 :file "cli/message-audience.bclj"} (audience port message) ^{:line 569 :file "cli/message-audience.bclj"} (bare-handle recipient)) ^{:line 570 :file "cli/message-audience.bclj"} (contains? ^{:line 570 :file "cli/message-audience.bclj"} (set direct-addresses) to))))

^{:line 572 :file "cli/message-audience.bclj"} (defn ^String acknowledge-message!
  "Validate, authorize, and durably settle one manual message ACK. The CLI edge\n   supplies only argv-derived values; all message identity and audience\n   semantics remain here." [port raw-message recipient direct-addresses]
  ^{:line 580 :file "cli/message-audience.bclj"} (let [^String message ^{:line 580 :file "cli/message-audience.bclj"} (canonical-message-reference raw-message)
   ^String recipient ^{:line 581 :file "cli/message-audience.bclj"} (bare-handle recipient)]
  ^{:line 582 :file "cli/message-audience.bclj"} (if ^{:line 582 :file "cli/message-audience.bclj"} (not ^{:line 582 :file "cli/message-audience.bclj"} (complete-message-envelope? port message)) ^{:line 582 :file "cli/message-audience.bclj"} (do
  ^{:line 583 :file "cli/message-audience.bclj"} (throw ^{:line 583 :file "cli/message-audience.bclj"} (ex-info "message envelope is incomplete" ^{:line 584 :file "cli/message-audience.bclj"} {:type :incomplete-message-envelope :message message}))))
  ^{:line 586 :file "cli/message-audience.bclj"} (if ^{:line 586 :file "cli/message-audience.bclj"} (not ^{:line 586 :file "cli/message-audience.bclj"} (deliverable? port message ^{:line 587 :file "cli/message-audience.bclj"} (coord/resolved! port message "to") recipient direct-addresses)) ^{:line 586 :file "cli/message-audience.bclj"} (do
  ^{:line 589 :file "cli/message-audience.bclj"} (throw ^{:line 589 :file "cli/message-audience.bclj"} (ex-info "message is not addressed to recipient" ^{:line 590 :file "cli/message-audience.bclj"} {:type :message-not-addressed :message message :recipient recipient}))))
  ^{:line 593 :file "cli/message-audience.bclj"} (cond
  ^{:line 594 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) message
  ^{:line 597 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 598 :file "cli/message-audience.bclj"} (throw ^{:line 598 :file "cli/message-audience.bclj"} (ex-info "message was already rejected for recipient" ^{:line 599 :file "cli/message-audience.bclj"} {:type :message-already-rejected :message message :recipient recipient}))
  :else ^{:line 604 :file "cli/message-audience.bclj"} (let [claim ^{:line 604 :file "cli/message-audience.bclj"} (claim-delivery! port message recipient)]
  ^{:line 605 :file "cli/message-audience.bclj"} (if ^{:line 605 :file "cli/message-audience.bclj"} (some? claim) ^{:line 606 :file "cli/message-audience.bclj"} (do
  ^{:line 607 :file "cli/message-audience.bclj"} (complete-delivery! port message recipient claim)
  message) ^{:line 609 :file "cli/message-audience.bclj"} (cond
  ^{:line 610 :file "cli/message-audience.bclj"} (acknowledged? port message recipient) message
  ^{:line 613 :file "cli/message-audience.bclj"} (rejected? port message recipient) ^{:line 614 :file "cli/message-audience.bclj"} (throw ^{:line 614 :file "cli/message-audience.bclj"} (ex-info "message was already rejected for recipient" ^{:line 615 :file "cli/message-audience.bclj"} {:type :message-already-rejected :message message :recipient recipient}))
  :else ^{:line 620 :file "cli/message-audience.bclj"} (throw ^{:line 620 :file "cli/message-audience.bclj"} (ex-info "message settlement is already in progress" ^{:line 621 :file "cli/message-audience.bclj"} {:type :message-settlement-in-progress :message message :recipient recipient}))))))))
