(ns north.orchestration-root-cwd-test
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [babashka.process :as p]
            [babashka.fs :as fs]))

(def tests-dir (.getParentFile (io/file (System/getProperty "babashka.file"))))

(def cli-dir (.getParentFile tests-dir))

(def ^String expected-orchestration (str (io/file (.getParentFile cli-dir) "agent-machinery")))

(def results (atom []))

(defn check! [^String label ^Boolean pass?]
  (do
  (swap! results conj (boolean pass?))
  (println (format "  %s %s" (if pass? "✓" "✗") label))
  nil))

(println "orchestration-root cwd-independence — daemon-free")

(def ^String unrelated-cwd (str (fs/create-temp-dir {:prefix "north-cwd-test-"})))

(defn probe [^String target-file ^String root-form]
  (let [^String script (str "(load-file \"" target-file "\")" "(println (" root-form "))")
   env (-> (into {} (System/getenv)) (dissoc "NORTH_HOME"))
   result (p/sh {:dir unrelated-cwd :env env :out :string :err :string} "bb" "--eval" script)]
  {:exit (:exit result) :out (str/trim (:out result)) :err (:err result)}))

(let [{:keys [exit out err]} (probe (str (io/file cli-dir "orchestration-project-cli.clj")) "orchestration-root")]
  (check! (str "orchestration-project-cli.clj: exits 0 from unrelated cwd (stderr: " err ")") (zero? exit))
  (check! (str "orchestration-project-cli.clj: resolves from the package owner, not " unrelated-cwd " (got " out ")") (= expected-orchestration out)))

(fs/delete-tree unrelated-cwd)

(let [rs (deref results)
   passed (count (filter true? rs))]
  (println (format "\n%d/%d cwd-independence checks passed" passed (count rs)))
  (System/exit (if (every? true? rs) 0 1)))
