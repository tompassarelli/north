#!/usr/bin/env bb
;; Isolated round-trip/validation tests for `north config routing`.
(require '[babashka.process :as p]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir (.toFile (java.nio.file.Files/createTempDirectory "north-routing-test-"
                       (make-array java.nio.file.attribute.FileAttribute 0))))
(def policy (str tmp-dir "/routing-policy.json"))
(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))
(defn run! [& args]
  (apply p/shell
         {:out :string :err :string :continue true
          :extra-env {"NORTH_ROUTING_POLICY" policy}}
         (into ["bb" cli "routing"] args)))
(defn data [] (json/parse-string (slurp policy) false))

(try
  (let [show (run!)]
    (check "no-arg show exposes obvious defaults" (and (zero? (:exit show))
                                                        (str/includes? (:out show) "mode preferential")
                                                        (str/includes? (:out show) "anthropic → openai")
                                                        (str/includes? (:out show) "anthropic · auth ambient"))))

  (doseq [args [["mode" "balanced"]
                ["target" "add" "claude-work" "anthropic" "work"]
                ["target" "add" "claude-isolated" "anthropic" "work_2" "--auth-mode" "isolated"]
                ["order" "claude-work" "openai"]
                ["weight" "claude-work" "3"]
                ["reserve" "claude-work"]
                ["pressure" "claude-work" "low" "--until" "2026-08-01T00:00:00Z"]
                ["envelope" "set" "month" "runs" "40"]
                ["envelope" "set" "project:north" "frontierRuns" "8"]]]
    (let [r (apply run! args)] (check (str "command succeeds: " (str/join " " args)) (zero? (:exit r)))))

  (let [j (data)]
    (check "round-trip mode" (= "balanced" (get j "mode")))
    (check "round-trip target profile" (some #(= {"id" "claude-work" "provider" "anthropic" "profile" "work"} %)
                                              (get j "targets")))
    (check "legacy profile does not imply isolated auth" (some #(and (= "claude-work" (get % "id"))
                                                                      (not (contains? % "authMode")))
                                                                (get j "targets")))
    (check "isolated auth mode round trips losslessly" (some #(= {"id" "claude-isolated" "provider" "anthropic"
                                                                   "authMode" "isolated" "profile" "work_2"} %)
                                                           (get j "targets")))
    (check "round-trip order" (= ["claude-work" "openai"] (get j "targetOrder")))
    (check "round-trip weight/reserve" (and (= 3 (get-in j ["weights" "claude-work"]))
                                             (= "claude-work" (get j "reservedFrontierTarget"))))
    (check "pressure stamps observation and expiry"
           (and (= "low" (get-in j ["pressures" "claude-work" "state"]))
                (string? (get-in j ["pressures" "claude-work" "observedAt"]))
                (= "2026-08-01T00:00:00Z" (get-in j ["pressures" "claude-work" "until"]))))
    (check "nested envelopes persist" (and (= 40 (get-in j ["envelopes" "month" "runs"]))
                                            (= 8 (get-in j ["envelopes" "projects" "north" "frontierRuns"])))))

  ;; True boundary test: the TypeScript loader consumes the exact file emitted
  ;; by the Clojure CLI, rather than a separately maintained fixture.
  (let [script (str "import { loadResourcePolicy } from '" root "/sdk/src/resource-policy.ts';"
                    "const p=loadResourcePolicy();"
                    "const legacy=p?.targets?.find(t=>t.id==='claude-work');"
                    "const isolated=p?.targets?.find(t=>t.id==='claude-isolated');"
                    "if(p?.targetOrder[0]!=='claude-work'||p?.envelopes?.projects?.north?.frontierRuns!==8||legacy?.authMode!=='ambient'||isolated?.authMode!=='isolated')process.exit(9);")
        loaded (p/shell {:out :string :err :string :continue true
                         :extra-env {"NORTH_ROUTING_POLICY" policy}}
                        "bun" "-e" script)]
    (check "CLI output loads through SDK canonical schema" (zero? (:exit loaded))))

  (let [before (slurp policy)]
    (doseq [[label args] [["unknown target rejected" ["order" "missing"]]
                          ["zero weight rejected" ["weight" "claude-work" "0"]]
                          ["bad pressure rejected" ["pressure" "claude-work" "empty"]]
                          ["bad expiry rejected" ["pressure" "claude-work" "low" "--until" "tomorrow"]]
                          ["isolated target without profile rejected" ["target" "add" "missing-profile" "anthropic" "--auth-mode" "isolated"]]
                          ["isolated target traversal rejected" ["target" "add" "traversal" "anthropic" "../work" "--auth-mode" "isolated"]]
                          ["isolated target path rejected" ["target" "add" "path" "anthropic" "work/team" "--auth-mode" "isolated"]]
                          ["bad envelope scope rejected" ["envelope" "set" "day" "runs" "2"]]]]
      (let [r (apply run! args)]
        (check label (not (zero? (:exit r))))
        (check (str label " leaves atomic state unchanged") (= before (slurp policy)))))
    (check "atomic replacement leaves no temp files"
           (empty? (filter #(str/starts-with? (.getName %) ".routing-policy.") (.listFiles tmp-dir)))))

  (let [r (run! "envelope" "clear" "month" "runs")]
    (check "envelope clear succeeds" (zero? (:exit r)))
    (check "envelope clear removes empty scope" (nil? (get-in (data) ["envelopes" "month"]))))
  (let [r (run! "target" "remove" "claude-work")]
    (check "target remove succeeds" (zero? (:exit r)))
    (check "target remove clears every reference"
           (let [j (data)]
             (and (not-any? #(= "claude-work" (get % "id")) (get j "targets"))
                  (nil? (get-in j ["weights" "claude-work"]))
                  (nil? (get-in j ["pressures" "claude-work"]))
                  (not-any? #{"claude-work"} (get j "targetOrder"))
                  (nil? (get j "reservedFrontierTarget"))))))

  (finally
    (doseq [f (reverse (file-seq tmp-dir))] (io/delete-file f true))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok] results] (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig routing: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
