(ns north.dispatch-mode
  "Canonical dispatch-mode vocabulary and interpretation.

   This is policy data, not persisted state. Callers supply the stored token;
   known legacy aliases normalize to a canonical mode and every other unknown
   value fails closed."
  (:require [clojure.string :as str]))

(def default-mode "managed-forced")

(def mode-specs
  [{:name "native-forced"
   :execution "native"
    :enforcement "forced"
    :guard-action "allow"
    :managed-admission "deny"
    :summary "raw provider-native spawns, no interference"
    :help "no interference. For A/B baselines against stock provider behavior."}
   {:name "native-biased"
    :execution "native"
    :enforcement "biased"
    :guard-action "remind-native"
    :managed-admission "warn-native"
    :summary "native allowed, soft reminder that managed dispatch exists"
    :help "native spawns allowed; a soft reminder that managed dispatch exists."}
   {:name "managed-biased"
    :execution "managed"
    :enforcement "biased"
    :guard-action "remind-managed"
    :managed-admission "allow"
    :summary "native allowed, nudged toward the North SDK"
    :help "native spawns allowed; remind the caller to prefer the North SDK."}
   {:name "managed-forced"
    :execution "managed"
    :enforcement "forced"
    :guard-action "deny"
    :managed-admission "allow"
    :summary "native Agent/Workflow denied; use the North SDK"
    :help "native Agent/Task/Workflow calls are denied and redirected to the North SDK."}])

(def legacy-aliases
  {"native" "native-forced"
   "warn" "managed-biased"
   "north" "managed-forced"})

(def ^:private specs-by-name
  (into {} (map (juxt :name identity) mode-specs)))

(defn canonical-names []
  (mapv :name mode-specs))

(defn usage []
  (str/join "|" (canonical-names)))

(defn recognized? [value]
  (or (contains? specs-by-name value)
      (contains? legacy-aliases value)))

(defn legacy-alias? [value]
  (contains? legacy-aliases value))

(defn normalize [value]
  (if-let [canonical (or (when (contains? specs-by-name value) value)
                         (get legacy-aliases value))]
    canonical
    (throw
     (ex-info
      (str "invalid dispatch mode " (pr-str value)
           "; expected " (usage)
           " (legacy aliases: native|warn|north)")
      {:value value
       :canonical (canonical-names)
       :legacy (keys legacy-aliases)}))))

(defn spec [value]
  (get specs-by-name (normalize value)))

(defn guard-action [value]
  (:guard-action (spec value)))

(defn managed-admission [value]
  (:managed-admission (spec value)))

(defn grid []
  (str/join
   "\n"
   (map (fn [{:keys [name execution enforcement]}]
          (format "  %-17s type=%-7s enforce=%s"
                  name execution enforcement))
        mode-specs)))
