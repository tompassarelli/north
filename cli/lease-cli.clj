;; lease-cli.clj — agent-side North lease helper (P0 shadow).
;; Speaks canonical typed FRAMRPC lease operations. Human output remains EDN;
;; --json is the exact machine envelope used by the Linear bridge.
;; This is the contract every agent session uses to take the build mutex over the socket
;; INSTEAD of dropping a per-agent BUILD-LOCK-<agent>.md lockfile.
;;
;; usage:
;;   bb lease-cli.clj <port> [--json] acquire <res> <holder> <ttl-ms>
;;   bb lease-cli.clj <port> [--json] renew  <res> <holder> <epoch> <ttl-ms>
;;   bb lease-cli.clj <port> [--json] release <res> <holder> <epoch>
;;   bb lease-cli.clj <port> [--json] fence   <res> <holder> <epoch>
;;   printf %s <value> | bb lease-cli.clj <port> [--json] put-fenced-stdin <res> <holder> <epoch> <subject> <predicate>
;;   bb lease-cli.clj <port> [--json] status
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

;; Shared coordination substrate: typed lease operations live once in cli/coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(defn fail! [message]
  (binding [*out* *err*] (println (str "lease-cli: " message)))
  (System/exit 2))

(defn positive-long [label raw]
  (let [value (try (Long/parseLong (str raw))
                   (catch Exception _ (fail! (str label " must be a positive integer"))))]
    (when-not (pos? value) (fail! (str label " must be a positive integer")))
    value))

(defn required-text [label value]
  (when (str/blank? value) (fail! (str label " must not be blank")))
  value)

;; Stay below both FRAMRPC's body cap and Linux's per-argument ceiling.
(def max-fenced-value-bytes (* 160 1024))

(defn read-fenced-value []
  (let [bytes (.readNBytes System/in (inc max-fenced-value-bytes))]
    (when (> (alength bytes) max-fenced-value-bytes)
      (fail! (str "fenced value exceeds " max-fenced-value-bytes " bytes")))
    (try
      (let [decoder
            (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
              (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
              (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
        (str (.decode decoder (java.nio.ByteBuffer/wrap bytes))))
      (catch java.nio.charset.CharacterCodingException _
        (fail! "fenced value must be valid UTF-8")))))

(defn public-result [verb result]
  (if-let [reject (:reject result)]
    {:reject reject :version (or (:version result) (:served-version result))}
    (case verb
      "acquire"
      {:ok (:epoch result) :holder (:holder result)
       :epoch (:epoch result) :exp (:exp result)}

      "renew"
      {:ok (:epoch result) :holder (:holder result)
       :epoch (:epoch result) :exp (:exp result)}

      "release"
      (cond-> {:ok (:ok result)}
        (false? (:released? result)) (assoc :noop true))

      "fence" {:fence-ok (boolean (:valid? result))}
      "put-fenced-stdin" {:ok (:ok result)}
      result)))

(let [[port-token maybe-format & tail] *command-line-args*
      json? (= maybe-format "--json")
      [verb & args] (if json? tail (cons maybe-format tail))
      port (positive-long "port" port-token)
      _ (when (> port 65535) (fail! "port must be at most 65535"))
      result
      (case verb
     "acquire"
     (north.coord/acquire-lease!
      port
      (required-text "resource" (nth args 0 nil))
      (required-text "holder" (nth args 1 nil))
      (positive-long "ttl-ms" (nth args 2 nil)))
     "renew"
     (north.coord/renew-lease!
      port
      (north.coord/lease-fence
       (required-text "resource" (nth args 0 nil))
       (required-text "holder" (nth args 1 nil))
       (positive-long "epoch" (nth args 2 nil)))
      (positive-long "ttl-ms" (nth args 3 nil)))
     "release"
     (north.coord/release-lease!
      port
      (north.coord/lease-fence
       (required-text "resource" (nth args 0 nil))
       (required-text "holder" (nth args 1 nil))
       (positive-long "epoch" (nth args 2 nil))))
     "fence"
     (north.coord/check-lease!
      port
      (north.coord/lease-fence
       (required-text "resource" (nth args 0 nil))
       (required-text "holder" (nth args 1 nil))
       (positive-long "epoch" (nth args 2 nil))))
     "put-fenced-stdin"
     (north.coord/put-with-fence!
      port
      {:resource (required-text "resource" (nth args 0 nil))
       :holder (required-text "holder" (nth args 1 nil))
       :epoch (positive-long "epoch" (nth args 2 nil))}
      (required-text "subject" (nth args 3 nil))
      (required-text "predicate" (nth args 4 nil))
      (read-fenced-value))
     "status"
     (north.coord/status port)
     (fail! (str "unknown verb: " verb)))
      result (public-result verb result)]
  (if json?
    (println (json/generate-string result))
    (prn result)))
