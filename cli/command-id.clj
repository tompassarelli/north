(ns north.command-id
  (:require [clojure.string :as str]))

(defn utf8-length [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn canonical-frame [tag values]
  (str tag
       "["
       (apply str
              (map (fn [value]
                     (let [value (str value)]
                       (str (utf8-length value) ":" value)))
                   values))
       "]"))

(declare canonical-value)

(defn canonical-value
  "Recursive, type-delimited canonical EDN projection for command identity.
   Map and set iteration order is erased; sequence order and scalar types remain
   authority-bearing."
  [value]
  (cond
    (nil? value) (canonical-frame "nil" [])
    (boolean? value) (canonical-frame "bool" [(if value "true" "false")])
    (string? value) (canonical-frame "string" [value])
    (keyword? value)
    (canonical-frame "keyword" [(or (namespace value) "") (name value)])
    (symbol? value)
    (canonical-frame "symbol" [(or (namespace value) "") (name value)])
    (char? value) (canonical-frame "char" [(str value)])
    (number? value)
    (canonical-frame "number"
                     [(.getName (class value)) (pr-str value)])
    (map? value)
    (canonical-frame
     "map"
     (sort
      (map (fn [[key item]]
             (canonical-frame
              "entry"
              [(canonical-value key) (canonical-value item)]))
           value)))
    (set? value)
    (canonical-frame "set" (sort (map canonical-value value)))
    (vector? value)
    (canonical-frame "vector" (map canonical-value value))
    (list? value)
    (canonical-frame "list" (map canonical-value value))
    (sequential? value)
    (canonical-frame "sequence" (map canonical-value value))
    :else
    (throw (ex-info "unsupported value in command idempotency preimage"
                    {:type :unsupported-command-id-value
                     :class (.getName (class value))}))))

(defn content-id
  "Stable full-entropy ID only when a caller explicitly supplies an
   idempotency key. The versioned preimage is recursively canonical."
  [op args target idempotency-key]
  (let [canon
        (canonical-frame
         "north-command-id-v2"
         [(canonical-value (str idempotency-key))
          (canonical-value (str op))
          (canonical-value args)
          (canonical-value (str target))])
        bs
        (.digest (java.security.MessageDigest/getInstance "SHA-256")
                 (.getBytes canon java.nio.charset.StandardCharsets/UTF_8))]
    (apply str
           (map #(format "%02x" (bit-and (int %) 0xff)) bs))))

(defn command-id [op args target idempotency-key]
  (if (str/blank? (str idempotency-key))
    (str (java.util.UUID/randomUUID))
    (content-id op args target idempotency-key)))
