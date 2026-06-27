(ns bridge.rt
  "Host-interop runtime for the lodestar-web bridge's Beagle module — the
  irreducible Clojure layer the .bclj `declare-extern`s bind to. Beagle
  (bridge.bclj -> bridge.clj) owns the typed logic: the graph fold, federation
  merge, presence/timetape projection, routing. THIS owns the host calls:
  line-delimited EDN/TCP to the fram daemons, http-kit server + WS channels,
  cheshire JSON, babashka.process, file IO, java.time. Keep NO business logic
  here — only host shims. (Self-host recipe, mirrors fram.rt.)"
  (:require [org.httpkit.server :as http]
            [cheshire.core :as json]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [babashka.process :as p])
  (:import [java.net Socket]))

;; bridge/rt.clj -> parent=bridge -> parent=repo root (matches the original's *file* walk).
(def ROOT (-> *file* io/file .getCanonicalFile .getParentFile .getParentFile))
(def WEB  (io/file ROOT "web"))
(def FLEET-DATA (io/file (System/getProperty "user.home") "code/fleet-data"))
(def MSG-CLI (io/file (System/getProperty "user.home") "code/beagle/.scratch/msg-cli.clj"))
(def DISTILLER (io/file (System/getProperty "user.home") "code/fram-lease/bin/fram-distill"))

;; ---- fram wire (one request per connection, one EDN line back) --------------
(defn send-op [port op]
  (with-open [s (Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w (.getBytes (str (pr-str op) "\n"))) (.flush w)
      (edn/read-string (.readLine r)))))

(def ALL-TRIPLES
  {:op :query
   :query {:find "t"
           :rules [{:head {:rel "t" :args [{:var "s"} {:var "p"} {:var "o"}]}
                    :body [{:rel "triple" :args [{:var "s"} {:var "p"} {:var "o"}]}]}]}})

;; ---- extern surface (typed accessors the .bclj binds to) --------------------

;; ALL triples as [s p o] string-vectors; nil when the daemon is unreachable
;; (lets Beagle's federation skip a dead daemon without try/catch).
(defn all-triples [port]
  (try
    (->> (:ok (send-op port ALL-TRIPLES))
         (mapv (fn [[s p o]] [(str s) (str p) (str o)])))
    (catch Exception _ nil)))

;; [[session-entity handle] ...] — ONE row per uuid. `agent` is overloaded: it anchors
;; @session:<h> (the session) AND every @run:<sid> (cost record). The bare query returned one row
;; per RUN (87 today) — dedup-presence collapsed by uuid but still kept @run-only phantoms whose
;; session was forgotten. Scope to @session:* (exactly one per live handle) so the roster is the
;; real agents, lease-judged. Mirrors presence-cli.clj's `sessions`.
(defn agents [port]
  (try
    (->> (:ok (send-op port {:op :query
                             :query {:find "s"
                                     :rules [{:head {:rel "s" :args [{:var "e"} {:var "h"}]}
                                              :body [{:rel "triple" :args [{:var "e"} "agent" {:var "h"}]}]}]}}))
         (filter (fn [[e _]] (str/starts-with? (str e) "@session:")))
         (reduce (fn [m [e h]] (assoc m (str h) [(str e) (str h)])) {})
         vals
         vec)
    (catch Exception _ [])))

;; cost-so-far per agent handle: sum cost_usd across @run:* grouped by `agent`.
(defn agent-costs [port]
  (try
    (let [rows (:ok (send-op port
                      {:op :query
                       :query {:find "r"
                               :rules [{:head {:rel "r" :args [{:var "run"} {:var "a"} {:var "c"}]}
                                        :body [{:rel "triple" :args [{:var "run"} "agent" {:var "a"}]}
                                               {:rel "triple" :args [{:var "run"} "cost_usd" {:var "c"}]}]}]}}))]
      (reduce (fn [m [_ a c]] (update m (str a) (fnil + 0.0) (or (parse-double (str c)) 0.0)))
              {} (or rows [])))
    (catch Exception _ {})))

(defn resolved [port te ppred]
  (try (let [v (:value (send-op port {:op :resolved :te te :p ppred}))]
         (when (some? v) (str v)))
       (catch Exception _ nil)))

(defn resolved-many [port te ppred]
  (try (mapv str (:values (send-op port {:op :resolved :te te :p ppred})))
       (catch Exception _ [])))

;; ---- control surface: OCC-retried writes through the coordinator ------------
(defn coord-assert! [port te p r]
  (loop [tries 5]
    (let [v   (:version (send-op port {:op :version}))
          res (send-op port {:op :assert :te te :p p :r (str r) :base v})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn coord-retract! [port te p r]
  (loop [tries 5]
    (let [v   (:version (send-op port {:op :version}))
          res (send-op port {:op :retract :te te :p p :r (str r) :base v})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn rejected? [res] (boolean (and (map? res) (:reject res))))

(defn gen-id [kind]
  (str "@" (or kind "work") ":"
       (.format (java.time.LocalDateTime/now)
                (java.time.format.DateTimeFormatter/ofPattern "yyyyMMdd-HHmmss"))
       "-" (format "%04x" (rand-int 0x10000))))

;; ---- per-agent activity stream files ----------------------------------------
(def EDIT-TOOLS #{"Edit" "Write" "MultiEdit" "NotebookEdit"})

;; most-recent file an agent EDITED, tailed from its activity stream.
(defn last-edit-file [uuid]
  (let [f (io/file FLEET-DATA (str "agent-" uuid ".stream.jsonl"))]
    (when (.exists f)
      (->> (str/split-lines (str (:out (p/sh ["tail" "-n" "400" (.getPath f)]))))
           (keep (fn [line]
                   (try
                     (let [c (get-in (json/parse-string line true) [:message :content])]
                       (when (sequential? c)
                         (some (fn [b]
                                 (when (and (= "tool_use" (:type b)) (EDIT-TOOLS (:name b)))
                                   (let [i (:input b)]
                                     (or (:file_path i) (:notebook_path i) (:path i)))))
                               c)))
                     (catch Exception _ nil))))
           last))))

(defn stream-exists? [uuid]
  (.exists (io/file FLEET-DATA (str "agent-" uuid ".stream.jsonl"))))

(defn stream-age-s [uuid]
  (let [f (io/file FLEET-DATA (str "agent-" uuid ".stream.jsonl"))]
    (when (.exists f) (int (/ (- (System/currentTimeMillis) (.lastModified f)) 1000)))))

;; ---- time -------------------------------------------------------------------
(defn now-ms [] (System/currentTimeMillis))

(defn parse-iso-ms [s]
  (when (string? s) (try (.toEpochMilli (java.time.Instant/parse s)) (catch Exception _ nil))))

;; fallback: the @msg:<YYYYMMDD>-<HHMMSS>-… id timestamp, system zone.
(defn parse-id-ms [id]
  (when-let [[_ d t] (re-find #"^@\w+:(\d{8})-(\d{6})" (str id))]
    (try (-> (java.time.LocalDateTime/parse (str d t)
               (java.time.format.DateTimeFormatter/ofPattern "yyyyMMddHHmmss"))
             (.atZone (java.time.ZoneId/systemDefault)) .toInstant .toEpochMilli)
         (catch Exception _ nil))))

;; ---- JSON -------------------------------------------------------------------
(defn to-json [x] (json/generate-string x))
(defn parse-json [s] (json/parse-string s true))
(defn read-json-body [req] (json/parse-string (slurp (:body req)) true))

;; wrap the Beagle route fn so any thrown exception becomes a 500 (the original's
;; handler-level try/catch — kept here so Beagle never touches a raw exception).
(defn guard [route req]
  (try (route req)
       (catch Exception e
         {:status 500 :headers {"Content-Type" "application/json"}
          :body (json/generate-string {:error (.getMessage e)})})))

;; ---- steer: send a message to an agent via msg-cli --------------------------
(defn steer! [port to body]
  (let [subject "steer from web"
        res (p/sh ["bb" (.getPath MSG-CLI) (str port) "send" "lodestar-web" to subject body])]
    {:ok (zero? (:exit res)) :out (str/trim (str (:out res) (:err res)))}))

;; ---- distill: (re)run the decision analyst for one agent into the decisions
;; daemon, on demand from the /distill route's POST (the "Distill" button). The
;; fold of the resulting @decision: claims into the response shape is Beagle's
;; job — this only fires the host process. Failures are swallowed: the route
;; still serves whatever decisions are already in the daemon (never a 500/404).
(defn run-distiller! [agent window port]
  (try
    (let [res (p/sh ["bb" (.getPath DISTILLER) "extract" (str agent)
                     "--window" (str window) "--port" (str port) "--emit" "--quiet"])]
      {:ok (zero? (:exit res))})
    (catch Exception e {:ok false :error (.getMessage e)})))

;; ---- static files -----------------------------------------------------------
(def MIME {"html" "text/html" "js" "application/javascript" "css" "text/css"
           "json" "application/json" "svg" "image/svg+xml" "ico" "image/x-icon"})
(defn- ext [^String n] (let [i (.lastIndexOf n ".")] (when (pos? i) (subs n (inc i)))))

;; ---- query params -----------------------------------------------------------
(defn qparams [^String qs]
  (when qs
    (into {} (for [pair (str/split qs #"&") :let [[k v] (str/split pair #"=" 2)]]
               [(keyword k) (some-> v (java.net.URLDecoder/decode "UTF-8"))]))))

(defn serve-static [uri]
  (let [rel (if (= uri "/") "index.html" (subs uri 1))
        f (io/file WEB rel)]
    (if (and (.exists f) (.isFile f) (str/starts-with? (.getCanonicalPath f) (.getCanonicalPath WEB)))
      {:status 200 :headers {"Content-Type" (get MIME (ext rel) "application/octet-stream")} :body (slurp f)}
      {:status 404 :body "not found"})))

;; ---- live commit feed: fan out to ALL daemons, relay each commit as JSON ----
;; Port list mirrors GRAPHS in bridge.bclj (the federation config); kept here too
;; because this WS plumbing is pure host fan-out.
(def LIVE-GRAPHS [["fleet" 7978] ["code" 7979] ["board" 7977] ["attention" 7980]])
(defn- ref-obj? [o]
  (and (string? o) (str/starts-with? o "@") (not (re-find #"\s" o))))

(defn ws-live [req]
  (http/as-channel req
    {:on-open
     (fn [ch]
       (let [socks (atom [])]
         (http/on-close ch (fn [_]
                             (doseq [s @socks]
                               (try (.close s) (catch Exception _ nil)))))
         (doseq [[name port] LIVE-GRAPHS]
           (future
             (try
               (let [sock (Socket. "127.0.0.1" (int port))]
                 (swap! socks conj sock)
                 (let [w (.getOutputStream sock)
                       r (io/reader (.getInputStream sock))]
                   (.write w (.getBytes (str (pr-str {:op :subscribe}) "\n"))) (.flush w)
                   (.readLine r)                              ; {:subscribed N}
                   (loop []
                     (when-let [line (.readLine r)]
                       (when-let [ev (try (edn/read-string line) (catch Exception _ nil))]
                         (when (and (map? ev) (= :commit (:event ev)))
                           (try
                             (http/send! ch (json/generate-string
                                              {:type "commit" :graph name
                                               :op (:op ev) :l (:l ev)
                                               :p (:p ev) :r (:r ev) :version (:version ev)
                                               :ref (ref-obj? (:r ev))}))
                             (catch Exception _ nil))))
                       (recur)))))
               (catch Exception _ nil))))))}))

;; ---- per-agent activity stream: tail agent-<uuid>.stream.jsonl --------------
(defn ws-stream [req uuid]
  (let [f (io/file FLEET-DATA (str "agent-" uuid ".stream.jsonl"))]
    (http/as-channel req
      {:on-open
       (fn [ch]
         (if-not (.exists f)
           (do (http/send! ch (json/generate-string {:type "error" :error "no stream file"}))
               (http/close ch))
           (let [proc (p/process ["tail" "-n" "200" "-f" (.getPath f)] {:out :stream})]
             (http/on-close ch (fn [_] (try (p/destroy-tree proc) (catch Exception _ nil))))
             (future
               (try
                 (with-open [r (io/reader (:out proc))]
                   (loop []
                     (when-let [line (.readLine r)]
                       (when (seq (str/trim line))
                         (http/send! ch (json/generate-string {:type "line" :raw line})))
                       (recur))))
                 (catch Exception _ nil)
                 (finally (try (p/destroy-tree proc) (catch Exception _ nil))))))))})))

;; ---- boot: read argv, run the http-kit server, block ------------------------
(defn boot! [handler]
  (let [port (or (some-> (first *command-line-args*) parse-long) 8088)]
    (http/run-server handler {:port port})
    (println (str "lodestar-web bridge up  →  http://localhost:" port))
    (println (str "  serving " (.getPath WEB)))
    (println "  lodestar web targets fram daemon :7978 (override per-request with ?port=N)")
    @(promise)))
