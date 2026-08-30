(ns north.message-routing
  (:require [clojure.string :as str]))

^{:line 16 :file "cli/message-routing.bclj"} (def ^String broadcast-address "*")

^{:line 17 :file "cli/message-routing.bclj"} (def ^String role-prefix "@role:")

^{:line 18 :file "cli/message-routing.bclj"} (def ^String agent-prefix "@agent:")

^{:line 19 :file "cli/message-routing.bclj"} (def ^String listener-lease-prefix "listener:")

^{:line 20 :file "cli/message-routing.bclj"} (def listener-generation-pattern #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

^{:line 22 :file "cli/message-routing.bclj"} (def max-route-candidates 4096)

^{:line 23 :file "cli/message-routing.bclj"} (def route-page-size 128)

^{:line 24 :file "cli/message-routing.bclj"} (def max-mail-candidates 16384)

^{:line 25 :file "cli/message-routing.bclj"} (def mail-page-size 512)

^{:line 27 :file "cli/message-routing.bclj"} (defn bare-agent [value]
  ^{:line 28 :file "cli/message-routing.bclj"} (if ^{:line 28 :file "cli/message-routing.bclj"} (string? value) ^{:line 28 :file "cli/message-routing.bclj"} (do
  ^{:line 29 :file "cli/message-routing.bclj"} (str/replace-first value #"^@agent:" ""))))

^{:line 31 :file "cli/message-routing.bclj"} (defn bare-role [value]
  ^{:line 32 :file "cli/message-routing.bclj"} (if ^{:line 32 :file "cli/message-routing.bclj"} (and ^{:line 32 :file "cli/message-routing.bclj"} (string? value) ^{:line 33 :file "cli/message-routing.bclj"} (str/starts-with? value role-prefix)) ^{:line 32 :file "cli/message-routing.bclj"} (do
  ^{:line 34 :file "cli/message-routing.bclj"} (subs value 6))))

^{:line 36 :file "cli/message-routing.bclj"} (defn ^String agent-subject [control]
  ^{:line 37 :file "cli/message-routing.bclj"} (str agent-prefix ^{:line 37 :file "cli/message-routing.bclj"} (bare-agent control)))

^{:line 39 :file "cli/message-routing.bclj"} (defn ^String listener-resource [control]
  ^{:line 40 :file "cli/message-routing.bclj"} (str listener-lease-prefix ^{:line 40 :file "cli/message-routing.bclj"} (bare-agent control)))

^{:line 42 :file "cli/message-routing.bclj"} (defn ^Boolean lease-live-at? [lease now]
  ^{:line 45 :file "cli/message-routing.bclj"} (and ^{:line 45 :file "cli/message-routing.bclj"} (map? lease) ^{:line 46 :file "cli/message-routing.bclj"} (some? ^{:line 46 :file "cli/message-routing.bclj"} (:resource lease)) ^{:line 47 :file "cli/message-routing.bclj"} (some? ^{:line 47 :file "cli/message-routing.bclj"} (:holder lease)) ^{:line 48 :file "cli/message-routing.bclj"} (integer? ^{:line 48 :file "cli/message-routing.bclj"} (:exp lease)) ^{:line 49 :file "cli/message-routing.bclj"} (> ^{:line 49 :file "cli/message-routing.bclj"} (:exp lease) now)))

^{:line 51 :file "cli/message-routing.bclj"} (defn ^Boolean exact-singleton? [envelope expected]
  ^{:line 54 :file "cli/message-routing.bclj"} (and ^{:line 54 :file "cli/message-routing.bclj"} (map? envelope) ^{:line 55 :file "cli/message-routing.bclj"} (= 1 ^{:line 55 :file "cli/message-routing.bclj"} (:members envelope)) ^{:line 56 :file "cli/message-routing.bclj"} (false? ^{:line 56 :file "cli/message-routing.bclj"} (:ambiguous? envelope)) ^{:line 57 :file "cli/message-routing.bclj"} (= ^{:line 57 :file "cli/message-routing.bclj"} [expected] ^{:line 57 :file "cli/message-routing.bclj"} (:values envelope)) ^{:line 58 :file "cli/message-routing.bclj"} (= expected ^{:line 58 :file "cli/message-routing.bclj"} (:value envelope))))

^{:line 60 :file "cli/message-routing.bclj"} (defn ^Boolean native-listener-live?
  "A native listener's durable armed bit is only descriptive. Reachability also\n   requires the matching renewable generation lease. The before/after lease\n   reads close release/reacquire races around the point reads without rejecting\n   an ordinary renewal, whose holder stays generation-stable while its numeric\n   fence epoch advances." [port ^String control]
  ^{:line 68 :file "cli/message-routing.bclj"} (let [^String subject ^{:line 68 :file "cli/message-routing.bclj"} (agent-subject control)
   ^String resource ^{:line 69 :file "cli/message-routing.bclj"} (listener-resource control)
   before ^{:line 70 :file "cli/message-routing.bclj"} (north.coord/lease-status! port resource)
   generation ^{:line 71 :file "cli/message-routing.bclj"} (north.coord/resolved-envelope! port subject "live_input_epoch")
   state ^{:line 73 :file "cli/message-routing.bclj"} (north.coord/resolved-envelope! port subject "live_input_state")
   after ^{:line 75 :file "cli/message-routing.bclj"} (north.coord/lease-status! port resource)
   now ^{:line 76 :file "cli/message-routing.bclj"} (System/currentTimeMillis)]
  ^{:line 77 :file "cli/message-routing.bclj"} (boolean ^{:line 78 :file "cli/message-routing.bclj"} (and ^{:line 78 :file "cli/message-routing.bclj"} (lease-live-at? before now) ^{:line 79 :file "cli/message-routing.bclj"} (lease-live-at? after now) ^{:line 80 :file "cli/message-routing.bclj"} (= resource ^{:line 80 :file "cli/message-routing.bclj"} (:resource before)) ^{:line 81 :file "cli/message-routing.bclj"} (= resource ^{:line 81 :file "cli/message-routing.bclj"} (:resource after)) ^{:line 82 :file "cli/message-routing.bclj"} (= ^{:line 82 :file "cli/message-routing.bclj"} (:holder before) ^{:line 82 :file "cli/message-routing.bclj"} (:holder after)) ^{:line 83 :file "cli/message-routing.bclj"} (re-matches listener-generation-pattern ^{:line 83 :file "cli/message-routing.bclj"} (:holder before)) ^{:line 84 :file "cli/message-routing.bclj"} (exact-singleton? generation ^{:line 84 :file "cli/message-routing.bclj"} (:holder before)) ^{:line 85 :file "cli/message-routing.bclj"} (exact-singleton? state "armed")))))

^{:line 87 :file "cli/message-routing.bclj"} (defn ^Boolean armed-route-live? [port ^String control]
  ^{:line 90 :file "cli/message-routing.bclj"} (let [^String subject ^{:line 90 :file "cli/message-routing.bclj"} (agent-subject control)
   kind ^{:line 91 :file "cli/message-routing.bclj"} (north.coord/resolved! port subject "kind")]
  ^{:line 92 :file "cli/message-routing.bclj"} (case kind
    "session" ^{:line 93 :file "cli/message-routing.bclj"} (native-listener-live? port control)
    "lane" ^{:line 94 :file "cli/message-routing.bclj"} (exact-singleton? ^{:line 95 :file "cli/message-routing.bclj"} (north.coord/resolved-envelope! port subject "live_input_state") "armed")
    false)))

^{:line 100 :file "cli/message-routing.bclj"} (defn ^Boolean recipient-live?
  "A direct recipient is reachable when its renewable session lease is live or\n   an authoritative listener route is explicitly armed." [port control]
  ^{:line 105 :file "cli/message-routing.bclj"} (let [^String control ^{:line 105 :file "cli/message-routing.bclj"} (or ^{:line 105 :file "cli/message-routing.bclj"} (bare-agent control) "")]
  ^{:line 106 :file "cli/message-routing.bclj"} (boolean ^{:line 107 :file "cli/message-routing.bclj"} (and ^{:line 107 :file "cli/message-routing.bclj"} (not ^{:line 107 :file "cli/message-routing.bclj"} (str/blank? control)) ^{:line 108 :file "cli/message-routing.bclj"} (or ^{:line 108 :file "cli/message-routing.bclj"} (north.coord/session-online?! port control) ^{:line 109 :file "cli/message-routing.bclj"} (armed-route-live? port control))))))

^{:line 111 :file "cli/message-routing.bclj"} (defn role-holders
  "All graph holders of ROLE-SLUG. Ordering is deterministic; liveness determines\n   which holder may receive new mail." [port ^String role-slug]
  ^{:line 116 :file "cli/message-routing.bclj"} (let [response ^{:line 116 :file "cli/message-routing.bclj"} (north.coord/query-page! port ^{:line 118 :file "cli/message-routing.bclj"} {:find "role_holder" :rules ^{:line 120 :file "cli/message-routing.bclj"} [^{:line 120 :file "cli/message-routing.bclj"} {:head ^{:line 120 :file "cli/message-routing.bclj"} {:rel "role_holder" :args ^{:line 120 :file "cli/message-routing.bclj"} [^{:line 120 :file "cli/message-routing.bclj"} {:var "agent"}]} :body ^{:line 121 :file "cli/message-routing.bclj"} [^{:line 121 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 122 :file "cli/message-routing.bclj"} [^{:line 122 :file "cli/message-routing.bclj"} {:var "agent"} "holds" ^{:line 123 :file "cli/message-routing.bclj"} (str role-prefix role-slug)]}]}]} route-page-size nil)]
  ^{:line 125 :file "cli/message-routing.bclj"} (if ^{:line 125 :file "cli/message-routing.bclj"} (not ^{:line 125 :file "cli/message-routing.bclj"} (:done? response)) ^{:line 125 :file "cli/message-routing.bclj"} (do
  ^{:line 126 :file "cli/message-routing.bclj"} (throw ^{:line 127 :file "cli/message-routing.bclj"} (ex-info "role holder projection exceeds its bounded page" ^{:line 128 :file "cli/message-routing.bclj"} {:type :role-holder-overflow :role role-slug :max route-page-size}))))
  ^{:line 131 :file "cli/message-routing.bclj"} (->> ^{:line 131 :file "cli/message-routing.bclj"} (:rows response) ^{:line 132 :file "cli/message-routing.bclj"} (mapv first) ^{:line 133 :file "cli/message-routing.bclj"} (filterv ^{:line 133 :file "cli/message-routing.bclj"} (fn [^String candidate] ^{:line 134 :file "cli/message-routing.bclj"} (str/starts-with? candidate agent-prefix))) ^{:line 135 :file "cli/message-routing.bclj"} (mapv bare-agent) distinct sort vec)))

^{:line 140 :file "cli/message-routing.bclj"} (defn live-role-holder [port ^String role-slug]
  ^{:line 143 :file "cli/message-routing.bclj"} (first ^{:line 144 :file "cli/message-routing.bclj"} (filterv ^{:line 144 :file "cli/message-routing.bclj"} (fn [^String holder] ^{:line 145 :file "cli/message-routing.bclj"} (recipient-live? port holder)) ^{:line 146 :file "cli/message-routing.bclj"} (role-holders port role-slug))))

^{:line 148 :file "cli/message-routing.bclj"} (defn resolve-address
  "Resolve ADDRESS to one concrete session at send time. A durable @role target\n   is authoritative. Existing held-role routing remains supported while old\n   sessions converge onto durable aliases." [port ^String address]
  ^{:line 154 :file "cli/message-routing.bclj"} (cond
  ^{:line 155 :file "cli/message-routing.bclj"} (= broadcast-address address) ^{:line 156 :file "cli/message-routing.bclj"} {:address address :recipient address :kind :broadcast}
  :else ^{:line 159 :file "cli/message-routing.bclj"} (let [^String role ^{:line 159 :file "cli/message-routing.bclj"} (str role-prefix address)
   durable ^{:line 160 :file "cli/message-routing.bclj"} (north.coord/resolved! port role "target")]
  ^{:line 161 :file "cli/message-routing.bclj"} (cond
  ^{:line 162 :file "cli/message-routing.bclj"} (not ^{:line 162 :file "cli/message-routing.bclj"} (str/blank? durable)) ^{:line 163 :file "cli/message-routing.bclj"} {:address address :recipient ^{:line 163 :file "cli/message-routing.bclj"} (bare-agent durable) :kind :alias}
  ^{:line 165 :file "cli/message-routing.bclj"} (recipient-live? port address) ^{:line 166 :file "cli/message-routing.bclj"} {:address address :recipient ^{:line 166 :file "cli/message-routing.bclj"} (bare-agent address) :kind :direct}
  :else ^{:line 169 :file "cli/message-routing.bclj"} (let [bind__0 ^{:line 169 :file "cli/message-routing.bclj"} (live-role-holder port address)]
  ^{:line 169 :file "cli/message-routing.bclj"} (if bind__0 ^{:line 169 :file "cli/message-routing.bclj"} (let [holder bind__0]
  ^{:line 170 :file "cli/message-routing.bclj"} {:address address :recipient holder :kind :held-role}) ^{:line 171 :file "cli/message-routing.bclj"} {:address address :recipient ^{:line 171 :file "cli/message-routing.bclj"} (bare-agent address) :kind :direct}))))))

^{:line 173 :file "cli/message-routing.bclj"} (defn- route-candidate-query [^String repo ^String role]
  ^{:line 176 :file "cli/message-routing.bclj"} {:find "same_route_agent" :rules ^{:line 178 :file "cli/message-routing.bclj"} [^{:line 178 :file "cli/message-routing.bclj"} {:head ^{:line 178 :file "cli/message-routing.bclj"} {:rel "same_route_agent" :args ^{:line 178 :file "cli/message-routing.bclj"} [^{:line 178 :file "cli/message-routing.bclj"} {:var "agent"}]} :body ^{:line 179 :file "cli/message-routing.bclj"} [^{:line 179 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 179 :file "cli/message-routing.bclj"} [^{:line 179 :file "cli/message-routing.bclj"} {:var "agent"} "repo" repo]} ^{:line 180 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 180 :file "cli/message-routing.bclj"} [^{:line 180 :file "cli/message-routing.bclj"} {:var "agent"} "role" role]}]}]})

^{:line 182 :file "cli/message-routing.bclj"} (defn live-same-route
  "Best-effort diagnostic successor for a dead recipient. Returns nil when the\n   dead identity lacks repo/role provenance or the bounded lookup is unavailable." [port ^String dead-control]
  ^{:line 187 :file "cli/message-routing.bclj"} (try
  ^{:line 188 :file "cli/message-routing.bclj"} (let [^String dead-control ^{:line 188 :file "cli/message-routing.bclj"} (or ^{:line 188 :file "cli/message-routing.bclj"} (bare-agent dead-control) "")
   ^String subject ^{:line 189 :file "cli/message-routing.bclj"} (agent-subject dead-control)
   repo ^{:line 190 :file "cli/message-routing.bclj"} (north.coord/resolved! port subject "repo")
   role ^{:line 191 :file "cli/message-routing.bclj"} (north.coord/resolved! port subject "role")]
  ^{:line 192 :file "cli/message-routing.bclj"} (if ^{:line 192 :file "cli/message-routing.bclj"} (and ^{:line 192 :file "cli/message-routing.bclj"} (not ^{:line 192 :file "cli/message-routing.bclj"} (str/blank? repo)) ^{:line 192 :file "cli/message-routing.bclj"} (not ^{:line 192 :file "cli/message-routing.bclj"} (str/blank? role))) ^{:line 192 :file "cli/message-routing.bclj"} (do
  ^{:line 193 :file "cli/message-routing.bclj"} (loop [after nil
   seen 0]
  ^{:line 195 :file "cli/message-routing.bclj"} (if ^{:line 195 :file "cli/message-routing.bclj"} (< seen max-route-candidates) ^{:line 195 :file "cli/message-routing.bclj"} (do
  ^{:line 196 :file "cli/message-routing.bclj"} (let [page ^{:line 196 :file "cli/message-routing.bclj"} (north.coord/query-page! port ^{:line 197 :file "cli/message-routing.bclj"} (route-candidate-query repo role) route-page-size after)
   rows ^{:line 199 :file "cli/message-routing.bclj"} (:rows page)
   candidates ^{:line 200 :file "cli/message-routing.bclj"} (->> rows ^{:line 201 :file "cli/message-routing.bclj"} (mapv first) ^{:line 202 :file "cli/message-routing.bclj"} (filterv ^{:line 202 :file "cli/message-routing.bclj"} (fn [^String candidate] ^{:line 203 :file "cli/message-routing.bclj"} (str/starts-with? candidate agent-prefix))) ^{:line 204 :file "cli/message-routing.bclj"} (mapv bare-agent) ^{:line 205 :file "cli/message-routing.bclj"} (filterv ^{:line 205 :file "cli/message-routing.bclj"} (fn [candidate] ^{:line 206 :file "cli/message-routing.bclj"} (not ^{:line 206 :file "cli/message-routing.bclj"} (= dead-control candidate)))) distinct sort)]
  ^{:line 209 :file "cli/message-routing.bclj"} (or ^{:line 209 :file "cli/message-routing.bclj"} (first ^{:line 210 :file "cli/message-routing.bclj"} (filterv ^{:line 210 :file "cli/message-routing.bclj"} (fn [candidate] ^{:line 211 :file "cli/message-routing.bclj"} (recipient-live? port candidate)) candidates)) ^{:line 213 :file "cli/message-routing.bclj"} (if ^{:line 213 :file "cli/message-routing.bclj"} (not ^{:line 213 :file "cli/message-routing.bclj"} (:done? page)) ^{:line 213 :file "cli/message-routing.bclj"} (do
  ^{:line 214 :file "cli/message-routing.bclj"} (recur ^{:line 214 :file "cli/message-routing.bclj"} (:cursor page) ^{:line 214 :file "cli/message-routing.bclj"} (+ seen ^{:line 214 :file "cli/message-routing.bclj"} (count rows)))))))))))))
  (catch Exception _
    nil)))

^{:line 217 :file "cli/message-routing.bclj"} (defn require-live-address
  "Return the concrete send-time route or a structured dead-recipient result.\n   Read failures are distinguished from genuine negative liveness." [port ^String address]
  ^{:line 222 :file "cli/message-routing.bclj"} (try
  ^{:line 223 :file "cli/message-routing.bclj"} (let [route ^{:line 223 :file "cli/message-routing.bclj"} (resolve-address port address)
   recipient ^{:line 224 :file "cli/message-routing.bclj"} (:recipient route)]
  ^{:line 225 :file "cli/message-routing.bclj"} (cond
  ^{:line 226 :file "cli/message-routing.bclj"} (= :broadcast ^{:line 226 :file "cli/message-routing.bclj"} (:kind route)) ^{:line 226 :file "cli/message-routing.bclj"} (assoc route :live true)
  ^{:line 227 :file "cli/message-routing.bclj"} (recipient-live? port recipient) ^{:line 227 :file "cli/message-routing.bclj"} (assoc route :live true)
  :else ^{:line 228 :file "cli/message-routing.bclj"} (assoc route :live false :alternative ^{:line 230 :file "cli/message-routing.bclj"} (live-same-route port recipient))))
  (catch Exception error
    ^{:line 232 :file "cli/message-routing.bclj"} {:address address :live :unavailable :error error})))

^{:line 234 :file "cli/message-routing.bclj"} (defn- mail-candidate-query []
  ^{:line 235 :file "cli/message-routing.bclj"} {:find "mail_candidate" :strata ^{:line 237 :file "cli/message-routing.bclj"} [^{:line 237 :file "cli/message-routing.bclj"} [^{:line 237 :file "cli/message-routing.bclj"} {:head ^{:line 237 :file "cli/message-routing.bclj"} {:rel "mail_settled" :args ^{:line 237 :file "cli/message-routing.bclj"} [^{:line 237 :file "cli/message-routing.bclj"} {:var "message"}]} :body ^{:line 238 :file "cli/message-routing.bclj"} [^{:line 238 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 239 :file "cli/message-routing.bclj"} [^{:line 239 :file "cli/message-routing.bclj"} {:var "message"} "acked_by" ^{:line 239 :file "cli/message-routing.bclj"} {:var "recipient"}]}]} ^{:line 240 :file "cli/message-routing.bclj"} {:head ^{:line 240 :file "cli/message-routing.bclj"} {:rel "mail_settled" :args ^{:line 240 :file "cli/message-routing.bclj"} [^{:line 240 :file "cli/message-routing.bclj"} {:var "message"}]} :body ^{:line 241 :file "cli/message-routing.bclj"} [^{:line 241 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 242 :file "cli/message-routing.bclj"} [^{:line 242 :file "cli/message-routing.bclj"} {:var "message"} "delivery_rejected_by" ^{:line 243 :file "cli/message-routing.bclj"} {:var "recipient"}]}]}] ^{:line 244 :file "cli/message-routing.bclj"} [^{:line 244 :file "cli/message-routing.bclj"} {:head ^{:line 244 :file "cli/message-routing.bclj"} {:rel "mail_candidate" :args ^{:line 245 :file "cli/message-routing.bclj"} [^{:line 245 :file "cli/message-routing.bclj"} {:var "message"} ^{:line 245 :file "cli/message-routing.bclj"} {:var "from"} ^{:line 245 :file "cli/message-routing.bclj"} {:var "to"} ^{:line 245 :file "cli/message-routing.bclj"} {:var "sent"}]} :body ^{:line 246 :file "cli/message-routing.bclj"} [^{:line 246 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 246 :file "cli/message-routing.bclj"} [^{:line 246 :file "cli/message-routing.bclj"} {:var "message"} "from" ^{:line 246 :file "cli/message-routing.bclj"} {:var "from"}]} ^{:line 247 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 247 :file "cli/message-routing.bclj"} [^{:line 247 :file "cli/message-routing.bclj"} {:var "message"} "to" ^{:line 247 :file "cli/message-routing.bclj"} {:var "to"}]} ^{:line 248 :file "cli/message-routing.bclj"} {:rel "triple" :args ^{:line 248 :file "cli/message-routing.bclj"} [^{:line 248 :file "cli/message-routing.bclj"} {:var "message"} "sent_at" ^{:line 248 :file "cli/message-routing.bclj"} {:var "sent"}]} ^{:line 249 :file "cli/message-routing.bclj"} {:rel "mail_settled" :args ^{:line 250 :file "cli/message-routing.bclj"} [^{:line 250 :file "cli/message-routing.bclj"} {:var "message"}] :neg true}]}]]})

