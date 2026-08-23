(ns north.agent-catalog-cli
  (:require [cheshire.core :as json]
            [clojure.string :as str]
            [north.agent-catalog :as catalog]))

(def usage
  (str "usage: north config agents "
       "[sync [--json]|status [--json]|on|off <UnitId> [--json]|skills|hooks|sets [list] [--json]|"
       "path <id> [--json]|inspect <id> [--json]]"))

(defn- fail [message]
  (throw (ex-info message {})))

(defn- json! [value]
  (println (json/generate-string value {:pretty true})))

(defn- current! []
  (or (catalog/current-activation)
      (fail (str "no current agent activation at " (catalog/agents-root)
                 "/current/activation.json; run `north config agents sync`"))))

(defn- owner-label [owner]
  (str (get owner "repo") ":" (get owner "path")))

(defn- print-unit [unit]
  (println
   (format "%-34s %-5s %-27s %s"
           (get unit "id")
           (get unit "kind")
           (str (get unit "permission") " · "
                (if (get unit "active") "active" "inactive"))
           (owner-label (get unit "owner")))))

(defn- select-kind [activation kind]
  (assoc activation "units"
         (filterv #(= kind (get % "kind")) (get activation "units"))))

(defn- output-activation! [activation json?]
  (if json?
    (json! activation)
    (do
      (println (str "generation: " (get activation "generationId")))
      (println (str "catalog:    " (get activation "catalogDigest")))
      (doseq [unit (get activation "units")] (print-unit unit)))))

(defn- split-json [args]
  (let [json? (some #{"--json"} args)
        rest (vec (remove #{"--json"} args))]
    (when (> (count args) (+ (count rest) (if json? 1 0)))
      (fail usage))
    [rest (boolean json?)]))

(defn- kind-command! [kind args]
  (let [[args json?] (split-json args)
        [verb & extra] args]
    (case (or verb "list")
      "list"
      (do
        (when (seq extra) (fail usage))
        (output-activation! (select-kind (current!) kind) json?))
      (fail usage))))

(defn cmd-agents [args]
  (let [[verb & rest] args]
    (case (or verb "status")
      "status"
      (let [[args json?] (split-json rest)]
        (when (seq args) (fail usage))
        (output-activation! (current!) json?))

      "sync"
      (let [[args json?] (split-json rest)]
        (when (seq args) (fail usage))
        (let [activation (catalog/sync!)]
          (if json?
            (json! activation)
            (println (str "agents synchronized → " (catalog/agents-root)
                          "/current (" (count (filter #(get % "active")
                                                      (get activation "units")))
                          "/" (count (get activation "units"))
                          " active)")))))

      ("on" "off")
      (let [[args json?] (split-json rest)
            [id & extra] args]
        (when (or (nil? id) (seq extra)) (fail usage))
        (catalog/unit id)
        (let [activation (catalog/change-permissions! {id verb})]
          (if json?
            (json! activation)
            (println (str id " → " verb " · generation "
                          (get activation "generationId"))))))

      "skills" (kind-command! "skill" rest)
      "hooks" (kind-command! "hook" rest)
      "sets" (kind-command! "set" rest)

      "path"
      (let [[args json?] (split-json rest)
            [id & extra] args]
        (when (or (nil? id) (seq extra)) (fail usage))
        (let [unit (catalog/unit id)
              path (catalog/unit-path id)]
          (if json?
            (json! {"id" id "kind" (get unit "kind")
                    "owner" (get unit "owner") "path" path})
            (println path))))

      "inspect"
      (let [[args json?] (split-json rest)
            [id & extra] args]
        (when (or (nil? id) (seq extra)) (fail usage))
        (let [unit (assoc (catalog/unit id) "resolvedOwnerPath" (catalog/unit-path id))]
          (if json?
            (json! unit)
            (do
              (print-unit unit)
              (println (str "  trigger: " (get unit "triggerDescription")))
              (println (str "  members: " (if (seq (get unit "members"))
                                             (str/join ", " (get unit "members")) "none")))
              (println (str "  supports: " (if (seq (get unit "supports"))
                                              (str/join ", " (get unit "supports")) "none")))
              (doseq [path (get unit "activationPaths")]
                (println (str "  active via: " (str/join " → " path))))))))

      (fail usage))))
