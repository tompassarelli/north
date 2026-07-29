#!/usr/bin/env bb
;; Asserts the Clojure dial reader against the SAME table the bash lib and the
;; TS SDK assert against. Three readers of one state file drift unless one
;; artifact holds them; this is that artifact's Clojure half.

(require '[clojure.string :as str]
         '[clojure.java.io :as io])

(def repo-root (-> *file* io/file .getCanonicalFile .getParentFile .getParentFile .getParentFile))
(load-file (str repo-root "/cli/harness-dial.clj"))

(def cases-path (str repo-root "/agent-profile/hooks/harness-dial-cases.tsv"))
(def registry-path (str repo-root "/agent-profile/hooks/registry.tsv"))

(def pass (atom 0))
(def failures (atom []))

(defn check [label expected actual]
  (if (= expected actual)
    (swap! pass inc)
    (swap! failures conj (format "%-44s expected %s, got %s" label expected actual))))

(defn dash->nil [s] (when-not (= s "-") s))

;; --- the shared precedence algebra ----------------------------------------
(let [lines (->> (slurp cases-path)
                 str/split-lines
                 (remove #(or (str/blank? %) (str/starts-with? % "#")))
                 (remove #(str/starts-with? % "id\t")))]
  (when (empty? lines)
    (println "FAIL: case table is empty —" cases-path)
    (System/exit 1))
  (doseq [line lines]
    (let [[id all cat item env now expect] (str/split line #"\t")
          env-raw (when-let [e (dash->nil env)] (subs e (inc (str/index-of e "="))))
          env-dec (north.harness-dial/env-decision env-raw)
          [verdict _] (north.harness-dial/resolve-dial
                       (dash->nil all) (dash->nil cat) (dash->nil item)
                       env-dec
                       (or (dash->nil now) (north.harness-dial/now-iso)))]
      (check id expect verdict))))

;; --- registry integration: the two special categories ----------------------
(let [registry (north.harness-dial/read-registry registry-path)
      verdict (fn [state id]
                (first (north.harness-dial/hook-verdict
                        (fn [k] (get state k)) registry id)))]
  (check "registry-parses" true (>= (count registry) 10))
  (check "authoring-category" "authoring"
         (north.harness-dial/category-of registry "tripwire-guard"))
  (check "dispatch-is-its-own-category" "dispatch"
         (north.harness-dial/category-of registry "agent-spawn-guard"))

  (check "all-sweeps-authoring" "off"
         (verdict {"hooks" "off"} "tripwire-guard"))
  (check "all-never-sweeps-coordination" "on"
         (verdict {"hooks" "off"} "north-session-end"))
  (check "guards-is-authoring-category" "off"
         (verdict {"guards" "off"} "tripwire-guard"))
  (check "guards-does-not-reach-dispatch" "on"
         (verdict {"guards" "off"} "agent-spawn-guard"))
  (check "guards-does-not-reach-billing" "on"
         (verdict {"guards" "off"} "north-clock-guard"))
  (check "item-on-beats-guards-off" "on"
         (verdict {"guards" "off" "hooks.hook.tripwire-guard" "on"} "tripwire-guard"))
  (check "sibling-verdict-unchanged" "off"
         (verdict {"guards" "off" "hooks.hook.tripwire-guard" "on"} "firn-guard"))
  (check "coordination-off-when-named" "off"
         (verdict {"hooks.cat.coordination" "off"} "north-session-end"))
  (check "lapsed-ttl-restores-guard" "on"
         (verdict {"hooks.hook.north-clock-guard" "off:until=2020-01-01T00:00:00Z"}
                  "north-clock-guard"))

  ;; every deny-capable hook must demand a TTL, or a silent permanent disable
  ;; of a launch-critical guard is one keystroke away
  (doseq [{:keys [id kind ttl-required?]} registry]
    (when (= kind "deny")
      (check (str "ttl-required/" id) true ttl-required?))))

(println)
(doseq [f @failures] (println "FAIL " f))
(printf "%d passed, %d failed%n" @pass (count @failures))
(flush) ; System/exit does not flush stdout; without this the report vanishes
(System/exit (if (empty? @failures) 0 1))