^{:line 253 :file "cli/message-routing.bclj"} (defn mail-candidates [port]
  ^{:line 254 :file "cli/message-routing.bclj"} (loop [after nil
   seen 0
   rows ^{:line 256 :file "cli/message-routing.bclj"} []]
  ^{:line 257 :file "cli/message-routing.bclj"} (let [page ^{:line 257 :file "cli/message-routing.bclj"} (north.coord/query-page! port ^{:line 258 :file "cli/message-routing.bclj"} (mail-candidate-query) mail-page-size after)
   page-rows ^{:line 259 :file "cli/message-routing.bclj"} (:rows page)
   next-seen ^{:line 260 :file "cli/message-routing.bclj"} (+ seen ^{:line 260 :file "cli/message-routing.bclj"} (count page-rows))]
  ^{:line 261 :file "cli/message-routing.bclj"} (if ^{:line 261 :file "cli/message-routing.bclj"} (> next-seen max-mail-candidates) ^{:line 261 :file "cli/message-routing.bclj"} (do
  ^{:line 262 :file "cli/message-routing.bclj"} (throw ^{:line 263 :file "cli/message-routing.bclj"} (ex-info "dead-letter scan exceeds its bounded mail corpus" ^{:line 264 :file "cli/message-routing.bclj"} {:type :mail-candidate-overflow :max max-mail-candidates}))))
  ^{:line 266 :file "cli/message-routing.bclj"} (let [all ^{:line 266 :file "cli/message-routing.bclj"} (into rows page-rows)]
  ^{:line 267 :file "cli/message-routing.bclj"} (if ^{:line 267 :file "cli/message-routing.bclj"} (:done? page) all ^{:line 268 :file "cli/message-routing.bclj"} (recur ^{:line 268 :file "cli/message-routing.bclj"} (:cursor page) next-seen all))))))

