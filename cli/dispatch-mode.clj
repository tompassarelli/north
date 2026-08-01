(ns north.dispatch-mode
  "Canonical dispatch-mode vocabulary and interpretation.

   This is policy data, not persisted state. Callers supply the stored token;
   known legacy aliases normalize to a canonical mode and every other unknown
   value fails closed."
  (:require [clojure.string :as str]))

(def default-mode "north")

(def mode-specs
  [{:name "native"
    :selection "pinned"
    :guard-action "allow"
    :managed-admission "deny"
    :summary "provider-native surface pinned"
    :help "pin every dispatch to the provider-native Agent/Task/Workflow surface."}
   {:name "north"
    :selection "pinned"
    :guard-action "deny"
    :managed-admission "allow"
    :summary "North dispatch surface pinned"
    :help "pin every dispatch to North; provider-native agent calls are denied."}
   {:name "auto"
    :selection "learning-regime"
    :guard-action "allow"
    :managed-admission "allow"
    :summary "system chooses per dispatch via the learning regime"
    :help "choose per dispatch: frozen is deterministic known-best; learning permits bounded experiments."}])

(def legacy-aliases
  {"native-forced" "native"
   "managed-forced" "north"
   "native-biased" "auto"
   "managed-biased" "auto"})

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
           " (legacy aliases: " (str/join "|" (keys legacy-aliases)) ")")
      {:value value
       :canonical (canonical-names)
       :legacy (keys legacy-aliases)}))))

(defn migration-note [value]
  (when (legacy-alias? value)
    (str "dispatch migration: legacy '" value "' → '" (normalize value) "'")))

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
