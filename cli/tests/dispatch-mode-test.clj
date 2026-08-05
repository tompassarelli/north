#!/usr/bin/env bb
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/dispatch-mode.clj"))

(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))

(def expected
  [["native" "allow" "deny"]
   ["managed" "deny" "allow"]
   ["auto" "allow" "allow"]])

(def home (.toFile (java.nio.file.Files/createTempDirectory
                    "north-dispatch-mode-"
                    (make-array java.nio.file.attribute.FileAttribute 0))))
(def home-path (.getCanonicalPath home))
(def state (str (io/file home ".local/state/north/harness.conf")))

(defn config [& args]
  (apply
   p/shell
   {:out :string :err :string :continue true
    :extra-env {"HOME" home-path
                "NORTH_HOME" root
                "NORTH_HARNESS_STATE" state
                "NORTH_LEGACY_HARNESS_STATE" (str (io/file home "legacy.conf"))}}
   (concat ["bb" (str root "/cli/config-cli.clj")] args)))

(try
  (check "canonical vocabulary is complete and ordered"
         (= (mapv first expected) (north.dispatch-mode/canonical-names)))
  (check "usage exposes exactly the canonical triple"
         (= "native|managed|auto" (north.dispatch-mode/usage)))
  (check "default is the managed surface"
         (= "managed" north.dispatch-mode/default-mode))

  (doseq [[mode guard admission] expected]
    (check (str mode " normalizes to itself")
           (= mode (north.dispatch-mode/normalize mode)))
    (check (str mode " supplies the spawn-guard action")
           (= guard (north.dispatch-mode/guard-action mode)))
    (check (str mode " supplies the managed-admission action")
           (= admission (north.dispatch-mode/managed-admission mode)))
    (let [set-result (config "dispatch" mode)
          canonical (config "dispatch" "--canonical")
          guard-result (config "dispatch" "--guard-action")
          admission-result (config "dispatch" "--managed-admission")
          display (config "dispatch")]
      (check (str mode " set succeeds silently")
             (and (zero? (:exit set-result))
                  (str/includes? (:out set-result) (str "dispatch → " mode))
                  (str/blank? (:err set-result))))
      (check (str mode " round-trips through canonical persistence")
             (= (str "dispatch=" mode "\n") (slurp state)))
      (check (str mode " is the config canonical selection")
             (and (zero? (:exit canonical))
                  (= mode (str/trim (:out canonical)))
                  (str/blank? (:err canonical))))
      (check (str mode " drives the config spawn-guard contract")
             (and (zero? (:exit guard-result))
                  (= guard (str/trim (:out guard-result)))))
      (check (str mode " drives central managed admission")
             (and (zero? (:exit admission-result))
                  (= admission (str/trim (:out admission-result)))))
      (check (str mode " is rendered as the selected config mode")
             (and (zero? (:exit display))
                  (str/includes? (:out display) (str "dispatch = " mode))))))

  (io/delete-file state true)
  (let [canonical (config "dispatch" "--canonical")
        guard-result (config "dispatch" "--guard-action")
        admission-result (config "dispatch" "--managed-admission")]
    (check "missing state uses the canonical managed default"
           (and (= "managed" (str/trim (:out canonical)))
                (= "deny" (str/trim (:out guard-result)))
                (= "allow" (str/trim (:out admission-result))))))

  (let [outside-vocabulary (config "dispatch" "somewhere-else")]
    (check "a value outside the canonical triple is refused by name"
           (and (not (zero? (:exit outside-vocabulary)))
                (str/includes? (:err outside-vocabulary)
                               "usage: north config dispatch [native|managed|auto]"))))

  (io/make-parents state)
  (spit state "dispatch=surprise\n")
  (doseq [[label args] [["canonical lookup" ["dispatch" "--canonical"]]
                        ["spawn-guard lookup" ["dispatch" "--guard-action"]]
                        ["managed-admission lookup" ["dispatch" "--managed-admission"]]]]
    (let [result (apply config args)]
      (check (str "unknown persisted mode fails " label " loudly")
             (and (not (zero? (:exit result)))
                  (str/includes? (:err result) "invalid dispatch mode \"surprise\"")
                  (str/blank? (:out result))))))

  (finally
    (doseq [file (reverse (file-seq home))] (io/delete-file file true))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\ndispatch mode: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