^{:line 271 :file "cli/message-routing.bclj"} (defn- epoch-ms [instant]
  ^{:line 272 :file "cli/message-routing.bclj"} (.toEpochMilli instant))

^{:line 274 :file "cli/message-routing.bclj"} (defn age-ms [now ^String sent]
  ^{:line 277 :file "cli/message-routing.bclj"} (try
  ^{:line 278 :file "cli/message-routing.bclj"} (max 0 ^{:line 278 :file "cli/message-routing.bclj"} (- ^{:line 278 :file "cli/message-routing.bclj"} (epoch-ms now) ^{:line 279 :file "cli/message-routing.bclj"} (epoch-ms ^{:line 279 :file "cli/message-routing.bclj"} (java.time.Instant/parse sent))))
  (catch Exception _
    nil)))

^{:line 282 :file "cli/message-routing.bclj"} (defn ^String human-age [milliseconds]
  ^{:line 283 :file "cli/message-routing.bclj"} (if ^{:line 283 :file "cli/message-routing.bclj"} (nil? milliseconds) "unknown" ^{:line 285 :file "cli/message-routing.bclj"} (let [seconds ^{:line 285 :file "cli/message-routing.bclj"} (quot milliseconds 1000)]
  ^{:line 286 :file "cli/message-routing.bclj"} (cond
  ^{:line 287 :file "cli/message-routing.bclj"} (< seconds 60) ^{:line 287 :file "cli/message-routing.bclj"} (str seconds "s")
  ^{:line 288 :file "cli/message-routing.bclj"} (< seconds 3600) ^{:line 288 :file "cli/message-routing.bclj"} (str ^{:line 288 :file "cli/message-routing.bclj"} (quot seconds 60) "m")
  ^{:line 289 :file "cli/message-routing.bclj"} (< seconds 86400) ^{:line 289 :file "cli/message-routing.bclj"} (str ^{:line 289 :file "cli/message-routing.bclj"} (quot seconds 3600) "h")
  :else ^{:line 290 :file "cli/message-routing.bclj"} (str ^{:line 290 :file "cli/message-routing.bclj"} (quot seconds 86400) "d")))))

