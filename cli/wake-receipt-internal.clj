(ns north.wake-receipt-internal
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

^{:line 12 :file "cli/wake-receipt-internal.bclj"} (def script-file ^{:line 12 :file "cli/wake-receipt-internal.bclj"} (.getCanonicalPath ^{:line 12 :file "cli/wake-receipt-internal.bclj"} (io/file *file*)))

^{:line 13 :file "cli/wake-receipt-internal.bclj"} (def cli-dir ^{:line 13 :file "cli/wake-receipt-internal.bclj"} (.getParent ^{:line 13 :file "cli/wake-receipt-internal.bclj"} (io/file script-file)))

^{:line 14 :file "cli/wake-receipt-internal.bclj"} (load-file ^{:line 14 :file "cli/wake-receipt-internal.bclj"} (str cli-dir "/coord.clj"))

^{:line 15 :file "cli/wake-receipt-internal.bclj"} (load-file ^{:line 15 :file "cli/wake-receipt-internal.bclj"} (str cli-dir "/run-ledger.clj"))

^{:line 17 :file "cli/wake-receipt-internal.bclj"} (def attempt-pattern #"^wake:[0-9a-f]{64}$")

^{:line 18 :file "cli/wake-receipt-internal.bclj"} (def message-pattern #"^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$")

^{:line 19 :file "cli/wake-receipt-internal.bclj"} (def target-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

^{:line 20 :file "cli/wake-receipt-internal.bclj"} (def wire-id-pattern #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$")

^{:line 21 :file "cli/wake-receipt-internal.bclj"} (def epoch-pattern #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

^{:line 24 :file "cli/wake-receipt-internal.bclj"} (def idle-predicates ^{:line 25 :file "cli/wake-receipt-internal.bclj"} ["wake_message_admission_version" "wake_message_admission_ordinal" "wake_idle_event" "wake_idle_run_id" "wake_idle_sequence" "wake_idle_commit_version" "wake_idle_model_call_id"])

^{:line 28 :file "cli/wake-receipt-internal.bclj"} (def turn-predicates ^{:line 29 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_event" "wake_turn_run_id" "wake_turn_sequence" "wake_turn_commit_version" "wake_turn_model_call_id"])

^{:line 31 :file "cli/wake-receipt-internal.bclj"} (def action-predicates ^{:line 32 :file "cli/wake-receipt-internal.bclj"} ["wake_first_action_event" "wake_first_action_kind" "wake_first_action_sequence"])

^{:line 35 :file "cli/wake-receipt-internal.bclj"} (defn- fail! [^String message]
  ^{:line 36 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 37 :file "cli/wake-receipt-internal.bclj"} (binding [*out* *err*]
  ^{:line 38 :file "cli/wake-receipt-internal.bclj"} (println ^{:line 38 :file "cli/wake-receipt-internal.bclj"} (str "wake receipt rejected: " message)))
  ^{:line 39 :file "cli/wake-receipt-internal.bclj"} (throw ^{:line 39 :file "cli/wake-receipt-internal.bclj"} (ex-info message ^{:line 39 :file "cli/wake-receipt-internal.bclj"} {}))))

^{:line 41 :file "cli/wake-receipt-internal.bclj"} (defn- coord-invoke! [^String operation args]
  ^{:line 44 :file "cli/wake-receipt-internal.bclj"} (let [callable ^{:line 44 :file "cli/wake-receipt-internal.bclj"} (clojure.core/ns-resolve ^{:line 44 :file "cli/wake-receipt-internal.bclj"} (symbol "north.coord") ^{:line 44 :file "cli/wake-receipt-internal.bclj"} (symbol operation))]
  ^{:line 45 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 45 :file "cli/wake-receipt-internal.bclj"} (some? callable) ^{:line 46 :file "cli/wake-receipt-internal.bclj"} (apply callable args) ^{:line 47 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 47 :file "cli/wake-receipt-internal.bclj"} (str "north.coord/" operation " is unavailable")))))

^{:line 49 :file "cli/wake-receipt-internal.bclj"} (defn- ledger-invoke! [^String operation args]
  ^{:line 52 :file "cli/wake-receipt-internal.bclj"} (let [callable ^{:line 52 :file "cli/wake-receipt-internal.bclj"} (clojure.core/ns-resolve ^{:line 52 :file "cli/wake-receipt-internal.bclj"} (symbol "north.run-ledger") ^{:line 53 :file "cli/wake-receipt-internal.bclj"} (symbol operation))]
  ^{:line 54 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 54 :file "cli/wake-receipt-internal.bclj"} (some? callable) ^{:line 55 :file "cli/wake-receipt-internal.bclj"} (apply callable args) ^{:line 56 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 56 :file "cli/wake-receipt-internal.bclj"} (str "north.run-ledger/" operation " is unavailable")))))

^{:line 58 :file "cli/wake-receipt-internal.bclj"} (defn- ^Boolean exact-singleton? [envelope ^String expected]
  ^{:line 61 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 61 :file "cli/wake-receipt-internal.bclj"} (= 1 ^{:line 61 :file "cli/wake-receipt-internal.bclj"} (:members envelope)) ^{:line 62 :file "cli/wake-receipt-internal.bclj"} (false? ^{:line 62 :file "cli/wake-receipt-internal.bclj"} (:ambiguous? envelope)) ^{:line 63 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 63 :file "cli/wake-receipt-internal.bclj"} [expected] ^{:line 63 :file "cli/wake-receipt-internal.bclj"} (:values envelope)) ^{:line 64 :file "cli/wake-receipt-internal.bclj"} (= expected ^{:line 64 :file "cli/wake-receipt-internal.bclj"} (:value envelope))))

^{:line 66 :file "cli/wake-receipt-internal.bclj"} (defn- ^Boolean exact-envelope? [envelope]
  ^{:line 67 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 67 :file "cli/wake-receipt-internal.bclj"} (= 1 ^{:line 67 :file "cli/wake-receipt-internal.bclj"} (:members envelope)) ^{:line 68 :file "cli/wake-receipt-internal.bclj"} (false? ^{:line 68 :file "cli/wake-receipt-internal.bclj"} (:ambiguous? envelope)) ^{:line 69 :file "cli/wake-receipt-internal.bclj"} (= 1 ^{:line 69 :file "cli/wake-receipt-internal.bclj"} (count ^{:line 69 :file "cli/wake-receipt-internal.bclj"} (:values envelope))) ^{:line 70 :file "cli/wake-receipt-internal.bclj"} (string? ^{:line 70 :file "cli/wake-receipt-internal.bclj"} (:value envelope)) ^{:line 71 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 71 :file "cli/wake-receipt-internal.bclj"} [^{:line 71 :file "cli/wake-receipt-internal.bclj"} (:value envelope)] ^{:line 71 :file "cli/wake-receipt-internal.bclj"} (:values envelope))))

^{:line 73 :file "cli/wake-receipt-internal.bclj"} (defn- ^Boolean absent? [envelope]
  ^{:line 74 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 74 :file "cli/wake-receipt-internal.bclj"} (= 0 ^{:line 74 :file "cli/wake-receipt-internal.bclj"} (:members envelope)) ^{:line 75 :file "cli/wake-receipt-internal.bclj"} (false? ^{:line 75 :file "cli/wake-receipt-internal.bclj"} (:ambiguous? envelope)) ^{:line 76 :file "cli/wake-receipt-internal.bclj"} (empty? ^{:line 76 :file "cli/wake-receipt-internal.bclj"} (:values envelope)) ^{:line 77 :file "cli/wake-receipt-internal.bclj"} (nil? ^{:line 77 :file "cli/wake-receipt-internal.bclj"} (:value envelope))))

^{:line 79 :file "cli/wake-receipt-internal.bclj"} (defn- envelope! [port ^String subject ^String predicate]
  ^{:line 83 :file "cli/wake-receipt-internal.bclj"} (coord-invoke! "resolved-envelope!" ^{:line 83 :file "cli/wake-receipt-internal.bclj"} [port subject predicate]))

^{:line 85 :file "cli/wake-receipt-internal.bclj"} (defn- ^String require-exact! [port ^String subject ^String predicate ^String expected]
  ^{:line 90 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 90 :file "cli/wake-receipt-internal.bclj"} (exact-singleton? ^{:line 90 :file "cli/wake-receipt-internal.bclj"} (envelope! port subject predicate) expected) expected ^{:line 92 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 92 :file "cli/wake-receipt-internal.bclj"} (str predicate " does not match the committed wake identity"))))

