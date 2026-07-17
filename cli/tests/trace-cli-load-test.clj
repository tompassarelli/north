#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def trace-cli (str root "/cli/trace-cli.clj"))
(def result
  (proc/shell {:out :string :err :string :continue true
               :extra-env {"NORTH_PORT" "59999"}}
              "bb" trace-cli "load-probe"))
(def output (str (:out result) (:err result)))
(def ok? (and (not (str/includes? output "Unable to resolve symbol"))
              (not (str/includes? output "EOF while reading"))
              (str/includes? output "coordinator :59999 unreachable")))

(println (format "  [%s] trace CLI loads before its unavailable-coordinator boundary"
                 (if ok? "PASS" "FAIL")))
(when-not ok?
  (println output))
(System/exit (if ok? 0 1))
