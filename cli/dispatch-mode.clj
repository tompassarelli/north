(ns north.dispatch-mode
  "Canonical dispatch-mode vocabulary and interpretation.

   This is policy data, not persisted state. Callers supply the stored token;
   anything outside the canonical vocabulary fails closed."
  (:require [clojure.string :as str]))

(def default-mode "managed")

(def mode-specs
  [{:name "native"
    :selection "pinned"
    :guard-action "allow"
    :managed-admission "deny"
    :summary "provider-native surface pinned"
    :help "pin every dispatch to the provider-native Agent/Task/Workflow surface."}
   {:name "managed"
    :selection "pinned"
    :guard-action "deny"
    :managed-admission "allow"
    :summary "North-managed dispatch surface pinned"
    :help "pin every dispatch to North; provider-native agent calls are denied."}
   {:name "auto"
    :selection "learning-regime"
    :guard-action "allow"
    :managed-admission "allow"
    :summary "dispatch resolves per run via the learning regime"
    :help "resolve per dispatch: frozen is deterministic known-best; learning permits bounded experiments."}])

(def ^:private specs-by-name
  (into {} (map (juxt :name identity) mode-specs)))

(defn canonical-names []
  (mapv :name mode-specs))

(defn usage []
  (str/join "|" (canonical-names)))

(defn recognized? [value]
  (contains? specs-by-name value))

(defn normalize [value]
  (if (recognized? value)
    value
    (throw
     (ex-info
      (str "invalid dispatch mode " (pr-str value) "; expected " (usage))
      {:value value
       :canonical (canonical-names)}))))

(defn spec [value]
  (get specs-by-name (normalize value)))

(defn guard-action [value]
  (:guard-action (spec value)))

(defn managed-admission [value]
  (:managed-admission (spec value)))

(defn grid []
  (str/join
   "\n"
   (map (fn [{:keys [name selection]}]
          (format "  %-7s selection=%s" name selection))
        mode-specs)))