^{:line 94 :file "cli/wake-receipt-internal.bclj"} (defn- ^String exact-value! [port ^String subject ^String predicate]
  ^{:line 98 :file "cli/wake-receipt-internal.bclj"} (let [envelope ^{:line 98 :file "cli/wake-receipt-internal.bclj"} (envelope! port subject predicate)]
  ^{:line 99 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 99 :file "cli/wake-receipt-internal.bclj"} (exact-envelope? envelope) ^{:line 100 :file "cli/wake-receipt-internal.bclj"} (:value envelope) ^{:line 101 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 101 :file "cli/wake-receipt-internal.bclj"} (str predicate " is not an exact singleton")))))

^{:line 103 :file "cli/wake-receipt-internal.bclj"} (defn- historic-message-context! [port ^String message]
  ^{:line 106 :file "cli/wake-receipt-internal.bclj"} (let [^String attempt ^{:line 106 :file "cli/wake-receipt-internal.bclj"} (exact-value! port message "wake_attempt_id")
   ^String target ^{:line 107 :file "cli/wake-receipt-internal.bclj"} (exact-value! port message "to")
   ^String epoch ^{:line 108 :file "cli/wake-receipt-internal.bclj"} (exact-value! port message "wake_listener_epoch")
   ^String manifest ^{:line 109 :file "cli/wake-receipt-internal.bclj"} (exact-value! port message "wake_listener_manifest_sha256")]
  ^{:line 110 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 110 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 110 :file "cli/wake-receipt-internal.bclj"} (some? ^{:line 110 :file "cli/wake-receipt-internal.bclj"} (re-matches message-pattern message)) ^{:line 111 :file "cli/wake-receipt-internal.bclj"} (some? ^{:line 111 :file "cli/wake-receipt-internal.bclj"} (re-matches attempt-pattern attempt)) ^{:line 112 :file "cli/wake-receipt-internal.bclj"} (some? ^{:line 112 :file "cli/wake-receipt-internal.bclj"} (re-matches target-pattern target)) ^{:line 113 :file "cli/wake-receipt-internal.bclj"} (some? ^{:line 113 :file "cli/wake-receipt-internal.bclj"} (re-matches epoch-pattern epoch))) ^{:line 115 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 116 :file "cli/wake-receipt-internal.bclj"} (require-exact! port message "target_identity_manifest_sha256" manifest)
  ^{:line 117 :file "cli/wake-receipt-internal.bclj"} {:message message :attempt attempt :target target :epoch epoch :manifest manifest}) ^{:line 114 :file "cli/wake-receipt-internal.bclj"} (fail! "wake identity is malformed"))))

^{:line 123 :file "cli/wake-receipt-internal.bclj"} (defn- current-message-context! [port ^String message ^String attempt ^String target ^String epoch]
  ^{:line 129 :file "cli/wake-receipt-internal.bclj"} (let [context ^{:line 129 :file "cli/wake-receipt-internal.bclj"} (historic-message-context! port message)
   ^String agent ^{:line 130 :file "cli/wake-receipt-internal.bclj"} (str "@agent:" target)]
  ^{:line 131 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (= attempt ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (:attempt context)) ^{:line 133 :file "cli/wake-receipt-internal.bclj"} (= target ^{:line 133 :file "cli/wake-receipt-internal.bclj"} (:target context)) ^{:line 134 :file "cli/wake-receipt-internal.bclj"} (= epoch ^{:line 134 :file "cli/wake-receipt-internal.bclj"} (:epoch context)))) ^{:line 132 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 135 :file "cli/wake-receipt-internal.bclj"} (fail! "caller identity does not match the committed wake identity")))
  ^{:line 136 :file "cli/wake-receipt-internal.bclj"} (require-exact! port agent "live_input" "turn-messages")
  ^{:line 137 :file "cli/wake-receipt-internal.bclj"} (require-exact! port agent "live_input_state" "armed")
  ^{:line 138 :file "cli/wake-receipt-internal.bclj"} (require-exact! port agent "live_input_epoch" epoch)
  ^{:line 139 :file "cli/wake-receipt-internal.bclj"} (require-exact! port agent "identity_manifest_sha256" ^{:line 139 :file "cli/wake-receipt-internal.bclj"} (:manifest context))
  context)))

^{:line 142 :file "cli/wake-receipt-internal.bclj"} (defn- query-rows! [port query]
  ^{:line 145 :file "cli/wake-receipt-internal.bclj"} (coord-invoke! "query-rows!" ^{:line 145 :file "cli/wake-receipt-internal.bclj"} [port query]))

^{:line 147 :file "cli/wake-receipt-internal.bclj"} (defn- exact-assertion-boundary! [port ^String subject ^String predicate ^String value]
  ^{:line 152 :file "cli/wake-receipt-internal.bclj"} (let [occurrences ^{:line 152 :file "cli/wake-receipt-internal.bclj"} (coord-invoke! "proposition-occurrences!" ^{:line 154 :file "cli/wake-receipt-internal.bclj"} [port subject predicate value])
   occurrence ^{:line 155 :file "cli/wake-receipt-internal.bclj"} (first occurrences)]
  ^{:line 156 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 156 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 156 :file "cli/wake-receipt-internal.bclj"} (= 1 ^{:line 156 :file "cli/wake-receipt-internal.bclj"} (count occurrences)) ^{:line 157 :file "cli/wake-receipt-internal.bclj"} (= :assert ^{:line 157 :file "cli/wake-receipt-internal.bclj"} (:operation occurrence)) ^{:line 158 :file "cli/wake-receipt-internal.bclj"} (integer? ^{:line 158 :file "cli/wake-receipt-internal.bclj"} (:version occurrence)) ^{:line 159 :file "cli/wake-receipt-internal.bclj"} (pos? ^{:line 159 :file "cli/wake-receipt-internal.bclj"} (:version occurrence)) ^{:line 160 :file "cli/wake-receipt-internal.bclj"} (integer? ^{:line 160 :file "cli/wake-receipt-internal.bclj"} (:ordinal occurrence)) ^{:line 161 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 161 :file "cli/wake-receipt-internal.bclj"} (neg? ^{:line 161 :file "cli/wake-receipt-internal.bclj"} (:ordinal occurrence)))) occurrence ^{:line 163 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 163 :file "cli/wake-receipt-internal.bclj"} (str predicate " does not have one exact assertion occurrence")))))

^{:line 165 :file "cli/wake-receipt-internal.bclj"} (defn- message-boundary! [port context]
  ^{:line 168 :file "cli/wake-receipt-internal.bclj"} (exact-assertion-boundary! port ^{:line 169 :file "cli/wake-receipt-internal.bclj"} (:message context) "to" ^{:line 169 :file "cli/wake-receipt-internal.bclj"} (:target context)))

^{:line 171 :file "cli/wake-receipt-internal.bclj"} (defn- event-subjects! [port ^String event-id]
  ^{:line 174 :file "cli/wake-receipt-internal.bclj"} (->> ^{:line 174 :file "cli/wake-receipt-internal.bclj"} (query-rows! port ^{:line 176 :file "cli/wake-receipt-internal.bclj"} {:find "wake_wire_event_subject" :rules ^{:line 177 :file "cli/wake-receipt-internal.bclj"} [^{:line 177 :file "cli/wake-receipt-internal.bclj"} {:head ^{:line 177 :file "cli/wake-receipt-internal.bclj"} {:rel "wake_wire_event_subject" :args ^{:line 177 :file "cli/wake-receipt-internal.bclj"} [^{:line 177 :file "cli/wake-receipt-internal.bclj"} {:var "subject"}]} :body ^{:line 178 :file "cli/wake-receipt-internal.bclj"} [^{:line 178 :file "cli/wake-receipt-internal.bclj"} {:rel "triple" :args ^{:line 179 :file "cli/wake-receipt-internal.bclj"} [^{:line 179 :file "cli/wake-receipt-internal.bclj"} {:var "subject"} "wire_event_id" event-id]}]}]}) ^{:line 180 :file "cli/wake-receipt-internal.bclj"} (map first) distinct sort vec))

