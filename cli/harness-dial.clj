;; harness-dial.clj — the Clojure reader of the control-plane dial.
;;
;; This is the second of three implementations of one precedence rule (bash for
;; enforcement, this for the report, TypeScript for the SDK). They are held
;; together by agent-profile/hooks/harness-dial-cases.tsv, which all three
;; assert against; a divergence is a test failure rather than a silent
;; disagreement between what the guard does and what the report claims.
;;
;; Rule: item > category > all > default(on). Env beats state entirely. An
;; `off:until=<iso>` whose deadline has passed reads as `on` at its own level
;; and stops the search, so a lapsed TTL restores the guard.

(ns north.harness-dial
  (:require [clojure.string :as str]
            [clojure.java.io :as io]))

(def ^:private iso-instant #"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z")

(defn now-iso
  "Canonical instant. Same shape as a stored until=, so lexicographic
   comparison is chronological comparison in every implementation."
  []
  (.format (java.time.format.DateTimeFormatter/ofPattern "yyyy-MM-dd'T'HH:mm:ss'Z'")
           (java.time.ZonedDateTime/now java.time.ZoneOffset/UTC)))

(defn decide-level
  "on | off for a level that decides, nil for a level that defers outward."
  [raw now]
  (cond
    (or (nil? raw) (str/blank? raw)) nil
    (= raw "on") "on"
    (= raw "off") "off"
    (str/starts-with? raw "off:until=")
    (let [deadline (subs raw (count "off:until="))]
      ;; An unreadable deadline must never hold a guard down.
      (if (and (re-matches iso-instant deadline) (neg? (compare now deadline)))
        "off"
        "on"))
    :else "on"))

(defn resolve-dial
  "Returns [verdict decided-by]. decided-by names the level that settled it,
   which is what `explain` prints."
  ([all cat item env] (resolve-dial all cat item env (now-iso)))
  ([all cat item env now]
   (if (and env (not (str/blank? env)))
     [env "env"]
     (or (first (keep (fn [[level raw]]
                        (when-let [verdict (decide-level raw now)]
                          [verdict level]))
                      [["item" item] ["category" cat] ["all" all]]))
         ["on" "default"]))))

(defn env-decision
  "Kill-switch env value → resolve-dial env decision. Pure, so the shared case
   table can assert it without mutating the process environment."
  [raw]
  (cond
    (or (nil? raw) (str/blank? raw)) nil
    (#{"0" "false"} raw) "on"
    :else "off"))

(defn authoring-env
  "The authoring kill-switch env vars as a resolve-dial env decision.
   AGENT_ is canonical; CLAUDE_ remains a compatibility alias. Reading only
   the alias — as this report did until 2026-07-30 — makes the report claim
   guards are live in a session where they are not."
  []
  (env-decision (not-empty (or (System/getenv "AGENT_NO_AUTHORING_HOOKS") ""))))

;; --- communications protocol ---------------------------------------------

(def comms-protocols #{"off" "db" "file" "both"})
(def comms-surfaces #{"native" "managed"})

(defn comms-selection
  "Resolve the protocol for one execution surface. `get-state` is
   (fn [key] raw-or-nil). Malformed state fails closed to today's db/forced
   behavior; the config writer rejects malformed values before persistence."
  [get-state surface]
  (let [surface (if (comms-surfaces surface) surface "native")
        raw-base (get-state "comms")
        base (if (comms-protocols raw-base) raw-base "db")
        raw-override (get-state (str "comms." surface))
        selected (if (comms-protocols raw-override) raw-override base)
        raw-enforcement (get-state "comms.enforcement")
        enforcement (if (#{"forced" "biased"} raw-enforcement)
                      raw-enforcement
                      "forced")]
    {:surface surface
     :base base
     :override (if (comms-protocols raw-override) raw-override "inherit")
     :selected selected
     :enforcement enforcement}))

;; --- registry -------------------------------------------------------------

(defn registry-path [home]
  (or (System/getenv "NORTH_HOOK_REGISTRY")
      (str home "/.agents/hooks/registry.tsv")))

(defn read-registry
  "Parsed hook inventory, or [] when the table is absent."
  [path]
  (if-let [content (try (slurp path) (catch Exception _ nil))]
    (->> (str/split-lines content)
         (remove #(or (str/blank? %) (str/starts-with? % "#")))
         (map #(str/split % #"\t"))
         (remove #(= "id" (first %)))
         (mapv (fn [[id category kind in-all ttl-req rel-path events]]
                 {:id id
                  :category category
                  :kind kind
                  :in-all? (= "yes" in-all)
                  :ttl-required? (= "yes" ttl-req)
                  :path rel-path
                  :events (or events "")})))
    []))

(defn category-of [registry id]
  (some #(when (= id (:id %)) (:category %)) registry))

(defn hook-verdict
  "Resolve one hook against state. `get-state` is (fn [key] raw-or-nil).
   Mirrors north_hook_enabled in lib/harness-dial.sh exactly."
  [get-state registry id]
  (let [category (category-of registry id)
        ;; The env kill-switch speaks only for authoring guards; it must not
        ;; reach across and silence dispatch or coordination.
        env (when (= category "authoring") (authoring-env))
        ;; coordination is never swept by `all` — it must be named.
        all (when-not (= category "coordination") (get-state "hooks"))
        cat (cond
              ;; `guards` IS the authoring category's storage. One key, no
              ;; migration, and `north config guards off` keeps its meaning.
              (= category "authoring") (get-state "guards")
              (some? category) (get-state (str "hooks.cat." category))
              :else nil)
        item (get-state (str "hooks.hook." id))]
    (resolve-dial all cat item env)))