^{:line 292 :file "cli/message-routing.bclj"} (defn dead-letter-scan
  "Pending human mail whose concrete recipient has neither a live lease nor an\n   armed provider listener. Returns a strict diagnostic envelope."
  ([port]
    ^{:line 296 :file "cli/message-routing.bclj"} (dead-letter-scan port ^{:line 296 :file "cli/message-routing.bclj"} (java.time.Instant/now)))
  ([port now]
    ^{:line 299 :file "cli/message-routing.bclj"} (try
  ^{:line 300 :file "cli/message-routing.bclj"} {:rows ^{:line 301 :file "cli/message-routing.bclj"} (->> ^{:line 301 :file "cli/message-routing.bclj"} (mail-candidates port) ^{:line 302 :file "cli/message-routing.bclj"} (keep ^{:line 303 :file "cli/message-routing.bclj"} (fn [[message from to sent]] ^{:line 304 :file "cli/message-routing.bclj"} (if ^{:line 304 :file "cli/message-routing.bclj"} (and ^{:line 304 :file "cli/message-routing.bclj"} (str/starts-with? message "@msg:") ^{:line 305 :file "cli/message-routing.bclj"} (not= broadcast-address to)) ^{:line 304 :file "cli/message-routing.bclj"} (do
  ^{:line 306 :file "cli/message-routing.bclj"} (let [route ^{:line 306 :file "cli/message-routing.bclj"} (resolve-address port to)
   recipient ^{:line 307 :file "cli/message-routing.bclj"} (:recipient route)]
  ^{:line 308 :file "cli/message-routing.bclj"} (if ^{:line 308 :file "cli/message-routing.bclj"} (not ^{:line 308 :file "cli/message-routing.bclj"} (recipient-live? port recipient)) ^{:line 308 :file "cli/message-routing.bclj"} (do
  ^{:line 309 :file "cli/message-routing.bclj"} (let [milliseconds ^{:line 309 :file "cli/message-routing.bclj"} (age-ms now sent)]
  ^{:line 310 :file "cli/message-routing.bclj"} {:message message :sender from :recipient to :resolved-recipient recipient :age-ms milliseconds :age ^{:line 315 :file "cli/message-routing.bclj"} (human-age milliseconds)})))))))) ^{:line 316 :file "cli/message-routing.bclj"} (sort-by ^{:line 316 :file "cli/message-routing.bclj"} (juxt ^{:line 316 :file "cli/message-routing.bclj"} (fn [%1] ^{:line 316 :file "cli/message-routing.bclj"} (or ^{:line 316 :file "cli/message-routing.bclj"} (:age-ms %1) -1)) :message) ^{:line 317 :file "cli/message-routing.bclj"} (fn [left right] ^{:line 320 :file "cli/message-routing.bclj"} (compare right left))) vec)}
  (catch Exception error
    ^{:line 323 :file "cli/message-routing.bclj"} {:error ^{:line 323 :file "cli/message-routing.bclj"} (.getMessage error)}))))

