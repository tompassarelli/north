;; Provider-independent configuration readers that remain outside agent activation.

(ns north.harness-dial
  (:require [clojure.string :as str]))

(defn env-decision
  "Kill-switch env value → resolve-dial env decision. Pure, so the shared case
   table can assert it without mutating the process environment."
  [raw]
  (cond
    (or (nil? raw) (str/blank? raw)) nil
    (#{"0" "false"} raw) "on"
    :else "off"))

(defn authoring-env
  "The canonical authoring kill-switch env var as a resolve-dial decision."
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