^{:line 185 :file "cli/wake-receipt-internal.bclj"} (defn- event-facts! [port ^String subject]
  ^{:line 188 :file "cli/wake-receipt-internal.bclj"} (query-rows! port ^{:line 190 :file "cli/wake-receipt-internal.bclj"} {:find "wake_wire_event_fact" :rules ^{:line 191 :file "cli/wake-receipt-internal.bclj"} [^{:line 191 :file "cli/wake-receipt-internal.bclj"} {:head ^{:line 191 :file "cli/wake-receipt-internal.bclj"} {:rel "wake_wire_event_fact" :args ^{:line 192 :file "cli/wake-receipt-internal.bclj"} [^{:line 192 :file "cli/wake-receipt-internal.bclj"} {:var "predicate"} ^{:line 192 :file "cli/wake-receipt-internal.bclj"} {:var "value"}]} :body ^{:line 193 :file "cli/wake-receipt-internal.bclj"} [^{:line 193 :file "cli/wake-receipt-internal.bclj"} {:rel "triple" :args ^{:line 194 :file "cli/wake-receipt-internal.bclj"} [subject ^{:line 194 :file "cli/wake-receipt-internal.bclj"} {:var "predicate"} ^{:line 194 :file "cli/wake-receipt-internal.bclj"} {:var "value"}]}]}]}))

^{:line 196 :file "cli/wake-receipt-internal.bclj"} (defn- durable-event-subject! [port ^String target ^String subject ^String event-id]
  ^{:line 201 :file "cli/wake-receipt-internal.bclj"} (let [event ^{:line 201 :file "cli/wake-receipt-internal.bclj"} (ledger-invoke! "validate-event-facts!" ^{:line 202 :file "cli/wake-receipt-internal.bclj"} [subject ^{:line 202 :file "cli/wake-receipt-internal.bclj"} (event-facts! port subject)])
   commit ^{:line 203 :file "cli/wake-receipt-internal.bclj"} (exact-assertion-boundary! port subject "wire_event_id" event-id)]
  ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (get event "id")) ^{:line 206 :file "cli/wake-receipt-internal.bclj"} (= target ^{:line 206 :file "cli/wake-receipt-internal.bclj"} (get event "agent")))) ^{:line 205 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 207 :file "cli/wake-receipt-internal.bclj"} (fail! "Wire event does not belong to the exact wake target")))
  ^{:line 208 :file "cli/wake-receipt-internal.bclj"} (assoc event :commit-version ^{:line 208 :file "cli/wake-receipt-internal.bclj"} (:version commit))))

^{:line 210 :file "cli/wake-receipt-internal.bclj"} (defn- durable-event! [port ^String target ^String event-id]
  ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (string? event-id) ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (re-matches wire-id-pattern event-id))) ^{:line 214 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 215 :file "cli/wake-receipt-internal.bclj"} (fail! "Wire event id is malformed")))
  ^{:line 216 :file "cli/wake-receipt-internal.bclj"} (let [subjects ^{:line 216 :file "cli/wake-receipt-internal.bclj"} (event-subjects! port event-id)]
  ^{:line 217 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 217 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 217 :file "cli/wake-receipt-internal.bclj"} (= 1 ^{:line 217 :file "cli/wake-receipt-internal.bclj"} (count subjects))) ^{:line 217 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 218 :file "cli/wake-receipt-internal.bclj"} (fail! "Wire event id is absent or ambiguous")))
  ^{:line 219 :file "cli/wake-receipt-internal.bclj"} (durable-event-subject! port target ^{:line 219 :file "cli/wake-receipt-internal.bclj"} (first subjects) event-id)))

^{:line 221 :file "cli/wake-receipt-internal.bclj"} (defn- run-events! [port ^String target ^String run]
  ^{:line 225 :file "cli/wake-receipt-internal.bclj"} (let [subjects ^{:line 225 :file "cli/wake-receipt-internal.bclj"} (->> ^{:line 225 :file "cli/wake-receipt-internal.bclj"} (query-rows! port ^{:line 227 :file "cli/wake-receipt-internal.bclj"} {:find "wake_wire_run_subject" :rules ^{:line 228 :file "cli/wake-receipt-internal.bclj"} [^{:line 228 :file "cli/wake-receipt-internal.bclj"} {:head ^{:line 228 :file "cli/wake-receipt-internal.bclj"} {:rel "wake_wire_run_subject" :args ^{:line 229 :file "cli/wake-receipt-internal.bclj"} [^{:line 229 :file "cli/wake-receipt-internal.bclj"} {:var "subject"}]} :body ^{:line 230 :file "cli/wake-receipt-internal.bclj"} [^{:line 230 :file "cli/wake-receipt-internal.bclj"} {:rel "triple" :args ^{:line 231 :file "cli/wake-receipt-internal.bclj"} [^{:line 231 :file "cli/wake-receipt-internal.bclj"} {:var "subject"} "wire_run_id" run]} ^{:line 232 :file "cli/wake-receipt-internal.bclj"} {:rel "triple" :args ^{:line 233 :file "cli/wake-receipt-internal.bclj"} [^{:line 233 :file "cli/wake-receipt-internal.bclj"} {:var "subject"} "agent" target]}]}]}) ^{:line 234 :file "cli/wake-receipt-internal.bclj"} (map first) distinct sort vec)
   events ^{:line 238 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 238 :file "cli/wake-receipt-internal.bclj"} (fn [subject] ^{:line 239 :file "cli/wake-receipt-internal.bclj"} (let [facts ^{:line 239 :file "cli/wake-receipt-internal.bclj"} (event-facts! port subject)
   event ^{:line 240 :file "cli/wake-receipt-internal.bclj"} (ledger-invoke! "validate-event-facts!" ^{:line 241 :file "cli/wake-receipt-internal.bclj"} [subject facts])]
  ^{:line 242 :file "cli/wake-receipt-internal.bclj"} (durable-event-subject! port target subject ^{:line 243 :file "cli/wake-receipt-internal.bclj"} (get event "id")))) subjects)
   ordered ^{:line 245 :file "cli/wake-receipt-internal.bclj"} (vec ^{:line 245 :file "cli/wake-receipt-internal.bclj"} (sort-by ^{:line 245 :file "cli/wake-receipt-internal.bclj"} (fn [event] ^{:line 245 :file "cli/wake-receipt-internal.bclj"} (get event "sequence")) events))]
  ^{:line 246 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 246 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 246 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 246 :file "cli/wake-receipt-internal.bclj"} (count ordered) ^{:line 247 :file "cli/wake-receipt-internal.bclj"} (count ^{:line 247 :file "cli/wake-receipt-internal.bclj"} (set ^{:line 247 :file "cli/wake-receipt-internal.bclj"} (map ^{:line 247 :file "cli/wake-receipt-internal.bclj"} (fn [event] ^{:line 247 :file "cli/wake-receipt-internal.bclj"} (get event "sequence")) ordered))))) ^{:line 246 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 249 :file "cli/wake-receipt-internal.bclj"} (fail! "Wire run contains an ambiguous event sequence")))
  ordered))

^{:line 252 :file "cli/wake-receipt-internal.bclj"} (defn- payload [event]
  ^{:line 252 :file "cli/wake-receipt-internal.bclj"} (get event "event"))

^{:line 253 :file "cli/wake-receipt-internal.bclj"} (defn- model-call-id [event]
  ^{:line 253 :file "cli/wake-receipt-internal.bclj"} (get ^{:line 253 :file "cli/wake-receipt-internal.bclj"} (payload event) "modelCallId"))

