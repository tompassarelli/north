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
