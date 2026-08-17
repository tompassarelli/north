#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def pred-cli (str root "/cli/pred-cli.clj"))
(def fram-out
  (or (some #(when (and % (.isDirectory (io/file %))) %)
            [(System/getenv "FRAM_OUT")
             (some-> (System/getenv "FRAM_TEST_CHECKOUT") (str "/out"))])
      (do
        (binding [*out* *err*]
          (println "predicate test requires FRAM_OUT or FRAM_TEST_CHECKOUT"))
        (System/exit 2))))
(def checks (atom []))

(defn check [label passed?]
  (swap! checks conj [label (boolean passed?)]))

(let [{:keys [exit out err]}
      (if fram-out
        (shell/sh "bb" "-cp" fram-out pred-cli "7977" "lint-offline" "--strict" :dir root)
        {:exit 1 :out "" :err "Fram classpath is unavailable"})]
  (check "offline predicate lint executes the production registry" (zero? exit))
  (check "every fixed v2 projection predicate is registered"
         (and (str/includes? out "clean against bootstrap inventory")
              (not (str/includes? out "absent from bootstrap inventory"))
              (str/blank? err))))

(doseq [[label passed?] @checks]
  (println (format "  [%s] %s" (if passed? "PASS" "FAIL") label)))
(let [passed (count (filter second @checks))]
  (println (format "\npredicate registry behavior: %d / %d PASS" passed (count @checks)))
  (System/exit (if (= passed (count @checks)) 0 1)))