^{:line 255 :file "cli/wake-receipt-internal.bclj"} (defn- idle-evidence! [port ^String target boundary ^String event-id]
  ^{:line 260 :file "cli/wake-receipt-internal.bclj"} (let [event ^{:line 260 :file "cli/wake-receipt-internal.bclj"} (durable-event! port target event-id)
   ^String run ^{:line 261 :file "cli/wake-receipt-internal.bclj"} (get event "run")
   candidates ^{:line 262 :file "cli/wake-receipt-internal.bclj"} (->> ^{:line 262 :file "cli/wake-receipt-internal.bclj"} (run-events! port target run) ^{:line 263 :file "cli/wake-receipt-internal.bclj"} (filter ^{:line 263 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 264 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 264 :file "cli/wake-receipt-internal.bclj"} (= "model-call.completed" ^{:line 264 :file "cli/wake-receipt-internal.bclj"} (get candidate "kind")) ^{:line 265 :file "cli/wake-receipt-internal.bclj"} (< ^{:line 265 :file "cli/wake-receipt-internal.bclj"} (:commit-version candidate) ^{:line 265 :file "cli/wake-receipt-internal.bclj"} (:version boundary))))) ^{:line 266 :file "cli/wake-receipt-internal.bclj"} (sort-by ^{:line 266 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 267 :file "cli/wake-receipt-internal.bclj"} [^{:line 267 :file "cli/wake-receipt-internal.bclj"} (:commit-version candidate) ^{:line 267 :file "cli/wake-receipt-internal.bclj"} (get candidate "sequence")])) vec)
   latest ^{:line 269 :file "cli/wake-receipt-internal.bclj"} (last candidates)
   model-call ^{:line 270 :file "cli/wake-receipt-internal.bclj"} (model-call-id event)]
  ^{:line 271 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 271 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 271 :file "cli/wake-receipt-internal.bclj"} (and latest ^{:line 272 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 272 :file "cli/wake-receipt-internal.bclj"} (get latest "id")) ^{:line 273 :file "cli/wake-receipt-internal.bclj"} (= "model-call.completed" ^{:line 273 :file "cli/wake-receipt-internal.bclj"} (get event "kind")) ^{:line 274 :file "cli/wake-receipt-internal.bclj"} (< ^{:line 274 :file "cli/wake-receipt-internal.bclj"} (:commit-version event) ^{:line 274 :file "cli/wake-receipt-internal.bclj"} (:version boundary)) ^{:line 275 :file "cli/wake-receipt-internal.bclj"} (string? model-call) ^{:line 276 :file "cli/wake-receipt-internal.bclj"} (re-matches wire-id-pattern model-call))) ^{:line 271 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 277 :file "cli/wake-receipt-internal.bclj"} (fail! "idle evidence is not the latest exact completion before message admission")))
  ^{:line 278 :file "cli/wake-receipt-internal.bclj"} {:event event-id :run run :sequence ^{:line 280 :file "cli/wake-receipt-internal.bclj"} (get event "sequence") :commit-version ^{:line 281 :file "cli/wake-receipt-internal.bclj"} (:commit-version event) :model-call model-call}))

^{:line 284 :file "cli/wake-receipt-internal.bclj"} (defn- first-turn-after-idle! [port ^String target boundary idle ^String event-id]
  ^{:line 290 :file "cli/wake-receipt-internal.bclj"} (let [event ^{:line 290 :file "cli/wake-receipt-internal.bclj"} (durable-event! port target event-id)
   candidates ^{:line 291 :file "cli/wake-receipt-internal.bclj"} (->> ^{:line 291 :file "cli/wake-receipt-internal.bclj"} (run-events! port target ^{:line 291 :file "cli/wake-receipt-internal.bclj"} (:run idle)) ^{:line 292 :file "cli/wake-receipt-internal.bclj"} (filter ^{:line 292 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 293 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 293 :file "cli/wake-receipt-internal.bclj"} (= "model-call.started" ^{:line 293 :file "cli/wake-receipt-internal.bclj"} (get candidate "kind")) ^{:line 294 :file "cli/wake-receipt-internal.bclj"} (> ^{:line 294 :file "cli/wake-receipt-internal.bclj"} (get candidate "sequence") ^{:line 294 :file "cli/wake-receipt-internal.bclj"} (:sequence idle)) ^{:line 295 :file "cli/wake-receipt-internal.bclj"} (>= ^{:line 295 :file "cli/wake-receipt-internal.bclj"} (:commit-version candidate) ^{:line 296 :file "cli/wake-receipt-internal.bclj"} (:version boundary))))) ^{:line 297 :file "cli/wake-receipt-internal.bclj"} (sort-by ^{:line 297 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 297 :file "cli/wake-receipt-internal.bclj"} (get candidate "sequence"))) vec)
   first-event ^{:line 299 :file "cli/wake-receipt-internal.bclj"} (first candidates)
   model-call ^{:line 300 :file "cli/wake-receipt-internal.bclj"} (model-call-id event)]
  ^{:line 301 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 301 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 301 :file "cli/wake-receipt-internal.bclj"} (and first-event ^{:line 302 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 302 :file "cli/wake-receipt-internal.bclj"} (get first-event "id")) ^{:line 303 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 303 :file "cli/wake-receipt-internal.bclj"} (get event "id")) ^{:line 304 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 304 :file "cli/wake-receipt-internal.bclj"} (:run idle) ^{:line 304 :file "cli/wake-receipt-internal.bclj"} (get event "run")) ^{:line 305 :file "cli/wake-receipt-internal.bclj"} (> ^{:line 305 :file "cli/wake-receipt-internal.bclj"} (get event "sequence") ^{:line 305 :file "cli/wake-receipt-internal.bclj"} (:sequence idle)) ^{:line 306 :file "cli/wake-receipt-internal.bclj"} (>= ^{:line 306 :file "cli/wake-receipt-internal.bclj"} (:commit-version event) ^{:line 306 :file "cli/wake-receipt-internal.bclj"} (:version boundary)) ^{:line 307 :file "cli/wake-receipt-internal.bclj"} (string? model-call) ^{:line 308 :file "cli/wake-receipt-internal.bclj"} (not= model-call ^{:line 308 :file "cli/wake-receipt-internal.bclj"} (:model-call idle)))) ^{:line 301 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 309 :file "cli/wake-receipt-internal.bclj"} (fail! "turn evidence is not the distinct first model call admitted after the message")))
  ^{:line 310 :file "cli/wake-receipt-internal.bclj"} {:event event-id :run ^{:line 311 :file "cli/wake-receipt-internal.bclj"} (get event "run") :sequence ^{:line 312 :file "cli/wake-receipt-internal.bclj"} (get event "sequence") :commit-version ^{:line 313 :file "cli/wake-receipt-internal.bclj"} (:commit-version event) :model-call model-call}))

^{:line 316 :file "cli/wake-receipt-internal.bclj"} (defn- action-kind [event]
  ^{:line 317 :file "cli/wake-receipt-internal.bclj"} (let [body ^{:line 317 :file "cli/wake-receipt-internal.bclj"} (payload event)]
  ^{:line 318 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 319 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 319 :file "cli/wake-receipt-internal.bclj"} (= "message.recorded" ^{:line 319 :file "cli/wake-receipt-internal.bclj"} (get event "kind")) ^{:line 320 :file "cli/wake-receipt-internal.bclj"} (= "assistant" ^{:line 320 :file "cli/wake-receipt-internal.bclj"} (get body "role"))) "assistant.message.recorded"
  ^{:line 323 :file "cli/wake-receipt-internal.bclj"} (= "tool.admitted" ^{:line 323 :file "cli/wake-receipt-internal.bclj"} (get event "kind")) "tool.admitted"
  :else nil)))