^{:line 325 :file "cli/message-routing.bclj"} (defn readiness-dead-letter-scan
  "Bounded doctor projection. Only messages inside ACTION-WINDOW-MS pay\n   recipient routing/liveness reads; older pending mail remains visible as\n   unacknowledged history."
  ([port action-window-ms]
    ^{:line 331 :file "cli/message-routing.bclj"} (readiness-dead-letter-scan port action-window-ms ^{:line 332 :file "cli/message-routing.bclj"} (java.time.Instant/now)))
  ([port action-window-ms now]
    ^{:line 336 :file "cli/message-routing.bclj"} (try
  ^{:line 337 :file "cli/message-routing.bclj"} {:rows ^{:line 338 :file "cli/message-routing.bclj"} (->> ^{:line 338 :file "cli/message-routing.bclj"} (mail-candidates port) ^{:line 339 :file "cli/message-routing.bclj"} (keep ^{:line 340 :file "cli/message-routing.bclj"} (fn [[message from to sent]] ^{:line 341 :file "cli/message-routing.bclj"} (if ^{:line 341 :file "cli/message-routing.bclj"} (and ^{:line 341 :file "cli/message-routing.bclj"} (str/starts-with? message "@msg:") ^{:line 342 :file "cli/message-routing.bclj"} (not= broadcast-address to)) ^{:line 341 :file "cli/message-routing.bclj"} (do
  ^{:line 343 :file "cli/message-routing.bclj"} (let [milliseconds ^{:line 343 :file "cli/message-routing.bclj"} (age-ms now sent)
   base ^{:line 344 :file "cli/message-routing.bclj"} {:message message :sender from :recipient to :age-ms milliseconds :age ^{:line 348 :file "cli/message-routing.bclj"} (human-age milliseconds)}]
  ^{:line 349 :file "cli/message-routing.bclj"} (if ^{:line 349 :file "cli/message-routing.bclj"} (and milliseconds ^{:line 350 :file "cli/message-routing.bclj"} (>= milliseconds action-window-ms)) ^{:line 351 :file "cli/message-routing.bclj"} (assoc base :resolved-recipient to :historical? true) ^{:line 354 :file "cli/message-routing.bclj"} (let [route ^{:line 354 :file "cli/message-routing.bclj"} (resolve-address port to)
   recipient ^{:line 355 :file "cli/message-routing.bclj"} (:recipient route)]
  ^{:line 356 :file "cli/message-routing.bclj"} (if ^{:line 356 :file "cli/message-routing.bclj"} (not ^{:line 356 :file "cli/message-routing.bclj"} (recipient-live? port recipient)) ^{:line 356 :file "cli/message-routing.bclj"} (do
  ^{:line 357 :file "cli/message-routing.bclj"} (assoc base :resolved-recipient recipient)))))))))) ^{:line 358 :file "cli/message-routing.bclj"} (sort-by ^{:line 358 :file "cli/message-routing.bclj"} (juxt ^{:line 358 :file "cli/message-routing.bclj"} (fn [%1] ^{:line 358 :file "cli/message-routing.bclj"} (or ^{:line 358 :file "cli/message-routing.bclj"} (:age-ms %1) -1)) :message) ^{:line 359 :file "cli/message-routing.bclj"} (fn [left right] ^{:line 362 :file "cli/message-routing.bclj"} (compare right left))) vec)}
  (catch Exception error
    ^{:line 365 :file "cli/message-routing.bclj"} {:error ^{:line 365 :file "cli/message-routing.bclj"} (.getMessage error)}))))