^{:line 328 :file "cli/wake-receipt-internal.bclj"} (defn- first-action-for-turn! [port ^String target turn ^String event-id ^String expected-kind]
  ^{:line 334 :file "cli/wake-receipt-internal.bclj"} (let [event ^{:line 334 :file "cli/wake-receipt-internal.bclj"} (durable-event! port target event-id)
   candidates ^{:line 335 :file "cli/wake-receipt-internal.bclj"} (->> ^{:line 335 :file "cli/wake-receipt-internal.bclj"} (run-events! port target ^{:line 335 :file "cli/wake-receipt-internal.bclj"} (:run turn)) ^{:line 336 :file "cli/wake-receipt-internal.bclj"} (filter ^{:line 336 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 337 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 337 :file "cli/wake-receipt-internal.bclj"} (> ^{:line 337 :file "cli/wake-receipt-internal.bclj"} (get candidate "sequence") ^{:line 337 :file "cli/wake-receipt-internal.bclj"} (:sequence turn)) ^{:line 338 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 338 :file "cli/wake-receipt-internal.bclj"} (:model-call turn) ^{:line 338 :file "cli/wake-receipt-internal.bclj"} (model-call-id candidate)) ^{:line 339 :file "cli/wake-receipt-internal.bclj"} (string? ^{:line 339 :file "cli/wake-receipt-internal.bclj"} (action-kind candidate))))) ^{:line 340 :file "cli/wake-receipt-internal.bclj"} (sort-by ^{:line 340 :file "cli/wake-receipt-internal.bclj"} (fn [candidate] ^{:line 340 :file "cli/wake-receipt-internal.bclj"} (get candidate "sequence"))) vec)
   first-event ^{:line 342 :file "cli/wake-receipt-internal.bclj"} (first candidates)
   actual-kind ^{:line 343 :file "cli/wake-receipt-internal.bclj"} (action-kind event)]
  ^{:line 344 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 344 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 344 :file "cli/wake-receipt-internal.bclj"} (and first-event ^{:line 345 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 345 :file "cli/wake-receipt-internal.bclj"} (get first-event "id")) ^{:line 346 :file "cli/wake-receipt-internal.bclj"} (= event-id ^{:line 346 :file "cli/wake-receipt-internal.bclj"} (get event "id")) ^{:line 347 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 347 :file "cli/wake-receipt-internal.bclj"} (:run turn) ^{:line 347 :file "cli/wake-receipt-internal.bclj"} (get event "run")) ^{:line 348 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 348 :file "cli/wake-receipt-internal.bclj"} (:model-call turn) ^{:line 348 :file "cli/wake-receipt-internal.bclj"} (model-call-id event)) ^{:line 349 :file "cli/wake-receipt-internal.bclj"} (= expected-kind actual-kind))) ^{:line 344 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 350 :file "cli/wake-receipt-internal.bclj"} (fail! "action evidence is not the first assistant/tool event for the wake turn")))
  ^{:line 351 :file "cli/wake-receipt-internal.bclj"} {:event event-id :kind actual-kind :sequence ^{:line 353 :file "cli/wake-receipt-internal.bclj"} (get event "sequence")}))

^{:line 355 :file "cli/wake-receipt-internal.bclj"} (defn- idle-facts [boundary idle]
  ^{:line 358 :file "cli/wake-receipt-internal.bclj"} [^{:line 358 :file "cli/wake-receipt-internal.bclj"} ["wake_message_admission_version" ^{:line 358 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 358 :file "cli/wake-receipt-internal.bclj"} (:version boundary))] ^{:line 359 :file "cli/wake-receipt-internal.bclj"} ["wake_message_admission_ordinal" ^{:line 359 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 359 :file "cli/wake-receipt-internal.bclj"} (:ordinal boundary))] ^{:line 360 :file "cli/wake-receipt-internal.bclj"} ["wake_idle_event" ^{:line 360 :file "cli/wake-receipt-internal.bclj"} (:event idle)] ^{:line 361 :file "cli/wake-receipt-internal.bclj"} ["wake_idle_run_id" ^{:line 361 :file "cli/wake-receipt-internal.bclj"} (:run idle)] ^{:line 362 :file "cli/wake-receipt-internal.bclj"} ["wake_idle_sequence" ^{:line 362 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 362 :file "cli/wake-receipt-internal.bclj"} (:sequence idle))] ^{:line 363 :file "cli/wake-receipt-internal.bclj"} ["wake_idle_commit_version" ^{:line 363 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 363 :file "cli/wake-receipt-internal.bclj"} (:commit-version idle))] ^{:line 364 :file "cli/wake-receipt-internal.bclj"} ["wake_idle_model_call_id" ^{:line 364 :file "cli/wake-receipt-internal.bclj"} (:model-call idle)]])

^{:line 366 :file "cli/wake-receipt-internal.bclj"} (defn- turn-facts [turn]
  ^{:line 367 :file "cli/wake-receipt-internal.bclj"} [^{:line 367 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_event" ^{:line 367 :file "cli/wake-receipt-internal.bclj"} (:event turn)] ^{:line 368 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_run_id" ^{:line 368 :file "cli/wake-receipt-internal.bclj"} (:run turn)] ^{:line 369 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_sequence" ^{:line 369 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 369 :file "cli/wake-receipt-internal.bclj"} (:sequence turn))] ^{:line 370 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_commit_version" ^{:line 370 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 370 :file "cli/wake-receipt-internal.bclj"} (:commit-version turn))] ^{:line 371 :file "cli/wake-receipt-internal.bclj"} ["wake_turn_model_call_id" ^{:line 371 :file "cli/wake-receipt-internal.bclj"} (:model-call turn)]])

^{:line 373 :file "cli/wake-receipt-internal.bclj"} (defn- action-facts [action]
  ^{:line 374 :file "cli/wake-receipt-internal.bclj"} [^{:line 374 :file "cli/wake-receipt-internal.bclj"} ["wake_first_action_event" ^{:line 374 :file "cli/wake-receipt-internal.bclj"} (:event action)] ^{:line 375 :file "cli/wake-receipt-internal.bclj"} ["wake_first_action_kind" ^{:line 375 :file "cli/wake-receipt-internal.bclj"} (:kind action)] ^{:line 376 :file "cli/wake-receipt-internal.bclj"} ["wake_first_action_sequence" ^{:line 376 :file "cli/wake-receipt-internal.bclj"} (str ^{:line 376 :file "cli/wake-receipt-internal.bclj"} (:sequence action))]])

^{:line 378 :file "cli/wake-receipt-internal.bclj"} (defn- group-state! [port ^String message predicates]
  ^{:line 382 :file "cli/wake-receipt-internal.bclj"} (let [envelopes ^{:line 382 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 382 :file "cli/wake-receipt-internal.bclj"} (fn [^String predicate] ^{:line 383 :file "cli/wake-receipt-internal.bclj"} (envelope! port message predicate)) predicates)
   states ^{:line 385 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 385 :file "cli/wake-receipt-internal.bclj"} (fn [envelope] ^{:line 386 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 387 :file "cli/wake-receipt-internal.bclj"} (absent? envelope) :absent
  ^{:line 388 :file "cli/wake-receipt-internal.bclj"} (exact-envelope? envelope) :exact
  :else :contradictory)) envelopes)]
  ^{:line 391 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 392 :file "cli/wake-receipt-internal.bclj"} (every? ^{:line 392 :file "cli/wake-receipt-internal.bclj"} (fn [state] ^{:line 392 :file "cli/wake-receipt-internal.bclj"} (= :absent state)) states) ^{:line 392 :file "cli/wake-receipt-internal.bclj"} {:state :absent}
  ^{:line 393 :file "cli/wake-receipt-internal.bclj"} (every? ^{:line 393 :file "cli/wake-receipt-internal.bclj"} (fn [state] ^{:line 393 :file "cli/wake-receipt-internal.bclj"} (= :exact state)) states) ^{:line 394 :file "cli/wake-receipt-internal.bclj"} {:state :exact :values ^{:line 395 :file "cli/wake-receipt-internal.bclj"} (zipmap predicates ^{:line 396 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 396 :file "cli/wake-receipt-internal.bclj"} (fn [envelope] ^{:line 396 :file "cli/wake-receipt-internal.bclj"} (:value envelope)) envelopes))}
  :else ^{:line 397 :file "cli/wake-receipt-internal.bclj"} {:state :contradictory})))

^{:line 399 :file "cli/wake-receipt-internal.bclj"} (defn- require-group-exact! [port ^String message predicates]
  ^{:line 403 :file "cli/wake-receipt-internal.bclj"} (let [group ^{:line 403 :file "cli/wake-receipt-internal.bclj"} (group-state! port message predicates)]
  ^{:line 404 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 404 :file "cli/wake-receipt-internal.bclj"} (= :exact ^{:line 404 :file "cli/wake-receipt-internal.bclj"} (:state group)) ^{:line 405 :file "cli/wake-receipt-internal.bclj"} (:values group) ^{:line 406 :file "cli/wake-receipt-internal.bclj"} (fail! "wake milestone is absent, partial, or ambiguous"))))

^{:line 408 :file "cli/wake-receipt-internal.bclj"} (defn- compare-facts! [values facts]
  ^{:line 411 :file "cli/wake-receipt-internal.bclj"} (doseq [fact facts]
  ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (= ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (get values ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (first fact)) ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (second fact))) ^{:line 412 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 413 :file "cli/wake-receipt-internal.bclj"} (fail! "wake milestone differs from exact durable Wire evidence")))))

^{:line 415 :file "cli/wake-receipt-internal.bclj"} (defn- resolved-idle! [port context]
  ^{:line 418 :file "cli/wake-receipt-internal.bclj"} (let [values ^{:line 418 :file "cli/wake-receipt-internal.bclj"} (require-group-exact! port ^{:line 418 :file "cli/wake-receipt-internal.bclj"} (:message context) idle-predicates)
   boundary ^{:line 419 :file "cli/wake-receipt-internal.bclj"} (message-boundary! port context)
   idle ^{:line 420 :file "cli/wake-receipt-internal.bclj"} (idle-evidence! port ^{:line 420 :file "cli/wake-receipt-internal.bclj"} (:target context) boundary ^{:line 421 :file "cli/wake-receipt-internal.bclj"} (get values "wake_idle_event"))]
  ^{:line 422 :file "cli/wake-receipt-internal.bclj"} (compare-facts! values ^{:line 422 :file "cli/wake-receipt-internal.bclj"} (idle-facts boundary idle))
  ^{:line 423 :file "cli/wake-receipt-internal.bclj"} (assoc idle :boundary boundary)))

^{:line 425 :file "cli/wake-receipt-internal.bclj"} (defn- resolved-turn! [port context idle]
  ^{:line 429 :file "cli/wake-receipt-internal.bclj"} (let [values ^{:line 429 :file "cli/wake-receipt-internal.bclj"} (require-group-exact! port ^{:line 429 :file "cli/wake-receipt-internal.bclj"} (:message context) turn-predicates)
   turn ^{:line 430 :file "cli/wake-receipt-internal.bclj"} (first-turn-after-idle! port ^{:line 430 :file "cli/wake-receipt-internal.bclj"} (:target context) ^{:line 430 :file "cli/wake-receipt-internal.bclj"} (:boundary idle) idle ^{:line 431 :file "cli/wake-receipt-internal.bclj"} (get values "wake_turn_event"))]
  ^{:line 432 :file "cli/wake-receipt-internal.bclj"} (compare-facts! values ^{:line 432 :file "cli/wake-receipt-internal.bclj"} (turn-facts turn))
  turn))

^{:line 435 :file "cli/wake-receipt-internal.bclj"} (defn- resolved-action! [port context turn]
  ^{:line 439 :file "cli/wake-receipt-internal.bclj"} (let [values ^{:line 439 :file "cli/wake-receipt-internal.bclj"} (require-group-exact! port ^{:line 439 :file "cli/wake-receipt-internal.bclj"} (:message context) action-predicates)
   action ^{:line 440 :file "cli/wake-receipt-internal.bclj"} (first-action-for-turn! port ^{:line 441 :file "cli/wake-receipt-internal.bclj"} (:target context) turn ^{:line 442 :file "cli/wake-receipt-internal.bclj"} (get values "wake_first_action_event") ^{:line 443 :file "cli/wake-receipt-internal.bclj"} (get values "wake_first_action_kind"))]
  ^{:line 444 :file "cli/wake-receipt-internal.bclj"} (compare-facts! values ^{:line 444 :file "cli/wake-receipt-internal.bclj"} (action-facts action))
  action))

^{:line 447 :file "cli/wake-receipt-internal.bclj"} (defn- milestone-state! [port ^String message facts]
  ^{:line 451 :file "cli/wake-receipt-internal.bclj"} (let [states ^{:line 451 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 452 :file "cli/wake-receipt-internal.bclj"} (fn [fact] ^{:line 453 :file "cli/wake-receipt-internal.bclj"} (let [^String predicate ^{:line 453 :file "cli/wake-receipt-internal.bclj"} (first fact)
   ^String expected ^{:line 454 :file "cli/wake-receipt-internal.bclj"} (second fact)
   current ^{:line 455 :file "cli/wake-receipt-internal.bclj"} (envelope! port message predicate)]
  ^{:line 456 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 457 :file "cli/wake-receipt-internal.bclj"} (absent? current) :absent
  ^{:line 458 :file "cli/wake-receipt-internal.bclj"} (exact-singleton? current expected) :exact
  :else :contradictory))) facts)]
  ^{:line 461 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 462 :file "cli/wake-receipt-internal.bclj"} (every? ^{:line 462 :file "cli/wake-receipt-internal.bclj"} (fn [state] ^{:line 462 :file "cli/wake-receipt-internal.bclj"} (= state :absent)) states) :absent
  ^{:line 463 :file "cli/wake-receipt-internal.bclj"} (every? ^{:line 463 :file "cli/wake-receipt-internal.bclj"} (fn [state] ^{:line 463 :file "cli/wake-receipt-internal.bclj"} (= state :exact)) states) :exact
  :else :contradictory)))

^{:line 466 :file "cli/wake-receipt-internal.bclj"} (defn- publish-exact! [port ^String message facts]
  ^{:line 470 :file "cli/wake-receipt-internal.bclj"} (let [state ^{:line 470 :file "cli/wake-receipt-internal.bclj"} (milestone-state! port message facts)]
  ^{:line 471 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 472 :file "cli/wake-receipt-internal.bclj"} (= state :exact) :existing
  ^{:line 473 :file "cli/wake-receipt-internal.bclj"} (= state :contradictory) ^{:line 473 :file "cli/wake-receipt-internal.bclj"} (fail! "wake milestone contradicts committed evidence")
  :else ^{:line 475 :file "cli/wake-receipt-internal.bclj"} (let [result ^{:line 475 :file "cli/wake-receipt-internal.bclj"} (coord-invoke! "publish!" ^{:line 477 :file "cli/wake-receipt-internal.bclj"} [port ^{:line 478 :file "cli/wake-receipt-internal.bclj"} (mapv ^{:line 479 :file "cli/wake-receipt-internal.bclj"} (fn [fact] ^{:line 480 :file "cli/wake-receipt-internal.bclj"} {:op :assert :subject message :predicate ^{:line 482 :file "cli/wake-receipt-internal.bclj"} (first fact) :value ^{:line 483 :file "cli/wake-receipt-internal.bclj"} (second fact) :cardinality :one}) facts)])]
  ^{:line 486 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 486 :file "cli/wake-receipt-internal.bclj"} (:reject result) ^{:line 487 :file "cli/wake-receipt-internal.bclj"} (fail! ^{:line 487 :file "cli/wake-receipt-internal.bclj"} (str "coordinator rejected milestone: " ^{:line 487 :file "cli/wake-receipt-internal.bclj"} (:reject result))) ^{:line 488 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 488 :file "cli/wake-receipt-internal.bclj"} (= :exact ^{:line 488 :file "cli/wake-receipt-internal.bclj"} (milestone-state! port message facts)) :created ^{:line 490 :file "cli/wake-receipt-internal.bclj"} (fail! "wake milestone readback is not exact")))))))

^{:line 492 :file "cli/wake-receipt-internal.bclj"} (defn wake-status! [port ^String message]
  ^{:line 495 :file "cli/wake-receipt-internal.bclj"} (let [attempt-envelope ^{:line 495 :file "cli/wake-receipt-internal.bclj"} (envelope! port message "wake_attempt_id")]
  ^{:line 496 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 496 :file "cli/wake-receipt-internal.bclj"} (absent? attempt-envelope) ^{:line 497 :file "cli/wake-receipt-internal.bclj"} {:status "not-applicable" :missing ^{:line 497 :file "cli/wake-receipt-internal.bclj"} [] :failures ^{:line 497 :file "cli/wake-receipt-internal.bclj"} []} ^{:line 498 :file "cli/wake-receipt-internal.bclj"} (try
  ^{:line 499 :file "cli/wake-receipt-internal.bclj"} (let [context ^{:line 499 :file "cli/wake-receipt-internal.bclj"} (historic-message-context! port message)
   failure-envelope ^{:line 500 :file "cli/wake-receipt-internal.bclj"} (envelope! port message "wake_failure")
   idle-group ^{:line 501 :file "cli/wake-receipt-internal.bclj"} (group-state! port message idle-predicates)
   turn-group ^{:line 502 :file "cli/wake-receipt-internal.bclj"} (group-state! port message turn-predicates)
   action-group ^{:line 503 :file "cli/wake-receipt-internal.bclj"} (group-state! port message action-predicates)]
  ^{:line 504 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 505 :file "cli/wake-receipt-internal.bclj"} (exact-envelope? failure-envelope) ^{:line 506 :file "cli/wake-receipt-internal.bclj"} {:status "failure" :missing ^{:line 506 :file "cli/wake-receipt-internal.bclj"} [] :failures ^{:line 506 :file "cli/wake-receipt-internal.bclj"} [^{:line 506 :file "cli/wake-receipt-internal.bclj"} (:value failure-envelope)]}
  ^{:line 508 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 508 :file "cli/wake-receipt-internal.bclj"} (absent? failure-envelope)) ^{:line 509 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 509 :file "cli/wake-receipt-internal.bclj"} ["ambiguous-failure"] :failures ^{:line 509 :file "cli/wake-receipt-internal.bclj"} []}
  ^{:line 511 :file "cli/wake-receipt-internal.bclj"} (= :contradictory ^{:line 511 :file "cli/wake-receipt-internal.bclj"} (:state idle-group)) ^{:line 512 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 512 :file "cli/wake-receipt-internal.bclj"} ["invalid-idle-proof"] :failures ^{:line 512 :file "cli/wake-receipt-internal.bclj"} []}
  ^{:line 514 :file "cli/wake-receipt-internal.bclj"} (= :absent ^{:line 514 :file "cli/wake-receipt-internal.bclj"} (:state idle-group)) ^{:line 515 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 516 :file "cli/wake-receipt-internal.bclj"} ["idle-proof" "target-turn" "first-action"] :failures ^{:line 517 :file "cli/wake-receipt-internal.bclj"} []}
  :else ^{:line 520 :file "cli/wake-receipt-internal.bclj"} (let [idle ^{:line 520 :file "cli/wake-receipt-internal.bclj"} (resolved-idle! port context)]
  ^{:line 521 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 522 :file "cli/wake-receipt-internal.bclj"} (= :contradictory ^{:line 522 :file "cli/wake-receipt-internal.bclj"} (:state turn-group)) ^{:line 523 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 523 :file "cli/wake-receipt-internal.bclj"} ["invalid-target-turn"] :failures ^{:line 523 :file "cli/wake-receipt-internal.bclj"} []}
  ^{:line 525 :file "cli/wake-receipt-internal.bclj"} (= :absent ^{:line 525 :file "cli/wake-receipt-internal.bclj"} (:state turn-group)) ^{:line 526 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 526 :file "cli/wake-receipt-internal.bclj"} ["target-turn" "first-action"] :failures ^{:line 526 :file "cli/wake-receipt-internal.bclj"} []}
  :else ^{:line 529 :file "cli/wake-receipt-internal.bclj"} (let [turn ^{:line 529 :file "cli/wake-receipt-internal.bclj"} (resolved-turn! port context idle)]
  ^{:line 530 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 531 :file "cli/wake-receipt-internal.bclj"} (= :contradictory ^{:line 531 :file "cli/wake-receipt-internal.bclj"} (:state action-group)) ^{:line 532 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 532 :file "cli/wake-receipt-internal.bclj"} ["invalid-first-action"] :failures ^{:line 532 :file "cli/wake-receipt-internal.bclj"} []}
  ^{:line 534 :file "cli/wake-receipt-internal.bclj"} (= :absent ^{:line 534 :file "cli/wake-receipt-internal.bclj"} (:state action-group)) ^{:line 535 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 535 :file "cli/wake-receipt-internal.bclj"} ["first-action"] :failures ^{:line 535 :file "cli/wake-receipt-internal.bclj"} []}
  :else ^{:line 538 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 539 :file "cli/wake-receipt-internal.bclj"} (resolved-action! port context turn)
  ^{:line 540 :file "cli/wake-receipt-internal.bclj"} {:status "complete" :missing ^{:line 540 :file "cli/wake-receipt-internal.bclj"} [] :failures ^{:line 540 :file "cli/wake-receipt-internal.bclj"} []})))))))
  (catch Exception _
    ^{:line 542 :file "cli/wake-receipt-internal.bclj"} {:status "unknown" :missing ^{:line 542 :file "cli/wake-receipt-internal.bclj"} ["invalid-durable-evidence"] :failures ^{:line 542 :file "cli/wake-receipt-internal.bclj"} []})))))

^{:line 544 :file "cli/wake-receipt-internal.bclj"} (defn- idle-phase! [port context ^String event]
  ^{:line 548 :file "cli/wake-receipt-internal.bclj"} (let [idle-group ^{:line 548 :file "cli/wake-receipt-internal.bclj"} (group-state! port ^{:line 548 :file "cli/wake-receipt-internal.bclj"} (:message context) idle-predicates)]
  ^{:line 549 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 550 :file "cli/wake-receipt-internal.bclj"} (= :absent ^{:line 550 :file "cli/wake-receipt-internal.bclj"} (:state idle-group)) ^{:line 551 :file "cli/wake-receipt-internal.bclj"} (let [boundary ^{:line 551 :file "cli/wake-receipt-internal.bclj"} (message-boundary! port context)
   idle ^{:line 552 :file "cli/wake-receipt-internal.bclj"} (idle-evidence! port ^{:line 552 :file "cli/wake-receipt-internal.bclj"} (:target context) boundary event)]
  ^{:line 553 :file "cli/wake-receipt-internal.bclj"} (publish-exact! port ^{:line 553 :file "cli/wake-receipt-internal.bclj"} (:message context) ^{:line 553 :file "cli/wake-receipt-internal.bclj"} (idle-facts boundary idle)))
  ^{:line 555 :file "cli/wake-receipt-internal.bclj"} (= :exact ^{:line 555 :file "cli/wake-receipt-internal.bclj"} (:state idle-group)) ^{:line 556 :file "cli/wake-receipt-internal.bclj"} (let [idle ^{:line 556 :file "cli/wake-receipt-internal.bclj"} (resolved-idle! port context)
   turn-group ^{:line 557 :file "cli/wake-receipt-internal.bclj"} (group-state! port ^{:line 557 :file "cli/wake-receipt-internal.bclj"} (:message context) turn-predicates)]
  ^{:line 558 :file "cli/wake-receipt-internal.bclj"} (cond
  ^{:line 559 :file "cli/wake-receipt-internal.bclj"} (= :absent ^{:line 559 :file "cli/wake-receipt-internal.bclj"} (:state turn-group)) :unknown
  ^{:line 560 :file "cli/wake-receipt-internal.bclj"} (= :exact ^{:line 560 :file "cli/wake-receipt-internal.bclj"} (:state turn-group)) ^{:line 561 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 561 :file "cli/wake-receipt-internal.bclj"} (resolved-turn! port context idle)
  :accepted)
  :else ^{:line 562 :file "cli/wake-receipt-internal.bclj"} (fail! "existing wake attempt has ambiguous turn evidence")))
  :else ^{:line 564 :file "cli/wake-receipt-internal.bclj"} (fail! "existing wake attempt has ambiguous idle evidence"))))

^{:line 566 :file "cli/wake-receipt-internal.bclj"} (defn- turn-phase! [port context ^String event]
  ^{:line 570 :file "cli/wake-receipt-internal.bclj"} (let [idle ^{:line 570 :file "cli/wake-receipt-internal.bclj"} (resolved-idle! port context)
   turn ^{:line 571 :file "cli/wake-receipt-internal.bclj"} (first-turn-after-idle! port ^{:line 571 :file "cli/wake-receipt-internal.bclj"} (:target context) ^{:line 571 :file "cli/wake-receipt-internal.bclj"} (:boundary idle) idle event)]
  ^{:line 573 :file "cli/wake-receipt-internal.bclj"} (publish-exact! port ^{:line 573 :file "cli/wake-receipt-internal.bclj"} (:message context) ^{:line 573 :file "cli/wake-receipt-internal.bclj"} (turn-facts turn))))

^{:line 575 :file "cli/wake-receipt-internal.bclj"} (defn- action-phase! [port context ^String event kind]
  ^{:line 580 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 580 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 580 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 580 :file "cli/wake-receipt-internal.bclj"} (string? kind) ^{:line 581 :file "cli/wake-receipt-internal.bclj"} (^{:line 581 :file "cli/wake-receipt-internal.bclj"} #{"assistant.message.recorded" "tool.admitted"} kind))) ^{:line 580 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 582 :file "cli/wake-receipt-internal.bclj"} (fail! "action kind is invalid")))
  ^{:line 583 :file "cli/wake-receipt-internal.bclj"} (let [idle ^{:line 583 :file "cli/wake-receipt-internal.bclj"} (resolved-idle! port context)
   turn ^{:line 584 :file "cli/wake-receipt-internal.bclj"} (resolved-turn! port context idle)
   action ^{:line 585 :file "cli/wake-receipt-internal.bclj"} (first-action-for-turn! port ^{:line 585 :file "cli/wake-receipt-internal.bclj"} (:target context) turn event kind)]
  ^{:line 586 :file "cli/wake-receipt-internal.bclj"} (publish-exact! port ^{:line 586 :file "cli/wake-receipt-internal.bclj"} (:message context) ^{:line 586 :file "cli/wake-receipt-internal.bclj"} (action-facts action))))

^{:line 588 :file "cli/wake-receipt-internal.bclj"} (defn- failure-phase! [port context ^String reason]
  ^{:line 592 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 592 :file "cli/wake-receipt-internal.bclj"} (and ^{:line 592 :file "cli/wake-receipt-internal.bclj"} (not ^{:line 592 :file "cli/wake-receipt-internal.bclj"} (str/blank? reason)) ^{:line 593 :file "cli/wake-receipt-internal.bclj"} (<= ^{:line 593 :file "cli/wake-receipt-internal.bclj"} (count reason) 256)) ^{:line 594 :file "cli/wake-receipt-internal.bclj"} (publish-exact! port ^{:line 594 :file "cli/wake-receipt-internal.bclj"} (:message context) ^{:line 594 :file "cli/wake-receipt-internal.bclj"} [^{:line 594 :file "cli/wake-receipt-internal.bclj"} ["wake_failure" reason]]) ^{:line 595 :file "cli/wake-receipt-internal.bclj"} (fail! "failure reason is invalid")))

^{:line 597 :file "cli/wake-receipt-internal.bclj"} (defn- print-result! [result]
  ^{:line 598 :file "cli/wake-receipt-internal.bclj"} (println ^{:line 598 :file "cli/wake-receipt-internal.bclj"} (name result)))

^{:line 600 :file "cli/wake-receipt-internal.bclj"} (defn- ^String required-arg [args index]
  ^{:line 603 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 603 :file "cli/wake-receipt-internal.bclj"} (< index ^{:line 603 :file "cli/wake-receipt-internal.bclj"} (count args)) ^{:line 603 :file "cli/wake-receipt-internal.bclj"} (nth args index) ""))

^{:line 605 :file "cli/wake-receipt-internal.bclj"} (defn- optional-arg [args index]
  ^{:line 608 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 608 :file "cli/wake-receipt-internal.bclj"} (< index ^{:line 608 :file "cli/wake-receipt-internal.bclj"} (count args)) ^{:line 608 :file "cli/wake-receipt-internal.bclj"} (nth args index) nil))

^{:line 610 :file "cli/wake-receipt-internal.bclj"} (defn- parse-port! [^String raw]
  ^{:line 611 :file "cli/wake-receipt-internal.bclj"} (let [parsed ^{:line 611 :file "cli/wake-receipt-internal.bclj"} (parse-long raw)]
  ^{:line 612 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 612 :file "cli/wake-receipt-internal.bclj"} (some? parsed) parsed ^{:line 612 :file "cli/wake-receipt-internal.bclj"} (fail! "port is malformed"))))

^{:line 614 :file "cli/wake-receipt-internal.bclj"} (defn- run-cli! [args]
  ^{:line 615 :file "cli/wake-receipt-internal.bclj"} (let [port ^{:line 615 :file "cli/wake-receipt-internal.bclj"} (parse-port! ^{:line 615 :file "cli/wake-receipt-internal.bclj"} (required-arg args 0))
   ^String phase ^{:line 616 :file "cli/wake-receipt-internal.bclj"} (required-arg args 1)
   ^String message ^{:line 617 :file "cli/wake-receipt-internal.bclj"} (required-arg args 2)]
  ^{:line 618 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 618 :file "cli/wake-receipt-internal.bclj"} (= phase "status") ^{:line 619 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 619 :file "cli/wake-receipt-internal.bclj"} (= 3 ^{:line 619 :file "cli/wake-receipt-internal.bclj"} (count args)) ^{:line 620 :file "cli/wake-receipt-internal.bclj"} (println ^{:line 620 :file "cli/wake-receipt-internal.bclj"} (json/generate-string ^{:line 620 :file "cli/wake-receipt-internal.bclj"} (wake-status! port message))) ^{:line 621 :file "cli/wake-receipt-internal.bclj"} (fail! "status usage is <port> status <message>")) ^{:line 622 :file "cli/wake-receipt-internal.bclj"} (let [^String attempt ^{:line 622 :file "cli/wake-receipt-internal.bclj"} (required-arg args 3)
   ^String target ^{:line 623 :file "cli/wake-receipt-internal.bclj"} (required-arg args 4)
   ^String epoch ^{:line 624 :file "cli/wake-receipt-internal.bclj"} (required-arg args 5)
   ^String event ^{:line 625 :file "cli/wake-receipt-internal.bclj"} (required-arg args 6)
   kind ^{:line 626 :file "cli/wake-receipt-internal.bclj"} (optional-arg args 7)]
  ^{:line 627 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 627 :file "cli/wake-receipt-internal.bclj"} (> ^{:line 627 :file "cli/wake-receipt-internal.bclj"} (count args) 8) ^{:line 627 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 628 :file "cli/wake-receipt-internal.bclj"} (fail! "usage: <port> <idle|turn|action|failure> <message> <attempt> <target> <epoch> <event-or-reason> [kind]")))
  ^{:line 629 :file "cli/wake-receipt-internal.bclj"} (let [context ^{:line 629 :file "cli/wake-receipt-internal.bclj"} (current-message-context! port message attempt target epoch)]
  ^{:line 630 :file "cli/wake-receipt-internal.bclj"} (case phase
    "idle" ^{:line 632 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 632 :file "cli/wake-receipt-internal.bclj"} (nil? kind) ^{:line 633 :file "cli/wake-receipt-internal.bclj"} (print-result! ^{:line 633 :file "cli/wake-receipt-internal.bclj"} (idle-phase! port context event)) ^{:line 634 :file "cli/wake-receipt-internal.bclj"} (fail! "idle accepts no action kind"))
    "turn" ^{:line 637 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 637 :file "cli/wake-receipt-internal.bclj"} (nil? kind) ^{:line 638 :file "cli/wake-receipt-internal.bclj"} (print-result! ^{:line 638 :file "cli/wake-receipt-internal.bclj"} (turn-phase! port context event)) ^{:line 639 :file "cli/wake-receipt-internal.bclj"} (fail! "turn accepts no action kind"))
    "action" ^{:line 642 :file "cli/wake-receipt-internal.bclj"} (print-result! ^{:line 642 :file "cli/wake-receipt-internal.bclj"} (action-phase! port context event kind))
    "failure" ^{:line 645 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 645 :file "cli/wake-receipt-internal.bclj"} (nil? kind) ^{:line 646 :file "cli/wake-receipt-internal.bclj"} (print-result! ^{:line 646 :file "cli/wake-receipt-internal.bclj"} (failure-phase! port context event)) ^{:line 647 :file "cli/wake-receipt-internal.bclj"} (fail! "failure accepts no action kind"))
    ^{:line 649 :file "cli/wake-receipt-internal.bclj"} (fail! "phase must be idle, turn, action, failure, or status")))))))

^{:line 651 :file "cli/wake-receipt-internal.bclj"} (defn- ^Boolean direct-invocation? []
  ^{:line 652 :file "cli/wake-receipt-internal.bclj"} (= script-file ^{:line 653 :file "cli/wake-receipt-internal.bclj"} (.getCanonicalPath ^{:line 653 :file "cli/wake-receipt-internal.bclj"} (io/file ^{:line 653 :file "cli/wake-receipt-internal.bclj"} (System/getProperty "babashka.file")))))

^{:line 655 :file "cli/wake-receipt-internal.bclj"} (if ^{:line 655 :file "cli/wake-receipt-internal.bclj"} (direct-invocation?) ^{:line 655 :file "cli/wake-receipt-internal.bclj"} (do
  ^{:line 656 :file "cli/wake-receipt-internal.bclj"} (try
  ^{:line 657 :file "cli/wake-receipt-internal.bclj"} (run-cli! ^{:line 657 :file "cli/wake-receipt-internal.bclj"} (vec *command-line-args*))
  (catch Exception _
    ^{:line 658 :file "cli/wake-receipt-internal.bclj"} (System/exit 2)))))
